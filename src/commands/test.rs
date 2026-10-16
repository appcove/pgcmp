use crate::App;
use crate::cli::TestArgs;
use crate::comparison::{analyze_databases, generate_xml, get_postgres_version, xml_escape};
use crate::config::Config;
use crate::db::postgres::DbConnection;
use crate::db::postgres::migration::{execute_migration_for_test, load_and_validate_migration};
use anyhow::Context;
use std::collections::HashMap;

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

    // Capture row counts before migration (outside of transaction)
    eprintln!("Capturing row counts (before)...");
    let row_counts_before = capture_row_counts(&old_conn).await?;

    // Apply migration - execute statements one at a time for detailed error reporting
    // This executes BEGIN and all statements except the final ROLLBACK
    eprintln!("Applying migration...");
    execute_migration_for_test(&old_conn, &migration_sql).await?;
    eprintln!("Migration applied successfully.");

    // Capture row counts after migration
    eprintln!("Capturing row counts (after)...");
    let row_counts_after = capture_row_counts(&old_conn).await?;

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
        &row_counts_xml(&row_counts_before, &row_counts_after),
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
    }

    Ok(())
}

async fn capture_row_counts(conn: &DbConnection) -> anyhow::Result<HashMap<String, i64>> {
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
            ORDER BY n.nspname, c.relname
            "#,
            &[],
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

    Ok(counts)
}

/// Render the `<row_counts>` and `<row_counts_total>` report sections.
fn row_counts_xml(
    row_counts_before: &HashMap<String, i64>,
    row_counts_after: &HashMap<String, i64>,
) -> String {
    let mut xml = String::new();

    // Row counts section
    xml.push_str("  <row_counts>\n");

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
