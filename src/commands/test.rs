use crate::App;
use crate::cli::TestArgs;
use crate::comparison::{
    SchemaSnapshot, compare_snapshots, generate_xml, get_postgres_version, xml_escape,
};
use crate::config::Config;
use crate::db::postgres::migration::{execute_migration_for_test, load_and_validate_migration};
use crate::db::postgres::{DbConnection, DbPool};
use anyhow::Context;
use futures_util::future::join_all;
use std::collections::{BTreeSet, HashMap};

pub async fn run_test(app: &'static App, args: TestArgs) -> anyhow::Result<()> {
    let config = Config::load(&app.path).context("Failed to load CONFIG.toml")?;

    let new_config = config
        .new
        .as_ref()
        .context("No 'new' database configured in CONFIG.toml")?;

    let old_config = config
        .old
        .as_ref()
        .context("No 'old' database configured in CONFIG.toml")?;

    // Read and validate migration file
    let migration_path = app.path.join(&args.migration_file);
    let migration_sql = load_and_validate_migration(&migration_path)?;

    eprintln!("Testing migration: {}", migration_path.display());

    // Connect to everything at once:
    // - a pool on the new database (comparison baseline)
    // - one connection on the old database, which carries the migration's
    //   transaction: everything that must see the migrated state runs on it
    // - with --row-counts, a pool on the old database for the "before" counts,
    //   which are taken outside the transaction
    eprintln!("Connecting to new (target) and old (source) databases...");
    let old_connection_string = old_config.connection_string();
    let (new_pool, old_txn, old_pool) = tokio::try_join!(
        async {
            DbPool::connect(&new_config.connection_string())
                .await
                .context("Failed to connect to new database")
        },
        async {
            DbConnection::connect(&old_connection_string)
                .await
                .map(DbPool::single)
                .context("Failed to connect to old database")
        },
        async {
            if !args.row_counts {
                return Ok(None);
            }
            DbPool::connect(&old_connection_string)
                .await
                .map(Some)
                .context("Failed to connect to old database")
        },
    )?;

    // Get version info
    let (new_version, old_version) = tokio::try_join!(
        async { get_postgres_version(&*new_pool.get().await?).await },
        async { get_postgres_version(&*old_txn.get().await?).await },
    )?;

    let exclude_like: Vec<String> = args
        .row_counts_exclude
        .iter()
        .map(|pattern| glob_to_like(pattern))
        .collect();

    // The new database is independent of the migration, so its schema is
    // fetched while the old database is being counted and migrated.
    let new_side = SchemaSnapshot::fetch(&new_pool);

    let old_side = async {
        // Capture row counts before migration (outside of transaction)
        let row_counts_before = match &old_pool {
            Some(pool) => {
                eprintln!("Capturing row counts (before)...");
                Some(capture_row_counts(pool, &exclude_like).await?)
            }
            None => None,
        };

        // Apply migration - execute statements one at a time for detailed error reporting
        // This executes BEGIN and all statements except the final ROLLBACK
        eprintln!("Applying migration...");
        execute_migration_for_test(&*old_txn.get().await?, &migration_sql).await?;
        eprintln!("Migration applied successfully.");

        // Capture row counts after migration
        let row_counts_section = match &row_counts_before {
            Some(before) => {
                eprintln!("Capturing row counts (after)...");
                let after = capture_row_counts(&old_txn, &exclude_like).await?;
                row_counts_xml(&args.row_counts_exclude, before, &after)
            }
            None => "  <row_counts skipped=\"true\">Run pgcmp test --row-counts to compare row counts before and after the migration.</row_counts>\n".to_string(),
        };

        // Old database with the migration applied
        eprintln!("Comparing schemas...");
        let snapshot = SchemaSnapshot::fetch(&old_txn).await?;

        anyhow::Ok((row_counts_section, snapshot))
    };

    // Compare schemas: new database vs old database (with migration applied)
    let (new_snapshot, (row_counts_section, old_snapshot)) = tokio::try_join!(new_side, old_side)?;
    let analysis = compare_snapshots(&new_snapshot, &old_snapshot);

    // Generate XML output
    let xml = generate_xml(
        &analysis,
        &new_config.connection_string(),
        &old_config.connection_string(),
        &new_version,
        &old_version,
        &row_counts_section,
    );

    // Rollback transaction
    eprintln!("Rolling back transaction...");
    old_txn
        .get()
        .await?
        .client()
        .execute("ROLLBACK", &[])
        .await?;

    // Print XML output
    println!("{}", xml);

    // Report results (always exit 0 on successful execution)
    if analysis.has_differences() {
        eprintln!(
            "\nMigration leaves {} difference(s) - schemas do not match.",
            analysis.count_differences()
        );
    } else {
        eprintln!("\nMigration successful - schemas match after applying migration.");
        if !args.row_counts {
            eprintln!("Run `pgcmp test --row-counts` for a final check of row counts.");
        }
    }

    Ok(())
}

