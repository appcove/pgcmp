use crate::App;
use crate::cli::TestArgs;
use crate::config::Config;
use crate::db::DbConnection;
use crate::db::constraints::fetch_constraints;
use crate::db::functions::fetch_functions;
use crate::db::indexes::fetch_indexes;
use crate::db::sequences::fetch_sequences;
use crate::db::tables::fetch_tables;
use crate::db::triggers::fetch_triggers;
use crate::db::views::{fetch_materialized_views, fetch_views};
use anyhow::{bail, Context};
use std::collections::{HashMap, HashSet};
use std::fs;

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

    // Read migration file
    let migration_path = app.path.join(&args.migration_file);
    let migration_sql = fs::read_to_string(&migration_path)
        .with_context(|| format!("Failed to read migration file: {}", migration_path.display()))?;

    // Validate migration SQL - check for forbidden keywords
    validate_migration_sql(&migration_sql)?;

    if migration_sql.trim().is_empty() {
        bail!("Migration file is empty: {}", migration_path.display());
    }

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

    // Begin transaction on old database
    eprintln!("Beginning transaction...");
    old_conn.client().execute("BEGIN", &[]).await?;

    // Capture row counts before migration
    eprintln!("Capturing row counts (before)...");
    let row_counts_before = capture_row_counts(&old_conn).await?;

    // Apply migration - execute statements one at a time for detailed error reporting
    eprintln!("Applying migration...");
    if let Err(e) = execute_migration_statements(&old_conn, &migration_sql).await {
        eprintln!("Rolling back transaction...");
        let _ = old_conn.client().execute("ROLLBACK", &[]).await;
        bail!("{}", e);
    }
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
        &row_counts_before,
        &row_counts_after,
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

/// Execute migration statements one at a time with detailed error reporting
async fn execute_migration_statements(conn: &DbConnection, sql: &str) -> anyhow::Result<()> {
    // Split SQL into individual statements using pg_query
    let statements = pg_query::split_with_parser(sql)
        .map_err(|e| anyhow::anyhow!("Failed to parse migration SQL: {}", e))?;

    let total = statements.len();
    eprintln!("  Executing {} statement(s)...", total);

    for (idx, stmt_sql) in statements.iter().enumerate() {
        let stmt_num = idx + 1;
        let stmt_trimmed = stmt_sql.trim();

        // Skip empty statements
        if stmt_trimmed.is_empty() {
            continue;
        }

        // Find line number of this statement in the original SQL
        let line_num = find_line_number(sql, stmt_sql);

        // Execute the statement
        match conn.client().batch_execute(stmt_trimmed).await {
            Ok(_) => {
                // Show progress for long migrations
                if total > 10 && stmt_num % 10 == 0 {
                    eprintln!("  Progress: {}/{} statements", stmt_num, total);
                }
            }
            Err(e) => {
                // Build exhaustive error message
                let mut error_msg = String::new();
                error_msg.push_str("\n");
                error_msg.push_str("╔══════════════════════════════════════════════════════════════════════════════\n");
                error_msg.push_str("║ MIGRATION FAILED\n");
                error_msg.push_str("╠══════════════════════════════════════════════════════════════════════════════\n");
                error_msg.push_str(&format!("║ Statement: {} of {}\n", stmt_num, total));
                error_msg.push_str(&format!("║ Line: {}\n", line_num));
                error_msg.push_str("╠══════════════════════════════════════════════════════════════════════════════\n");
                error_msg.push_str("║ SQL:\n");
                error_msg.push_str("║ \n");

                // Show the SQL with line numbers
                for (i, line) in stmt_trimmed.lines().enumerate() {
                    let display_line = if line.len() > 72 {
                        format!("{}...", &line[..72])
                    } else {
                        line.to_string()
                    };
                    error_msg.push_str(&format!("║ {:4}: {}\n", line_num + i, display_line));
                }

                error_msg.push_str("║ \n");
                error_msg.push_str("╠══════════════════════════════════════════════════════════════════════════════\n");
                error_msg.push_str("║ PostgreSQL Error:\n");
                error_msg.push_str("║ \n");

                // Extract detailed error information from tokio-postgres error
                if let Some(db_error) = e.as_db_error() {
                    error_msg.push_str(&format!("║   Severity: {}\n", db_error.severity()));
                    error_msg.push_str(&format!("║   Code: {}\n", db_error.code().code()));
                    error_msg.push_str(&format!("║   Message: {}\n", db_error.message()));

                    if let Some(detail) = db_error.detail() {
                        error_msg.push_str(&format!("║   Detail: {}\n", detail));
                    }
                    if let Some(hint) = db_error.hint() {
                        error_msg.push_str(&format!("║   Hint: {}\n", hint));
                    }
                    if let Some(position) = db_error.position() {
                        error_msg.push_str(&format!("║   Position: {:?}\n", position));
                    }
                    if let Some(where_) = db_error.where_() {
                        error_msg.push_str(&format!("║   Where: {}\n", where_));
                    }
                    if let Some(schema) = db_error.schema() {
                        error_msg.push_str(&format!("║   Schema: {}\n", schema));
                    }
                    if let Some(table) = db_error.table() {
                        error_msg.push_str(&format!("║   Table: {}\n", table));
                    }
                    if let Some(column) = db_error.column() {
                        error_msg.push_str(&format!("║   Column: {}\n", column));
                    }
                    if let Some(constraint) = db_error.constraint() {
                        error_msg.push_str(&format!("║   Constraint: {}\n", constraint));
                    }
                    if let Some(datatype) = db_error.datatype() {
                        error_msg.push_str(&format!("║   Data Type: {}\n", datatype));
                    }
                } else {
                    error_msg.push_str(&format!("║   {}\n", e));
                }

                error_msg.push_str("║ \n");
                error_msg.push_str("╚══════════════════════════════════════════════════════════════════════════════\n");

                return Err(anyhow::anyhow!("{}", error_msg));
            }
        }
    }

    Ok(())
}

