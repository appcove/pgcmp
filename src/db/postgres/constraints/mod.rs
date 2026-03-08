mod compare;
mod fetch;
mod format;

pub use compare::{ConstraintDiff, compare_constraint_lists, compare_constraints};
pub use fetch::{ConstraintInfo, fetch_constraints};
pub use format::format_constraint_ddl;

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_constraints(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let constraints = fetch_constraints(client).await?;
    let mut objects = Vec::new();

    for con in constraints {
        let ddl = format_constraint_ddl(&con);
        objects.push(SchemaObject {
            schema_name: con.schema_name,
            object_name: con.constraint_name,
            object_type: ObjectType::Constraint,
            ddl,
        });
    }

    Ok(objects)
}