/// Convert a glob pattern (`*`, `?`) to a SQL `LIKE` pattern, escaping the
/// characters `LIKE` treats specially so `_` and `%` in schema names match
/// literally.
fn glob_to_like(pattern: &str) -> String {
    let mut like = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        match ch {
            '*' => like.push('%'),
            '?' => like.push('_'),
            '%' | '_' | '\\' => {
                like.push('\\');
                like.push(ch);
            }
            _ => like.push(ch),
        }
    }
    like
}

/// Row counts captured at one point in time.
struct RowCountSnapshot {
    /// `schema.table` -> row count.
    counts: HashMap<String, i64>,
    /// Schemas skipped for each exclude pattern, indexed like the patterns.
    excluded_schemas: Vec<BTreeSet<String>>,
}

/// Count rows in every table, skipping schemas that match any of the
/// `exclude_like` patterns (SQL `LIKE` syntax, see `glob_to_like`).
///
/// Tables are counted concurrently, one per pooled connection.
async fn capture_row_counts(
    pool: &DbPool,
    exclude_like: &[String],
) -> anyhow::Result<RowCountSnapshot> {
    let conn = pool.get().await?;
    let excluded_schemas = capture_excluded_schemas(&conn, exclude_like).await?;

    // Unlike fetch_tables, extension-owned tables are intentionally included:
    // row counts verify data changes, and migrations can legitimately write to
    // extension tables (e.g. config tables).
    // First get list of all tables
    let tables = conn
        .client()
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                c.relname AS table_name
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind = 'r'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND left(n.nspname, 2) <> 'x_'
              AND NOT (n.nspname LIKE ANY($1::text[]))
            ORDER BY n.nspname, c.relname
            "#,
            &[&exclude_like],
        )
        .await?;

    // Return the connection before the counts start checking them out
    drop(conn);

    // Get actual COUNT(*) for each table
    let results = join_all(tables.iter().map(|table_row| async move {
        let schema_name: String = table_row.get("schema_name");
        let table_name: String = table_row.get("table_name");
        let full_name = format!("{}.{}", schema_name, table_name);

        // Use quoted identifiers for safety
        let query = format!(
            "SELECT COUNT(*) FROM \"{}\".\"{}\"",
            schema_name.replace('"', "\"\""),
            table_name.replace('"', "\"\"")
        );

        let conn = pool.get().await?;
        let count = match conn.client().query_one(&query, &[]).await {
            Ok(row) => Some(row.get::<_, i64>(0)),
            Err(e) => {
                eprintln!("Warning: Could not count rows in {}: {}", full_name, e);
                None
            }
        };
        anyhow::Ok((full_name, count))
    }))
    .await;

    let mut counts = HashMap::new();
    for result in results {
        if let (full_name, Some(count)) = result? {
            counts.insert(full_name, count);
        }
    }

    Ok(RowCountSnapshot {
        counts,
        excluded_schemas,
    })
}

/// List the schemas each `exclude_like` pattern matches, among the schemas row
/// counting would otherwise include.
async fn capture_excluded_schemas(
    conn: &DbConnection,
    exclude_like: &[String],
) -> anyhow::Result<Vec<BTreeSet<String>>> {
    let mut excluded = vec![BTreeSet::new(); exclude_like.len()];
    if exclude_like.is_empty() {
        return Ok(excluded);
    }

    let rows = conn
        .client()
        .query(
            r#"
            SELECT p.ord, n.nspname AS schema_name
            FROM unnest($1::text[]) WITH ORDINALITY AS p(like_pattern, ord)
            JOIN pg_namespace n ON n.nspname LIKE p.like_pattern
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND n.nspname NOT LIKE 'pg_toast_temp_%'
              AND left(n.nspname, 2) <> 'x_'
            "#,
            &[&exclude_like],
        )
        .await?;

    for row in rows {
        let ord: i64 = row.get("ord");
        let schema_name: String = row.get("schema_name");
        // WITH ORDINALITY numbers from 1.
        let slot = usize::try_from(ord - 1)
            .ok()
            .and_then(|index| excluded.get_mut(index));
        if let Some(schemas) = slot {
            schemas.insert(schema_name);
        }
    }

    Ok(excluded)
}

