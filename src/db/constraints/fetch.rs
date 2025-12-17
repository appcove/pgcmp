use tokio_postgres::Client;

/// Metadata for a constraint
#[derive(Debug)]
pub struct ConstraintInfo {
    pub schema_name: String,
    pub table_name: String,
    pub constraint_name: String,
    pub constraint_type: String,
    pub definition: String,
}

/// Fetch all user constraints from the database
pub async fn fetch_constraints(client: &Client) -> anyhow::Result<Vec<ConstraintInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                c.relname AS table_name,
                con.conname AS constraint_name,
                con.contype AS constraint_type,
                pg_get_constraintdef(con.oid, true) AS definition
            FROM pg_constraint con
            JOIN pg_class c ON c.oid = con.conrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            ORDER BY n.nspname, c.relname, con.conname
            "#,
            &[],
        )
        .await?;

    let mut constraints = Vec::new();

    for row in rows {
        let contype: i8 = row.get("constraint_type");
        let constraint_type = match contype as u8 as char {
            'c' => "CHECK",
            'f' => "FOREIGN KEY",
            'p' => "PRIMARY KEY",
            'u' => "UNIQUE",
            'x' => "EXCLUDE",
            't' => "TRIGGER",
            _ => "UNKNOWN",
        };

        constraints.push(ConstraintInfo {
            schema_name: row.get("schema_name"),
            table_name: row.get("table_name"),
            constraint_name: row.get("constraint_name"),
            constraint_type: constraint_type.to_string(),
            definition: row.get("definition"),
        });
    }

    Ok(constraints)
}