/// Find the line number where a statement starts in the original SQL
fn find_line_number(full_sql: &str, statement: &str) -> usize {
    // Try to find the statement in the original SQL
    let stmt_trimmed = statement.trim();
    if let Some(pos) = full_sql.find(stmt_trimmed) {
        // Count newlines before this position
        full_sql[..pos].matches('\n').count() + 1
    } else {
        // Fallback - try to find first non-comment line
        1
    }
}

/// Validate migration SQL for forbidden transaction control statements using pg_query parser
fn validate_migration_sql(sql: &str) -> anyhow::Result<()> {
    use pg_query::protobuf::TransactionStmtKind;

    // Parse SQL using PostgreSQL's actual parser
    let result = pg_query::parse(sql)
        .map_err(|e| anyhow::anyhow!("Failed to parse migration SQL: {}", e))?;

    // Walk through all nodes looking for transaction statements
    for node in result.protobuf.stmts.iter() {
        if let Some(ref stmt) = node.stmt {
            if let Some(ref node_enum) = stmt.node {
                if let pg_query::protobuf::node::Node::TransactionStmt(txn) = node_enum {
                    let kind = txn.kind();
                    match kind {
                        TransactionStmtKind::TransStmtCommit
                        | TransactionStmtKind::TransStmtCommitPrepared => {
                            bail!(
                                "Migration file contains COMMIT statement. \
                                Transaction control is managed by pgcmp test. \
                                Please remove COMMIT from your migration."
                            );
                        }
                        TransactionStmtKind::TransStmtRollback
                        | TransactionStmtKind::TransStmtRollbackPrepared
                        | TransactionStmtKind::TransStmtRollbackTo => {
                            bail!(
                                "Migration file contains ROLLBACK statement. \
                                Transaction control is managed by pgcmp test. \
                                Please remove ROLLBACK from your migration."
                            );
                        }
                        // BEGIN/START TRANSACTION would fail anyway since we're already in a transaction,
                        // but let's warn about it too
                        TransactionStmtKind::TransStmtBegin
                        | TransactionStmtKind::TransStmtStart => {
                            bail!(
                                "Migration file contains BEGIN/START TRANSACTION statement. \
                                Transaction control is managed by pgcmp test. \
                                Please remove BEGIN from your migration."
                            );
                        }
                        // Savepoints are fine
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

async fn get_postgres_version(conn: &DbConnection) -> anyhow::Result<String> {
    let row = conn
        .client()
        .query_one("SHOW server_version", &[])
        .await?;
    let version: String = row.get(0);
    let major = version.split('.').next().unwrap_or(&version);
    Ok(major.to_string())
}

async fn capture_row_counts(conn: &DbConnection) -> anyhow::Result<HashMap<String, i64>> {
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

// =============================================================================
// Analysis Types (same as diff.rs)
// =============================================================================

struct SummaryRow {
    object_type: &'static str,
    left_count: usize,
    right_count: usize,
}

impl SummaryRow {
    fn is_different(&self) -> bool {
        self.left_count != self.right_count
    }
}

struct ObjectAnalysis {
    name: String,
    action_description: String,
    modification_detail: Option<String>,
}

struct AnalysisResult {
    summary: Vec<SummaryRow>,
    schemas: Vec<ObjectAnalysis>,
    tables: Vec<ObjectAnalysis>,
    columns: Vec<ObjectAnalysis>,
    views: Vec<ObjectAnalysis>,
    materialized_views: Vec<ObjectAnalysis>,
    functions: Vec<ObjectAnalysis>,
    indexes: Vec<ObjectAnalysis>,
    constraints: Vec<ObjectAnalysis>,
    triggers: Vec<ObjectAnalysis>,
    sequences: Vec<ObjectAnalysis>,
}

impl AnalysisResult {
    fn has_differences(&self) -> bool {
        !self.schemas.is_empty()
            || !self.tables.is_empty()
            || !self.columns.is_empty()
            || !self.views.is_empty()
            || !self.materialized_views.is_empty()
            || !self.functions.is_empty()
            || !self.indexes.is_empty()
            || !self.constraints.is_empty()
            || !self.triggers.is_empty()
            || !self.sequences.is_empty()
    }

    fn count_differences(&self) -> usize {
        self.schemas.len()
            + self.tables.len()
            + self.columns.len()
            + self.views.len()
            + self.materialized_views.len()
            + self.functions.len()
            + self.indexes.len()
            + self.constraints.len()
            + self.triggers.len()
            + self.sequences.len()
    }
}

// =============================================================================
// Database Analysis (similar to diff.rs)
// =============================================================================

async fn analyze_databases(
    left_conn: &DbConnection,
    right_conn: &DbConnection,
) -> anyhow::Result<AnalysisResult> {
    let left_client = left_conn.client();
    let right_client = right_conn.client();

    // Fetch all object types from both databases in parallel
    let (left_tables, right_tables) =
        tokio::try_join!(fetch_tables(left_client), fetch_tables(right_client))?;

    let (left_views, right_views) =
        tokio::try_join!(fetch_views(left_client), fetch_views(right_client))?;

    let (left_matviews, right_matviews) = tokio::try_join!(
        fetch_materialized_views(left_client),
        fetch_materialized_views(right_client)
    )?;

    let (left_functions, right_functions) =
        tokio::try_join!(fetch_functions(left_client), fetch_functions(right_client))?;

    let (left_indexes, right_indexes) =
        tokio::try_join!(fetch_indexes(left_client), fetch_indexes(right_client))?;

    let (left_constraints, right_constraints) =
        tokio::try_join!(fetch_constraints(left_client), fetch_constraints(right_client))?;

    let (left_triggers, right_triggers) =
        tokio::try_join!(fetch_triggers(left_client), fetch_triggers(right_client))?;

    let (left_sequences, right_sequences) =
        tokio::try_join!(fetch_sequences(left_client), fetch_sequences(right_client))?;

    // Extract schemas from tables
    let left_schemas: HashSet<&str> = left_tables.iter().map(|t| t.schema_name.as_str()).collect();
    let right_schemas: HashSet<&str> = right_tables.iter().map(|t| t.schema_name.as_str()).collect();

    // Build summary
    let summary = vec![
        SummaryRow {
            object_type: "Schemas",
            left_count: left_schemas.len(),
            right_count: right_schemas.len(),
        },
        SummaryRow {
            object_type: "Tables",
            left_count: left_tables.len(),
            right_count: right_tables.len(),
        },
        SummaryRow {
            object_type: "Columns",
            left_count: left_tables.iter().map(|t| t.columns.len()).sum(),
            right_count: right_tables.iter().map(|t| t.columns.len()).sum(),
        },
        SummaryRow {
            object_type: "Views",
            left_count: left_views.len(),
            right_count: right_views.len(),
        },
        SummaryRow {
            object_type: "Materialized Views",
            left_count: left_matviews.len(),
            right_count: right_matviews.len(),
        },
        SummaryRow {
            object_type: "Functions",
            left_count: left_functions.len(),
            right_count: right_functions.len(),
        },
        SummaryRow {
            object_type: "Indexes",
            left_count: left_indexes.len(),
            right_count: right_indexes.len(),
        },
        SummaryRow {
            object_type: "Constraints",
            left_count: left_constraints.len(),
            right_count: right_constraints.len(),
        },
        SummaryRow {
            object_type: "Triggers",
            left_count: left_triggers.len(),
            right_count: right_triggers.len(),
        },
        SummaryRow {
            object_type: "Sequences",
            left_count: left_sequences.len(),
            right_count: right_sequences.len(),
        },
    ];

    // Analyze all object types
    let schemas = analyze_schemas(&left_schemas, &right_schemas);
    let (tables, columns) = analyze_tables(&left_tables, &right_tables);
    let views = analyze_views(&left_views, &right_views, false);
    let materialized_views = analyze_views(&left_matviews, &right_matviews, true);
    let functions = analyze_functions(&left_functions, &right_functions);
    let indexes = analyze_indexes(&left_indexes, &right_indexes);
    let constraints = analyze_constraints(&left_constraints, &right_constraints);
    let triggers = analyze_triggers(&left_triggers, &right_triggers);
    let sequences = analyze_sequences(&left_sequences, &right_sequences);

    Ok(AnalysisResult {
        summary,
        schemas,
        tables,
        columns,
        views,
        materialized_views,
        functions,
        indexes,
        constraints,
        triggers,
        sequences,
    })
}

fn analyze_schemas(left: &HashSet<&str>, right: &HashSet<&str>) -> Vec<ObjectAnalysis> {
    let mut result = Vec::new();

    for schema in left.difference(right) {
        result.push(ObjectAnalysis {
            name: schema.to_string(),
            action_description: format!("create schema {}", schema),
            modification_detail: None,
        });
    }

    for schema in right.difference(left) {
        result.push(ObjectAnalysis {
            name: schema.to_string(),
            action_description: format!("drop schema {}", schema),
            modification_detail: None,
        });
    }

    result
}

fn analyze_tables(
    left: &[crate::db::tables::TableInfo],
    right: &[crate::db::tables::TableInfo],
) -> (Vec<ObjectAnalysis>, Vec<ObjectAnalysis>) {
    use crate::db::tables::TableInfo;

    let left_map: HashMap<(&str, &str), &TableInfo> = left
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.table_name.as_str()), t))
        .collect();

    let right_map: HashMap<(&str, &str), &TableInfo> = right
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.table_name.as_str()), t))
        .collect();

    let mut tables = Vec::new();
    let mut columns = Vec::new();

    for table in left {
        let key = (table.schema_name.as_str(), table.table_name.as_str());
        let full_name = format!("{}.{}", table.schema_name, table.table_name);

        if let Some(right_table) = right_map.get(&key) {
            let col_diffs = analyze_columns(table, right_table);
            columns.extend(col_diffs);
        } else {
            tables.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create table {}.{}", table.schema_name, table.table_name),
                modification_detail: None,
            });
        }
    }

    for table in right {
        let key = (table.schema_name.as_str(), table.table_name.as_str());
        if !left_map.contains_key(&key) {
            tables.push(ObjectAnalysis {
                name: format!("{}.{}", table.schema_name, table.table_name),
                action_description: format!("drop table {}.{}", table.schema_name, table.table_name),
                modification_detail: None,
            });
        }
    }

    (tables, columns)
}

