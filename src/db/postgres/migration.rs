use super::DbConnection;
use anyhow::{bail, Context};
use pg_query::protobuf::TransactionStmtKind;
use std::fs;
use std::path::Path;

/// Result of migration execution
pub struct MigrationResult {
    /// Whether the migration was committed (true) or rolled back (false)
    pub committed: bool,
}

/// Load and validate a migration file.
/// Returns the SQL content if valid.
pub fn load_and_validate_migration(path: &Path) -> anyhow::Result<String> {
    let migration_sql = fs::read_to_string(path)
        .with_context(|| format!("Failed to read migration file: {}", path.display()))?;

    if migration_sql.trim().is_empty() {
        bail!("Migration file is empty: {}", path.display());
    }

    validate_migration_structure(&migration_sql)?;

    Ok(migration_sql)
}

/// Validate that migration SQL has proper transaction structure:
/// - Must start with BEGIN TRANSACTION (or BEGIN)
/// - Must end with ROLLBACK
fn validate_migration_structure(sql: &str) -> anyhow::Result<()> {
    // Parse SQL using PostgreSQL's actual parser
    let result = pg_query::parse(sql)
        .map_err(|e| anyhow::anyhow!("Failed to parse migration SQL: {}", e))?;

    let stmts = &result.protobuf.stmts;

    if stmts.is_empty() {
        bail!("Migration file contains no SQL statements");
    }

    // Check first statement is BEGIN
    let first_stmt = &stmts[0];
    if let Some(ref stmt) = first_stmt.stmt {
        if let Some(ref node_enum) = stmt.node {
            match node_enum {
                pg_query::protobuf::node::Node::TransactionStmt(txn) => {
                    let kind = txn.kind();
                    if !matches!(
                        kind,
                        TransactionStmtKind::TransStmtBegin | TransactionStmtKind::TransStmtStart
                    ) {
                        bail!(
                            "Migration file must start with BEGIN TRANSACTION.\n\
                            First statement found: {:?}\n\n\
                            Migration files must have this structure:\n\
                            \n\
                            BEGIN TRANSACTION;\n\
                            -- your migration SQL here\n\
                            ROLLBACK;",
                            kind
                        );
                    }
                }
                _ => {
                    bail!(
                        "Migration file must start with BEGIN TRANSACTION.\n\n\
                        Migration files must have this structure:\n\
                        \n\
                        BEGIN TRANSACTION;\n\
                        -- your migration SQL here\n\
                        ROLLBACK;"
                    );
                }
            }
        } else {
            bail!("Invalid first statement in migration file");
        }
    } else {
        bail!("Invalid first statement in migration file");
    }

    // Check last statement is ROLLBACK
    let last_stmt = &stmts[stmts.len() - 1];
    if let Some(ref stmt) = last_stmt.stmt {
        if let Some(ref node_enum) = stmt.node {
            match node_enum {
                pg_query::protobuf::node::Node::TransactionStmt(txn) => {
                    let kind = txn.kind();
                    if !matches!(kind, TransactionStmtKind::TransStmtRollback) {
                        bail!(
                            "Migration file must end with ROLLBACK.\n\
                            Last statement found: {:?}\n\n\
                            Migration files must have this structure:\n\
                            \n\
                            BEGIN TRANSACTION;\n\
                            -- your migration SQL here\n\
                            ROLLBACK;",
                            kind
                        );
                    }
                }
                _ => {
                    bail!(
                        "Migration file must end with ROLLBACK.\n\n\
                        Migration files must have this structure:\n\
                        \n\
                        BEGIN TRANSACTION;\n\
                        -- your migration SQL here\n\
                        ROLLBACK;"
                    );
                }
            }
        } else {
            bail!("Invalid last statement in migration file");
        }
    } else {
        bail!("Invalid last statement in migration file");
    }

    // Check for COMMIT statements in the middle (not allowed)
    for (idx, node) in stmts.iter().enumerate() {
        // Skip first and last
        if idx == 0 || idx == stmts.len() - 1 {
            continue;
        }

        if let Some(ref stmt) = node.stmt {
            if let Some(ref node_enum) = stmt.node {
                if let pg_query::protobuf::node::Node::TransactionStmt(txn) = node_enum {
                    let kind = txn.kind();
                    match kind {
                        TransactionStmtKind::TransStmtCommit
                        | TransactionStmtKind::TransStmtCommitPrepared => {
                            bail!(
                                "Migration file contains COMMIT statement in the middle.\n\
                                Transaction control should only be at the start (BEGIN) and end (ROLLBACK).\n\
                                Please remove the COMMIT statement."
                            );
                        }
                        TransactionStmtKind::TransStmtRollback
                        | TransactionStmtKind::TransStmtRollbackPrepared => {
                            bail!(
                                "Migration file contains ROLLBACK statement in the middle.\n\
                                Transaction control should only be at the start (BEGIN) and end (ROLLBACK).\n\
                                Please remove the extra ROLLBACK statement."
                            );
                        }
                        TransactionStmtKind::TransStmtBegin | TransactionStmtKind::TransStmtStart => {
                            bail!(
                                "Migration file contains BEGIN statement in the middle.\n\
                                Transaction control should only be at the start (BEGIN) and end (ROLLBACK).\n\
                                Please remove the extra BEGIN statement."
                            );
                        }
                        // Savepoints and rollback to savepoint are fine
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

/// Execute migration statements one at a time with detailed error reporting.
/// If `commit` is true, replaces the final ROLLBACK with COMMIT.
/// Returns whether the transaction was committed or rolled back.
pub async fn execute_migration(
    conn: &DbConnection,
    sql: &str,
    commit: bool,
) -> anyhow::Result<MigrationResult> {
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

        // Check if this is the last statement (ROLLBACK)
        let is_last = idx == total - 1;

        // Determine what to actually execute
        let actual_sql = if is_last && commit {
            // Replace ROLLBACK with COMMIT
            "COMMIT"
        } else {
            stmt_trimmed
        };

        // Find line number of this statement in the original SQL
        let line_num = find_line_number(sql, stmt_sql);

        // Execute the statement
        match conn.client().batch_execute(actual_sql).await {
            Ok(_) => {
                // Show progress for long migrations
                if total > 10 && stmt_num % 10 == 0 {
                    eprintln!("  Progress: {}/{} statements", stmt_num, total);
                }
            }
            Err(e) => {
                // Try to rollback before returning error
                eprintln!("Rolling back transaction due to error...");
                let _ = conn.client().execute("ROLLBACK", &[]).await;

                // Build exhaustive error message
                let error_msg = build_error_message(stmt_num, total, line_num, stmt_trimmed, &e);
                return Err(anyhow::anyhow!("{}", error_msg));
            }
        }
    }

    Ok(MigrationResult { committed: commit })
}

/// Execute migration statements for testing (schema comparison).
/// Executes all statements EXCEPT the final ROLLBACK, leaving the transaction open.
/// This allows the test command to compare schemas before deciding to rollback.
pub async fn execute_migration_for_test(conn: &DbConnection, sql: &str) -> anyhow::Result<()> {
    // Split SQL into individual statements using pg_query
    let statements = pg_query::split_with_parser(sql)
        .map_err(|e| anyhow::anyhow!("Failed to parse migration SQL: {}", e))?;

    let total = statements.len();
    // We'll execute all but the last statement (ROLLBACK)
    let execute_count = total - 1;
    eprintln!("  Executing {} statement(s)...", execute_count);

    for (idx, stmt_sql) in statements.iter().enumerate() {
        let stmt_num = idx + 1;
        let stmt_trimmed = stmt_sql.trim();

        // Skip empty statements
        if stmt_trimmed.is_empty() {
            continue;
        }

        // Skip the last statement (ROLLBACK) - we'll handle it later
        if idx == total - 1 {
            continue;
        }

        // Find line number of this statement in the original SQL
        let line_num = find_line_number(sql, stmt_sql);

        // Execute the statement
        match conn.client().batch_execute(stmt_trimmed).await {
            Ok(_) => {
                // Show progress for long migrations
                if execute_count > 10 && stmt_num % 10 == 0 {
                    eprintln!("  Progress: {}/{} statements", stmt_num, execute_count);
                }
            }
            Err(e) => {
                // Try to rollback before returning error
                eprintln!("Rolling back transaction due to error...");
                let _ = conn.client().execute("ROLLBACK", &[]).await;

                // Build exhaustive error message
                let error_msg = build_error_message(stmt_num, total, line_num, stmt_trimmed, &e);
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

/// Build a detailed error message for a failed statement
fn build_error_message(
    stmt_num: usize,
    total: usize,
    line_num: usize,
    stmt_trimmed: &str,
    e: &tokio_postgres::Error,
) -> String {
    let mut error_msg = String::new();
    error_msg.push_str("\n");
    error_msg.push_str(
        "╔══════════════════════════════════════════════════════════════════════════════\n",
    );
    error_msg.push_str("║ MIGRATION FAILED\n");
    error_msg.push_str(
        "╠══════════════════════════════════════════════════════════════════════════════\n",
    );
    error_msg.push_str(&format!("║ Statement: {} of {}\n", stmt_num, total));
    error_msg.push_str(&format!("║ Line: {}\n", line_num));
    error_msg.push_str(
        "╠══════════════════════════════════════════════════════════════════════════════\n",
    );
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
    error_msg.push_str(
        "╠══════════════════════════════════════════════════════════════════════════════\n",
    );
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
    error_msg.push_str(
        "╚══════════════════════════════════════════════════════════════════════════════\n",
    );

    error_msg
}

/// Print a big warning banner that migration was NOT applied
pub fn print_rollback_banner() {
    eprintln!();
    eprintln!("╔══════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                                                                              ║");
    eprintln!("║   ██████╗  ██████╗ ██╗     ██╗     ███████╗██████╗     ██████╗  █████╗  ██████╗██╗  ██╗   ║");
    eprintln!("║   ██╔══██╗██╔═══██╗██║     ██║     ██╔════╝██╔══██╗    ██╔══██╗██╔══██╗██╔════╝██║ ██╔╝   ║");
    eprintln!("║   ██████╔╝██║   ██║██║     ██║     █████╗  ██║  ██║    ██████╔╝███████║██║     █████╔╝    ║");
    eprintln!("║   ██╔══██╗██║   ██║██║     ██║     ██╔══╝  ██║  ██║    ██╔══██╗██╔══██║██║     ██╔═██╗    ║");
    eprintln!("║   ██║  ██║╚██████╔╝███████╗███████╗███████╗██████╔╝    ██████╔╝██║  ██║╚██████╗██║  ██╗   ║");
    eprintln!("║   ╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚══════╝╚══════╝╚═════╝     ╚═════╝ ╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝   ║");
    eprintln!("║                                                                              ║");
    eprintln!("║                    MIGRATION WAS NOT APPLIED TO DATABASE                     ║");
    eprintln!("║                                                                              ║");
    eprintln!("║          The migration executed successfully but was ROLLED BACK.            ║");
    eprintln!("║                                                                              ║");
    eprintln!("║          To actually apply the migration, run:                               ║");
    eprintln!("║                                                                              ║");
    eprintln!("║                      pgcmp apply --commit                                    ║");
    eprintln!("║                                                                              ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════════════════╝");
    eprintln!();
}

/// Print a success banner that migration was committed
pub fn print_commit_banner() {
    eprintln!();
    eprintln!("╔══════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                                                                              ║");
    eprintln!("║    ██████╗ ██████╗ ███╗   ███╗███╗   ███╗██╗████████╗████████╗███████╗██████╗    ║");
    eprintln!("║   ██╔════╝██╔═══██╗████╗ ████║████╗ ████║██║╚══██╔══╝╚══██╔══╝██╔════╝██╔══██╗   ║");
    eprintln!("║   ██║     ██║   ██║██╔████╔██║██╔████╔██║██║   ██║      ██║   █████╗  ██║  ██║   ║");
    eprintln!("║   ██║     ██║   ██║██║╚██╔╝██║██║╚██╔╝██║██║   ██║      ██║   ██╔══╝  ██║  ██║   ║");
    eprintln!("║   ╚██████╗╚██████╔╝██║ ╚═╝ ██║██║ ╚═╝ ██║██║   ██║      ██║   ███████╗██████╔╝   ║");
    eprintln!("║    ╚═════╝ ╚═════╝ ╚═╝     ╚═╝╚═╝     ╚═╝╚═╝   ╚═╝      ╚═╝   ╚══════╝╚═════╝    ║");
    eprintln!("║                                                                              ║");
    eprintln!("║                   MIGRATION SUCCESSFULLY APPLIED TO DATABASE                 ║");
    eprintln!("║                                                                              ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════════════════╝");
    eprintln!();
}
