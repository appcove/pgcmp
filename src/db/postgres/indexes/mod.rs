mod compare;
mod fetch;
mod format;

pub use compare::{IndexDiff, compare_index_lists, compare_indexes};
pub use fetch::{IndexInfo, fetch_indexes};
pub use format::format_index_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_indexes(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let indexes = fetch_indexes(client).await?;
    let mut objects = Vec::new();

    for idx in indexes {
        let ddl = format_index_ddl(&idx);
        objects.push(SchemaObject {
            schema_name: idx.schema_name,
            object_name: idx.index_name,
            object_type: ObjectType::Index,
            ddl,
        });
    }

    Ok(objects)
}