fn analyze_columns(
    left_table: &crate::db::tables::TableInfo,
    right_table: &crate::db::tables::TableInfo,
) -> Vec<ObjectAnalysis> {
    use crate::db::tables::ColumnInfo;

    let left_map: HashMap<&str, &ColumnInfo> = left_table
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let right_map: HashMap<&str, &ColumnInfo> = right_table
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let mut result = Vec::new();

    for col in &left_table.columns {
        let full_name = format!("{}.{}.{}", left_table.schema_name, left_table.table_name, col.name);

        if let Some(right_col) = right_map.get(col.name.as_str()) {
            let mods = get_column_modifications(col, right_col);
            if !mods.is_empty() {
                let detail = format!(
                    "alter column {}\n  old: {}\n  new: {}",
                    full_name,
                    format_column_spec(right_col),
                    format_column_spec(col)
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "alter column {}.{}.{}",
                        left_table.schema_name, left_table.table_name, col.name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "add column {}.{}.{}",
                    left_table.schema_name, left_table.table_name, col.name
                ),
                modification_detail: None,
            });
        }
    }

    for col in &right_table.columns {
        if !left_map.contains_key(col.name.as_str()) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}.{}", right_table.schema_name, right_table.table_name, col.name),
                action_description: format!(
                    "drop column {}.{}.{}",
                    right_table.schema_name, right_table.table_name, col.name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn get_column_modifications(
    left: &crate::db::tables::ColumnInfo,
    right: &crate::db::tables::ColumnInfo,
) -> Vec<String> {
    let mut mods = Vec::new();

    if left.data_type != right.data_type {
        mods.push(format!("type: {} -> {}", right.data_type, left.data_type));
    }
    if left.is_nullable != right.is_nullable {
        let old_null = if right.is_nullable { "null" } else { "not null" };
        let new_null = if left.is_nullable { "null" } else { "not null" };
        mods.push(format!("nullable: {} -> {}", old_null, new_null));
    }
    if left.column_default != right.column_default {
        mods.push(format!(
            "default: {:?} -> {:?}",
            right.column_default, left.column_default
        ));
    }

    mods
}

fn format_column_spec(col: &crate::db::tables::ColumnInfo) -> String {
    let mut parts = vec![col.data_type.clone()];
    parts.push(if col.is_nullable { "null" } else { "not null" }.to_string());
    if let Some(ref default) = col.column_default {
        parts.push(format!("default {}", default));
    }
    parts.join(" ")
}

fn analyze_views(
    left: &[crate::db::views::ViewInfo],
    right: &[crate::db::views::ViewInfo],
    is_materialized: bool,
) -> Vec<ObjectAnalysis> {
    use crate::db::views::ViewInfo;

    let type_name = if is_materialized { "materialized view" } else { "view" };

    let left_map: HashMap<(&str, &str), &ViewInfo> = left
        .iter()
        .map(|v| ((v.schema_name.as_str(), v.view_name.as_str()), v))
        .collect();

    let right_map: HashMap<(&str, &str), &ViewInfo> = right
        .iter()
        .map(|v| ((v.schema_name.as_str(), v.view_name.as_str()), v))
        .collect();

    let mut result = Vec::new();

    for view in left {
        let key = (view.schema_name.as_str(), view.view_name.as_str());
        let full_name = format!("{}.{}", view.schema_name, view.view_name);

        if let Some(right_view) = right_map.get(&key) {
            if view.definition.trim() != right_view.definition.trim() {
                let detail = format!(
                    "replace {} {}\n  old: {}\n  new: {}",
                    type_name,
                    full_name,
                    truncate(&right_view.definition, 100),
                    truncate(&view.definition, 100)
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!("replace {} {}.{}", type_name, view.schema_name, view.view_name),
                    modification_detail: Some(detail),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create {} {}.{}", type_name, view.schema_name, view.view_name),
                modification_detail: None,
            });
        }
    }

    for view in right {
        let key = (view.schema_name.as_str(), view.view_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", view.schema_name, view.view_name),
                action_description: format!("drop {} {}.{}", type_name, view.schema_name, view.view_name),
                modification_detail: None,
            });
        }
    }

    result
}

