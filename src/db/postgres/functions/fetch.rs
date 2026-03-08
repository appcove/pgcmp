use tokio_postgres::Client;

/// Metadata for a function
#[derive(Debug)]
pub struct FunctionInfo {
    pub schema_name: String,
    pub function_name: String,
    pub definition: String,
}

/// Fetch all user functions from the database
pub async fn fetch_functions(client: &Client) -> anyhow::Result<Vec<FunctionInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                p.proname AS function_name,
                pg_get_functiondef(p.oid) AS definition
            FROM pg_proc p
            JOIN pg_namespace n ON n.oid = p.pronamespace
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND p.prokind IN ('f', 'p')  -- functions and procedures
            ORDER BY n.nspname, p.proname
            "#,
            &[],
        )
        .await?;

    let mut functions = Vec::new();

    for row in rows {
        let definition: Option<String> = row.get("definition");
        if let Some(def) = definition {
            functions.push(FunctionInfo {
                schema_name: row.get("schema_name"),
                function_name: row.get("function_name"),
                definition: def,
            });
        }
    }

    Ok(functions)
}
