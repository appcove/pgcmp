use tokio_postgres::Client;

/// Metadata for an installed PostgreSQL extension
#[derive(Debug)]
pub struct ExtensionInfo {
    pub name: String,
    pub version: String,
    pub schema_name: String,
}

/// Fetch all installed extensions from the database
///
/// Extensions installed in system schemas are excluded — notably plpgsql,
/// which lives in pg_catalog in every database and is not user-managed.
pub async fn fetch_extensions(client: &Client) -> anyhow::Result<Vec<ExtensionInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                e.extname AS name,
                e.extversion AS version,
                n.nspname AS schema_name
            FROM pg_extension e
            JOIN pg_namespace n ON n.oid = e.extnamespace
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND left(n.nspname, 2) <> 'x_'
            ORDER BY e.extname
            "#,
            &[],
        )
        .await?;

    let mut extensions = Vec::new();

    for row in rows {
        extensions.push(ExtensionInfo {
            name: row.get("name"),
            version: row.get("version"),
            schema_name: row.get("schema_name"),
        });
    }

    Ok(extensions)
}