fn analyze_functions(
    left: &[crate::db::functions::FunctionInfo],
    right: &[crate::db::functions::FunctionInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::functions::FunctionInfo;

    let left_map: HashMap<(&str, &str), &FunctionInfo> = left
        .iter()
        .map(|f| ((f.schema_name.as_str(), f.function_name.as_str()), f))
        .collect();

    let right_map: HashMap<(&str, &str), &FunctionInfo> = right
        .iter()
        .map(|f| ((f.schema_name.as_str(), f.function_name.as_str()), f))
        .collect();

    let mut result = Vec::new();

    for func in left {
        let key = (func.schema_name.as_str(), func.function_name.as_str());
        let full_name = format!("{}.{}", func.schema_name, func.function_name);

        if let Some(right_func) = right_map.get(&key) {
            let left_def = normalize_function_def(&func.definition);
            let right_def = normalize_function_def(&right_func.definition);
            if left_def != right_def {
                result.push(ObjectAnalysis {
                    name: full_name.clone(),
                    action_description: format!("replace function {}.{}", func.schema_name, func.function_name),
                    modification_detail: Some(format!("replace function {}\n  definition changed", full_name)),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create function {}.{}", func.schema_name, func.function_name),
                modification_detail: None,
            });
        }
    }

    for func in right {
        let key = (func.schema_name.as_str(), func.function_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", func.schema_name, func.function_name),
                action_description: format!("drop function {}.{}", func.schema_name, func.function_name),
                modification_detail: None,
            });
        }
    }

    result
}

