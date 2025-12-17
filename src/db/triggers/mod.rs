mod compare;
mod fetch;
mod format;

pub use compare::{TriggerDiff, compare_trigger_lists, compare_triggers};
pub use fetch::{TriggerInfo, fetch_triggers};
pub use format::format_trigger_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_triggers(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let triggers = fetch_triggers(client).await?;
    let mut objects = Vec::new();

    for trig in triggers {
        let ddl = format_trigger_ddl(&trig);
        objects.push(SchemaObject {
            schema_name: trig.schema_name,
            object_name: trig.trigger_name,
            object_type: ObjectType::Trigger,
            ddl,
        });
    }

    Ok(objects)
}
