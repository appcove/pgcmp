mod compare;
mod fetch;
mod format;

pub use compare::{TableDiff, compare_table_lists, compare_tables};
pub use fetch::{ColumnInfo, TableInfo, fetch_tables};
pub use format::format_table_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_tables(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let tables = fetch_tables(client).await?;
    let mut objects = Vec::new();

    for table in tables {
        let ddl = format_table_ddl(&table);
        objects.push(SchemaObject {
            schema_name: table.schema_name,
            object_name: table.table_name,
            object_type: ObjectType::Table,
            ddl,
        });
    }

    Ok(objects)
}