fn normalize_function_def(def: &str) -> String {
    def.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn analyze_indexes(
    left: &[crate::db::indexes::IndexInfo],
    right: &[crate::db::indexes::IndexInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::indexes::IndexInfo;

    let left_map: HashMap<(&str, &str), &IndexInfo> = left
        .iter()
        .map(|i| ((i.schema_name.as_str(), i.index_name.as_str()), i))
        .collect();

    let right_map: HashMap<(&str, &str), &IndexInfo> = right
        .iter()
        .map(|i| ((i.schema_name.as_str(), i.index_name.as_str()), i))
        .collect();

    let mut result = Vec::new();

    for idx in left {
        let key = (idx.schema_name.as_str(), idx.index_name.as_str());
        let full_name = format!("{}.{}", idx.schema_name, idx.index_name);

        if let Some(right_idx) = right_map.get(&key) {
            if idx.definition != right_idx.definition {
                let detail = format!(
                    "recreate index {}\n  old: {}\n  new: {}",
                    full_name, right_idx.definition, idx.definition
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!("recreate index {}.{}", idx.schema_name, idx.index_name),
                    modification_detail: Some(detail),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create index {}.{}", idx.schema_name, idx.index_name),
                modification_detail: None,
            });
        }
    }

    for idx in right {
        let key = (idx.schema_name.as_str(), idx.index_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", idx.schema_name, idx.index_name),
                action_description: format!("drop index {}.{}", idx.schema_name, idx.index_name),
                modification_detail: None,
            });
        }
    }

    result
}

