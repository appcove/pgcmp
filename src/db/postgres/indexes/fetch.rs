use tokio_postgres::Client;

/// Metadata for an index
#[derive(Debug)]
pub struct IndexInfo {
    pub schema_name: String,
    pub index_name: String,
    pub definition: String,
}

/// Fetch all user indexes from the database (excluding primary key indexes)
pub async fn fetch_indexes(client: &Client) -> anyhow::Result<Vec<IndexInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                c.relname AS index_name,
                pg_get_indexdef(c.oid) AS definition
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            JOIN pg_index i ON i.indexrelid = c.oid
            WHERE c.relkind = 'i'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND NOT i.indisprimary  -- exclude primary key indexes (handled with constraints)
            ORDER BY n.nspname, c.relname
            "#,
            &[],
        )
        .await?;

    let mut indexes = Vec::new();

    for row in rows {
        indexes.push(IndexInfo {
            schema_name: row.get("schema_name"),
            index_name: row.get("index_name"),
            definition: row.get("definition"),
        });
    }

    Ok(indexes)
}
