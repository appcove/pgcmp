use super::DbConnection;
use super::constraints::extract_constraints;
use super::functions::extract_functions;
use super::indexes::extract_indexes;
use super::sequences::extract_sequences;
use super::tables::extract_tables;
use super::triggers::extract_triggers;
use super::types::extract_types;
use super::views::{extract_materialized_views, extract_views};
use crate::schema::SchemaObject;

pub struct SchemaExtractor<'a> {
    conn: &'a DbConnection,
}

impl<'a> SchemaExtractor<'a> {
    pub fn new(conn: &'a DbConnection) -> Self {
        Self { conn }
    }

    pub async fn extract_all(&self) -> anyhow::Result<Vec<SchemaObject>> {
        let client = self.conn.client();
        let mut objects = Vec::new();

        // Extract all object types (types first since other objects may depend on them)
        objects.extend(extract_types(client).await?);
        objects.extend(extract_tables(client).await?);
        objects.extend(extract_views(client).await?);
        objects.extend(extract_materialized_views(client).await?);
        objects.extend(extract_functions(client).await?);
        objects.extend(extract_indexes(client).await?);
        objects.extend(extract_constraints(client).await?);
        objects.extend(extract_triggers(client).await?);
        objects.extend(extract_sequences(client).await?);

        Ok(objects)
    }
}