fn analyze_constraints(
    left: &[crate::db::constraints::ConstraintInfo],
    right: &[crate::db::constraints::ConstraintInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::constraints::ConstraintInfo;

    let left_map: HashMap<(&str, &str, &str), &ConstraintInfo> = left
        .iter()
        .map(|c| ((c.schema_name.as_str(), c.table_name.as_str(), c.constraint_name.as_str()), c))
        .collect();

    let right_map: HashMap<(&str, &str, &str), &ConstraintInfo> = right
        .iter()
        .map(|c| ((c.schema_name.as_str(), c.table_name.as_str(), c.constraint_name.as_str()), c))
        .collect();

    let mut result = Vec::new();

    for con in left {
        let key = (con.schema_name.as_str(), con.table_name.as_str(), con.constraint_name.as_str());
        let full_name = format!("{}.{}.{}", con.schema_name, con.table_name, con.constraint_name);

        if let Some(right_con) = right_map.get(&key) {
            let left_def = normalize_constraint_definition(&con.definition);
            let right_def = normalize_constraint_definition(&right_con.definition);
            if left_def != right_def {
                let detail = format!(
                    "replace constraint {}\n  old: {}\n  new: {}",
                    full_name, right_con.definition, con.definition
                );
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!(
                        "replace constraint {}.{}.{}",
                        con.schema_name, con.table_name, con.constraint_name
                    ),
                    modification_detail: Some(detail),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "add constraint {}.{}.{}",
                    con.schema_name, con.table_name, con.constraint_name
                ),
                modification_detail: None,
            });
        }
    }

    for con in right {
        let key = (con.schema_name.as_str(), con.table_name.as_str(), con.constraint_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}.{}", con.schema_name, con.table_name, con.constraint_name),
                action_description: format!(
                    "drop constraint {}.{}.{}",
                    con.schema_name, con.table_name, con.constraint_name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn normalize_constraint_definition(def: &str) -> String {
    let mut result = def.to_string();
    result = result.replace("::character varying[]", "");
    result = result.replace("::character varying", "");
    result = result.replace("::text[]", "");
    result = result.replace("::text", "");

    let mut normalized = String::with_capacity(result.len());
    let mut last_was_space = true;
    for c in result.chars() {
        if c.is_alphanumeric() {
            normalized.push(c);
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }

    normalized.trim().to_string()
}

fn analyze_triggers(
    left: &[crate::db::triggers::TriggerInfo],
    right: &[crate::db::triggers::TriggerInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::triggers::TriggerInfo;

    let left_map: HashMap<(&str, &str), &TriggerInfo> = left
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.trigger_name.as_str()), t))
        .collect();

    let right_map: HashMap<(&str, &str), &TriggerInfo> = right
        .iter()
        .map(|t| ((t.schema_name.as_str(), t.trigger_name.as_str()), t))
        .collect();

    let mut result = Vec::new();

    for trig in left {
        let key = (trig.schema_name.as_str(), trig.trigger_name.as_str());
        let full_name = format!("{}.{}.{}", trig.schema_name, trig.table_name, trig.trigger_name);

        if let Some(right_trig) = right_map.get(&key) {
            let left_def = normalize_trigger_def(&trig.definition);
            let right_def = normalize_trigger_def(&right_trig.definition);
            if left_def != right_def {
                result.push(ObjectAnalysis {
                    name: full_name.clone(),
                    action_description: format!(
                        "replace trigger {}.{}.{}",
                        trig.schema_name, trig.table_name, trig.trigger_name
                    ),
                    modification_detail: Some(format!("replace trigger {}\n  definition changed", full_name)),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!(
                    "create trigger {}.{}.{}",
                    trig.schema_name, trig.table_name, trig.trigger_name
                ),
                modification_detail: None,
            });
        }
    }

    for trig in right {
        let key = (trig.schema_name.as_str(), trig.trigger_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}.{}", trig.schema_name, trig.table_name, trig.trigger_name),
                action_description: format!(
                    "drop trigger {}.{}.{}",
                    trig.schema_name, trig.table_name, trig.trigger_name
                ),
                modification_detail: None,
            });
        }
    }

    result
}

