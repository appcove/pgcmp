mod compare;
mod fetch;
mod format;

pub use compare::{TypeDiff, compare_type_lists, compare_types};
pub use fetch::{TypeInfo, TypeKind, fetch_types};
pub use format::format_type_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_types(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let types = fetch_types(client).await?;
    let mut objects = Vec::new();

    for type_info in types {
        let ddl = format_type_ddl(&type_info);
        objects.push(SchemaObject {
            schema_name: type_info.schema_name,
            object_name: type_info.type_name,
            object_type: ObjectType::Type,
            ddl,
        });
    }

    Ok(objects)
}
