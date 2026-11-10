use std::collections::HashMap;
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
    // Tables (exclude system schemas) and the columns of every table, as two
    // queries pipelined on the connection rather than one column query per table.
    let (table_rows, column_rows) = tokio::try_join!(
        client.query(
            r#"
            SELECT
                c.oid AS table_oid,
                n.nspname AS schema_name,
                c.relname AS table_name
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relkind = 'r'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND left(n.nspname, 2) <> 'x_'
              AND NOT EXISTS (
                  SELECT 1 FROM pg_depend d
                  WHERE d.classid = 'pg_class'::regclass
                    AND d.objid = c.oid AND d.deptype = 'e'
              )
            ORDER BY n.nspname, c.relname
            "#,
            &[],
        ),
        client.query(
            r#"
            SELECT
                a.attrelid AS table_oid,
                a.attname AS column_name,
                pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
                NOT a.attnotnull AS is_nullable,
                pg_get_expr(d.adbin, d.adrelid) AS column_default,
                a.attidentity::text AS identity_generation
            FROM pg_attribute a
            JOIN pg_class c ON c.oid = a.attrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
            WHERE c.relkind = 'r'
              AND n.nspname NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND n.nspname NOT LIKE 'pg_temp_%'
              AND left(n.nspname, 2) <> 'x_'
              AND a.attnum > 0
              AND NOT a.attisdropped
            ORDER BY a.attrelid, a.attnum
            "#,
            &[],
        ),
    )?;

    // Columns grouped by table oid, in attnum order
    let mut columns_by_table: HashMap<u32, Vec<ColumnInfo>> = HashMap::new();

    for row in column_rows {
        let identity: String = row.get("identity_generation");
        let identity_generation = if identity.is_empty() {
            None
        } else {
            Some(identity)
        };

        columns_by_table
            .entry(row.get("table_oid"))
            .or_default()
            .push(ColumnInfo {
                name: row.get("column_name"),
                data_type: row.get("data_type"),
                is_nullable: row.get("is_nullable"),
                column_default: row.get("column_default"),
                identity_generation,
            });
    }

    let tables = table_rows
        .into_iter()
        .map(|row| {
            let table_oid: u32 = row.get("table_oid");
            TableInfo {
                schema_name: row.get("schema_name"),
                table_name: row.get("table_name"),
                columns: columns_by_table.remove(&table_oid).unwrap_or_default(),
            }
        })
        .collect();

    Ok(tables)
}