fn normalize_trigger_def(def: &str) -> String {
    def.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

fn analyze_sequences(
    left: &[crate::db::sequences::SequenceInfo],
    right: &[crate::db::sequences::SequenceInfo],
) -> Vec<ObjectAnalysis> {
    use crate::db::sequences::SequenceInfo;

    let left_map: HashMap<(&str, &str), &SequenceInfo> = left
        .iter()
        .map(|s| ((s.schema_name.as_str(), s.sequence_name.as_str()), s))
        .collect();

    let right_map: HashMap<(&str, &str), &SequenceInfo> = right
        .iter()
        .map(|s| ((s.schema_name.as_str(), s.sequence_name.as_str()), s))
        .collect();

    let mut result = Vec::new();

    for seq in left {
        let key = (seq.schema_name.as_str(), seq.sequence_name.as_str());
        let full_name = format!("{}.{}", seq.schema_name, seq.sequence_name);

        if let Some(right_seq) = right_map.get(&key) {
            let mods = get_sequence_modifications(seq, right_seq);
            if !mods.is_empty() {
                let detail = format!("alter sequence {}\n  {}", full_name, mods.join("\n  "));
                result.push(ObjectAnalysis {
                    name: full_name,
                    action_description: format!("alter sequence {}.{}", seq.schema_name, seq.sequence_name),
                    modification_detail: Some(detail),
                });
            }
        } else {
            result.push(ObjectAnalysis {
                name: full_name,
                action_description: format!("create sequence {}.{}", seq.schema_name, seq.sequence_name),
                modification_detail: None,
            });
        }
    }

    for seq in right {
        let key = (seq.schema_name.as_str(), seq.sequence_name.as_str());
        if !left_map.contains_key(&key) {
            result.push(ObjectAnalysis {
                name: format!("{}.{}", seq.schema_name, seq.sequence_name),
                action_description: format!("drop sequence {}.{}", seq.schema_name, seq.sequence_name),
                modification_detail: None,
            });
        }
    }

    result
}

fn get_sequence_modifications(
    left: &crate::db::sequences::SequenceInfo,
    right: &crate::db::sequences::SequenceInfo,
) -> Vec<String> {
    let mut mods = Vec::new();

    if left.data_type != right.data_type {
        mods.push(format!("type: {} -> {}", right.data_type, left.data_type));
    }
    if left.start_value != right.start_value {
        mods.push(format!("start: {} -> {}", right.start_value, left.start_value));
    }
    if left.increment_by != right.increment_by {
        mods.push(format!("increment: {} -> {}", right.increment_by, left.increment_by));
    }
    if left.min_value != right.min_value {
        mods.push(format!("min: {} -> {}", right.min_value, left.min_value));
    }
    if left.max_value != right.max_value {
        mods.push(format!("max: {} -> {}", right.max_value, left.max_value));
    }
    if left.cycle != right.cycle {
        mods.push(format!("cycle: {} -> {}", right.cycle, left.cycle));
    }

    mods
}

fn truncate(s: &str, max_len: usize) -> String {
    let s = s.replace('\n', " ").replace("  ", " ");
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len])
    }
}

