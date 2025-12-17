use tokio_postgres::Client;

/// Metadata for a trigger
#[derive(Debug)]
pub struct TriggerInfo {
    pub schema_name: String,
    pub table_name: String,
    pub trigger_name: String,
    pub definition: String,
}

/// Fetch all user triggers from the database
pub async fn fetch_triggers(client: &Client) -> anyhow::Result<Vec<TriggerInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                c.relname AS table_name,
                t.tgname AS trigger_name,
                pg_get_triggerdef(t.oid, true) AS definition
            FROM pg_trigger t
            JOIN pg_class c ON c.oid = t.tgrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE NOT t.tgisinternal
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            ORDER BY n.nspname, c.relname, t.tgname
            "#,
            &[],
        )
        .await?;

    let mut triggers = Vec::new();

    for row in rows {
        triggers.push(TriggerInfo {
            schema_name: row.get("schema_name"),
            table_name: row.get("table_name"),
            trigger_name: row.get("trigger_name"),
            definition: row.get("definition"),
        });
    }

    Ok(triggers)
}
