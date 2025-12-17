use tokio_postgres::Client;

/// Metadata for a single column
#[derive(Debug)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub column_default: Option<String>,
    pub identity_generation: Option<String>,
}

/// Metadata for a table
#[derive(Debug)]
pub struct TableInfo {
    pub schema_name: String,
    pub table_name: String,
    pub columns: Vec<ColumnInfo>,
}

/// Fetch all user tables from the database
pub async fn fetch_tables(client: &Client) -> anyhow::Result<Vec<TableInfo>> {
    // Get all user tables (exclude system schemas)
    let table_rows = client
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

    let mut tables = Vec::new();

    for row in table_rows {
        let schema_name: String = row.get("schema_name");
        let table_name: String = row.get("table_name");

        let columns = fetch_columns(client, &schema_name, &table_name).await?;

        tables.push(TableInfo {
            schema_name,
            table_name,
            columns,
        });
    }

    Ok(tables)
}

/// Fetch column information for a specific table
async fn fetch_columns(
    client: &Client,
    schema_name: &str,
    table_name: &str,
) -> anyhow::Result<Vec<ColumnInfo>> {
    let rows = client
        .query(
            r#"
            SELECT
                a.attname AS column_name,
                pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
                NOT a.attnotnull AS is_nullable,
                pg_get_expr(d.adbin, d.adrelid) AS column_default,
                a.attidentity::text AS identity_generation
            FROM pg_attribute a
            JOIN pg_class c ON c.oid = a.attrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
            WHERE n.nspname = $1
              AND c.relname = $2
              AND a.attnum > 0
              AND NOT a.attisdropped
            ORDER BY a.attnum
            "#,
            &[&schema_name, &table_name],
        )
        .await?;

    let mut columns = Vec::new();

    for row in rows {
        let identity: String = row.get("identity_generation");
        let identity_generation = if identity.is_empty() {
            None
        } else {
            Some(identity)
        };

        columns.push(ColumnInfo {
            name: row.get("column_name"),
            data_type: row.get("data_type"),
            is_nullable: row.get("is_nullable"),
            column_default: row.get("column_default"),
            identity_generation,
        });
    }

    Ok(columns)
}