/// Render the `<row_counts>` and `<row_counts_total>` report sections.
/// `exclude_globs` are the `--row-counts-exclude` patterns as given.
fn row_counts_xml(
    exclude_globs: &[String],
    before: &RowCountSnapshot,
    after: &RowCountSnapshot,
) -> String {
    let row_counts_before = &before.counts;
    let row_counts_after = &after.counts;
    let mut xml = String::new();

    // Row counts section
    xml.push_str("  <row_counts>\n");

    // Schemas matched before or after the migration, per exclude pattern
    if !exclude_globs.is_empty() {
        xml.push_str("    <excluded_schemas>\n");
        for (index, glob) in exclude_globs.iter().enumerate() {
            let schemas: BTreeSet<&String> = before
                .excluded_schemas
                .get(index)
                .into_iter()
                .chain(after.excluded_schemas.get(index))
                .flatten()
                .collect();
            if schemas.is_empty() {
                xml.push_str(&format!(
                    "      <pattern glob=\"{}\" />\n",
                    xml_escape(glob)
                ));
                continue;
            }
            xml.push_str(&format!("      <pattern glob=\"{}\">\n", xml_escape(glob)));
            for schema in schemas {
                xml.push_str(&format!(
                    "        <schema>{}</schema>\n",
                    xml_escape(schema)
                ));
            }
            xml.push_str("      </pattern>\n");
        }
        xml.push_str("    </excluded_schemas>\n");
    }

    let mut all_tables: Vec<&String> = row_counts_before
        .keys()
        .chain(row_counts_after.keys())
        .collect();
    all_tables.sort();
    all_tables.dedup();

    for table in all_tables {
        let before_count = row_counts_before.get(table).copied();
        let after_count = row_counts_after.get(table).copied();

        match (before_count, after_count) {
            (Some(b), Some(a)) if b != a => {
                xml.push_str("    <row_count>\n");
                xml.push_str(&format!("      <name>{}</name>\n", xml_escape(table)));
                xml.push_str("      <action>modified</action>\n");
                xml.push_str(&format!("      <before_count>{}</before_count>\n", b));
                xml.push_str(&format!("      <after_count>{}</after_count>\n", a));
                let change = a - b;
                let change_str = if change > 0 {
                    format!("+{}", change)
                } else {
                    change.to_string()
                };
                xml.push_str(&format!("      <change>{}</change>\n", change_str));
                xml.push_str("    </row_count>\n");
            }
            (Some(b), None) => {
                xml.push_str("    <row_count>\n");
                xml.push_str(&format!("      <name>{}</name>\n", xml_escape(table)));
                xml.push_str("      <action>removed</action>\n");
                xml.push_str(&format!("      <rows_removed>{}</rows_removed>\n", b));
                xml.push_str("    </row_count>\n");
            }
            (None, Some(a)) => {
                xml.push_str("    <row_count>\n");
                xml.push_str(&format!("      <name>{}</name>\n", xml_escape(table)));
                xml.push_str("      <action>added</action>\n");
                xml.push_str(&format!("      <rows_added>{}</rows_added>\n", a));
                xml.push_str("    </row_count>\n");
            }
            _ => {}
        }
    }

    xml.push_str("  </row_counts>\n");

    // Row counts total section
    let total_before: i64 = row_counts_before.values().sum();
    let total_after: i64 = row_counts_after.values().sum();
    let total_change = total_after - total_before;

    xml.push_str("  <row_counts_total>\n");
    xml.push_str(&format!("    <before>{}</before>\n", total_before));
    xml.push_str(&format!("    <after>{}</after>\n", total_after));
    let change_str = if total_change > 0 {
        format!("+{}", total_change)
    } else {
        total_change.to_string()
    };
    xml.push_str(&format!("    <change>{}</change>\n", change_str));
    xml.push_str("  </row_counts_total>\n");

    xml
}

#[cfg(test)]
mod tests {
    use super::{RowCountSnapshot, glob_to_like, row_counts_xml};
    use std::collections::{BTreeSet, HashMap};

    fn snapshot(excluded_schemas: &[&[&str]]) -> RowCountSnapshot {
        RowCountSnapshot {
            counts: HashMap::new(),
            excluded_schemas: excluded_schemas
                .iter()
                .map(|schemas| schemas.iter().map(|s| s.to_string()).collect())
                .collect::<Vec<BTreeSet<String>>>(),
        }
    }

    #[test]
    fn row_counts_xml_lists_excluded_schemas_per_pattern() {
        let globs = vec!["audit*".to_string(), "nomatch?".to_string()];
        // `audit_new` only exists after the migration.
        let before = snapshot(&[&["audit"], &[]]);
        let after = snapshot(&[&["audit", "audit_new"], &[]]);

        let xml = row_counts_xml(&globs, &before, &after);

        assert!(xml.contains(
            "    <excluded_schemas>\n      <pattern glob=\"audit*\">\n        <schema>audit</schema>\n        <schema>audit_new</schema>\n      </pattern>\n      <pattern glob=\"nomatch?\" />\n    </excluded_schemas>\n"
        ));
    }

    #[test]
    fn row_counts_xml_omits_excluded_schemas_without_patterns() {
        let xml = row_counts_xml(&[], &snapshot(&[]), &snapshot(&[]));
        assert!(!xml.contains("excluded_schemas"));
    }

    #[test]
    fn glob_to_like_translates_wildcards() {
        assert_eq!(glob_to_like("audit*"), "audit%");
        assert_eq!(glob_to_like("log?"), "log_");
        assert_eq!(glob_to_like("archive"), "archive");
    }

    #[test]
    fn glob_to_like_escapes_like_metacharacters() {
        assert_eq!(glob_to_like("tmp_*"), "tmp\\_%");
        assert_eq!(glob_to_like("100%"), "100\\%");
        assert_eq!(glob_to_like("a\\b"), "a\\\\b");
    }
}