// =============================================================================
// XML Generation
// =============================================================================

fn generate_xml(
    analysis: &AnalysisResult,
    left_conn_str: &str,
    right_conn_str: &str,
    left_version: &str,
    right_version: &str,
    row_counts_before: &HashMap<String, i64>,
    row_counts_after: &HashMap<String, i64>,
) -> String {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<database_comparison>\n");

    // Connections section
    xml.push_str("  <connections>\n");
    xml.push_str(&format!(
        "    <left postgres_version=\"{}\">{}</left>\n",
        xml_escape(left_version),
        xml_escape(left_conn_str)
    ));
    xml.push_str(&format!(
        "    <right postgres_version=\"{}\">{}</right>\n",
        xml_escape(right_version),
        xml_escape(right_conn_str)
    ));
    xml.push_str("  </connections>\n");

    // Version warning if versions differ
    if left_version != right_version {
        xml.push_str(&format!(
            "  <version_warning>PostgreSQL versions differ (left: {}, right: {}). Some differences may be due to version-specific output formatting.</version_warning>\n",
            xml_escape(left_version),
            xml_escape(right_version)
        ));
    }

    // Summary section
    xml.push_str("  <summary>\n");
    for row in &analysis.summary {
        xml.push_str("    <item>\n");
        xml.push_str(&format!("      <type>{}</type>\n", row.object_type));
        xml.push_str(&format!("      <left_count>{}</left_count>\n", row.left_count));
        xml.push_str(&format!("      <right_count>{}</right_count>\n", row.right_count));
        xml.push_str(&format!(
            "      <different>{}</different>\n",
            row.is_different().to_string().to_lowercase()
        ));
        xml.push_str("    </item>\n");
    }
    xml.push_str("  </summary>\n");

    // Object sections
    write_object_section(&mut xml, "schemas", "schema", &analysis.schemas);
    write_object_section(&mut xml, "tables", "table", &analysis.tables);
    write_object_section(&mut xml, "columns", "column", &analysis.columns);
    write_object_section(&mut xml, "views", "view", &analysis.views);
    write_object_section(&mut xml, "materialized_views", "materialized_view", &analysis.materialized_views);
    write_object_section(&mut xml, "functions", "function", &analysis.functions);
    write_object_section(&mut xml, "indexes", "index", &analysis.indexes);
    write_object_section(&mut xml, "constraints", "constraint", &analysis.constraints);
    write_object_section(&mut xml, "triggers", "trigger", &analysis.triggers);
    write_object_section(&mut xml, "sequences", "sequence", &analysis.sequences);

    // Row counts section
    xml.push_str("  <row_counts>\n");

    let mut all_tables: Vec<&String> = row_counts_before.keys().chain(row_counts_after.keys()).collect();
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

    // Total differences count
    xml.push_str(&format!(
        "  <number_of_differences>{}</number_of_differences>\n",
        analysis.count_differences()
    ));

    xml.push_str("</database_comparison>\n");

    xml
}

fn write_object_section(xml: &mut String, section_name: &str, item_name: &str, objects: &[ObjectAnalysis]) {
    xml.push_str(&format!("  <{}>\n", section_name));
    for obj in objects {
        xml.push_str(&format!("    <{}>\n", item_name));
        xml.push_str(&format!("      <name>{}</name>\n", xml_escape(&obj.name)));
        xml.push_str(&format!("      <action>{}</action>\n", xml_escape(&obj.action_description)));
        if let Some(ref detail) = obj.modification_detail {
            xml.push_str(&format!("      <detail>{}</detail>\n", xml_escape(detail)));
        }
        xml.push_str(&format!("    </{}>\n", item_name));
    }
    xml.push_str(&format!("  </{}>\n", section_name));
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
