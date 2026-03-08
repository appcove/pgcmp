use tokio_postgres::Client;

/// Metadata for a view
#[derive(Debug)]
pub struct ViewInfo {
    pub schema_name: String,
    pub view_name: String,
    pub definition: String,
}

/// Fetch all user views from the database
pub async fn fetch_views(client: &Client) -> anyhow::Result<Vec<ViewInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                c.relname AS view_name,
                pg_get_viewdef(c.oid, true) AS definition
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind = 'v'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            ORDER BY n.nspname, c.relname
            "#,
            &[],
        )
        .await?;

    let mut views = Vec::new();

    for row in rows {
        views.push(ViewInfo {
            schema_name: row.get("schema_name"),
            view_name: row.get("view_name"),
            definition: row.get("definition"),
        });
    }

    Ok(views)
}

/// Fetch all materialized views from the database
pub async fn fetch_materialized_views(client: &Client) -> anyhow::Result<Vec<ViewInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                c.relname AS view_name,
                pg_get_viewdef(c.oid, true) AS definition
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind = 'm'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            ORDER BY n.nspname, c.relname
            "#,
            &[],
        )
        .await?;

    let mut views = Vec::new();

    for row in rows {
        views.push(ViewInfo {
            schema_name: row.get("schema_name"),
            view_name: row.get("view_name"),
            definition: row.get("definition"),
        });
    }

    Ok(views)
}
