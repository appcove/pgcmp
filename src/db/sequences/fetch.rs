use tokio_postgres::Client;

/// Metadata for a sequence
#[derive(Debug)]
pub struct SequenceInfo {
    pub schema_name: String,
    pub sequence_name: String,
    pub data_type: String,
    pub start_value: i64,
    pub min_value: i64,
    pub max_value: i64,
    pub increment_by: i64,
    pub cycle: bool,
    pub cache_size: i64,
}

/// Fetch all user sequences from the database
pub async fn fetch_sequences(client: &Client) -> anyhow::Result<Vec<SequenceInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                n.nspname AS schema_name,
                c.relname AS sequence_name,
                pg_catalog.format_type(s.seqtypid, NULL) AS data_type,
                s.seqstart AS start_value,
                s.seqmin AS min_value,
                s.seqmax AS max_value,
                s.seqincrement AS increment_by,
                s.seqcycle AS cycle,
                s.seqcache AS cache_size
            FROM pg_sequence s
            JOIN pg_class c ON c.oid = s.seqrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
            ORDER BY n.nspname, c.relname
            "#,
            &[],
        )
        .await?;

    let mut sequences = Vec::new();

    for row in rows {
        sequences.push(SequenceInfo {
            schema_name: row.get("schema_name"),
            sequence_name: row.get("sequence_name"),
            data_type: row.get("data_type"),
            start_value: row.get("start_value"),
            min_value: row.get("min_value"),
            max_value: row.get("max_value"),
            increment_by: row.get("increment_by"),
            cycle: row.get("cycle"),
            cache_size: row.get("cache_size"),
        });
    }

    Ok(sequences)
}
