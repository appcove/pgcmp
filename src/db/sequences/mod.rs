mod compare;
mod fetch;
mod format;

pub use compare::{SequenceDiff, compare_sequence_lists, compare_sequences};
pub use fetch::{SequenceInfo, fetch_sequences};
pub use format::format_sequence_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_sequences(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let sequences = fetch_sequences(client).await?;
    let mut objects = Vec::new();

    for seq in sequences {
        let ddl = format_sequence_ddl(&seq);
        objects.push(SchemaObject {
            schema_name: seq.schema_name,
            object_name: seq.sequence_name,
            object_type: ObjectType::Sequence,
            ddl,
        });
    }

    Ok(objects)
}
