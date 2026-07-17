mod fetch;
mod format;

pub use fetch::{ExtensionInfo, fetch_extensions};
pub use format::format_extension_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_extensions(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let extensions = fetch_extensions(client).await?;
    let mut objects = Vec::new();

    for ext in extensions {
        let ddl = format_extension_ddl(&ext);
        objects.push(SchemaObject {
            schema_name: ext.schema_name,
            object_name: ext.name,
            object_type: ObjectType::Extension,
            ddl,
        });
    }

    Ok(objects)
}
