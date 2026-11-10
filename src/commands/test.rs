use crate::App;
use crate::cli::TestArgs;
use crate::comparison::{analyze_databases, generate_xml, get_postgres_version, xml_escape};
use crate::config::Config;
use crate::db::postgres::DbConnection;
use crate::db::postgres::migration::{execute_migration_for_test, load_and_validate_migration};
use anyhow::Context;
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

    // Connect to new database (for comparison baseline)
    eprintln!("Connecting to new (target) database...");
    let new_conn = DbConnection::connect(&new_config.connection_string())
        .await
        .context("Failed to connect to new database")?;

    // Connect to old database (where we'll apply the migration)
    eprintln!("Connecting to old (source) database...");
    let old_conn = DbConnection::connect(&old_config.connection_string())
        .await
        .context("Failed to connect to old database")?;

    // Get version info
    let new_version = get_postgres_version(&new_conn).await?;
    let old_version = get_postgres_version(&old_conn).await?;

    let exclude_like: Vec<String> = args
        .row_counts_exclude
        .iter()
        .map(|pattern| glob_to_like(pattern))
        .collect();

    // Capture row counts before migration (outside of transaction)
    let row_counts_before = if args.row_counts {
        eprintln!("Capturing row counts (before)...");
        Some(capture_row_counts(&old_conn, &exclude_like).await?)
    } else {
        None
    };

    // Apply migration - execute statements one at a time for detailed error reporting
    // This executes BEGIN and all statements except the final ROLLBACK
    eprintln!("Applying migration...");
    execute_migration_for_test(&old_conn, &migration_sql).await?;
    eprintln!("Migration applied successfully.");

    // Capture row counts after migration
    let row_counts_section = match &row_counts_before {
        Some(before) => {
            eprintln!("Capturing row counts (after)...");
            let after = capture_row_counts(&old_conn, &exclude_like).await?;
            row_counts_xml(&args.row_counts_exclude, before, &after)
        }
        None => "  <row_counts skipped=\"true\">Run pgcmp test --row-counts to compare row counts before and after the migration.</row_counts>\n".to_string(),
    };

    // Compare schemas: new database vs old database (with migration applied)
    eprintln!("Comparing schemas...");
    let analysis = analyze_databases(&new_conn, &old_conn).await?;

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
    old_conn.client().execute("ROLLBACK", &[]).await?;

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
        if row_counts_before.is_none() {
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
async fn capture_row_counts(
    conn: &DbConnection,
    exclude_like: &[String],
) -> anyhow::Result<RowCountSnapshot> {
    let excluded_schemas = capture_excluded_schemas(conn, exclude_like).await?;

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

    let mut counts = HashMap::new();

    // Get actual COUNT(*) for each table
    for table_row in tables {
        let schema_name: String = table_row.get("schema_name");
        let table_name: String = table_row.get("table_name");
        let full_name = format!("{}.{}", schema_name, table_name);

        // Use quoted identifiers for safety
        let query = format!(
            "SELECT COUNT(*) FROM \"{}\".\"{}\"",
            schema_name.replace('"', "\"\""),
            table_name.replace('"', "\"\"")
        );

        match conn.client().query_one(&query, &[]).await {
            Ok(row) => {
                let count: i64 = row.get(0);
                counts.insert(full_name, count);
            }
            Err(e) => {
                eprintln!("Warning: Could not count rows in {}: {}", full_name, e);
            }
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
