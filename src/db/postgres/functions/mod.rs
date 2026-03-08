mod compare;
mod fetch;
mod format;

pub use compare::{FunctionDiff, compare_function_lists, compare_functions};
pub use fetch::{FunctionInfo, fetch_functions};
pub use format::format_function_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_functions(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let functions = fetch_functions(client).await?;
    let mut objects = Vec::new();

    for func in functions {
        let ddl = format_function_ddl(&func);
        objects.push(SchemaObject {
            schema_name: func.schema_name,
            object_name: func.function_name,
            object_type: ObjectType::Function,
            ddl,
        });
    }

    Ok(objects)
}
