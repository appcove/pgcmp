mod compare;
mod fetch;
mod format;

pub use compare::{ViewDiff, compare_view_lists, compare_views};
pub use fetch::{ViewInfo, fetch_materialized_views, fetch_views};
pub use format::{format_matview_ddl, format_view_ddl};

use crate::schema::{ObjectType, SchemaObject};
use tokio_postgres::Client;

pub async fn extract_views(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let views = fetch_views(client).await?;
    let mut objects = Vec::new();

    for view in views {
        let ddl = format_view_ddl(&view);
        objects.push(SchemaObject {
            schema_name: view.schema_name,
            object_name: view.view_name,
            object_type: ObjectType::View,
            ddl,
        });
    }

    Ok(objects)
}

pub async fn extract_materialized_views(client: &Client) -> anyhow::Result<Vec<SchemaObject>> {
    let views = fetch::fetch_materialized_views(client).await?;
    let mut objects = Vec::new();

    for view in views {
        let ddl = format::format_matview_ddl(&view);
        objects.push(SchemaObject {
            schema_name: view.schema_name,
            object_name: view.view_name,
            object_type: ObjectType::MaterializedView,
            ddl,
        });
    }

    Ok(objects)
}
