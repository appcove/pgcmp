use super::DbPool;
use super::constraints::extract_constraints;
use super::extensions::extract_extensions;
use super::functions::extract_functions;
use super::indexes::extract_indexes;
use super::sequences::extract_sequences;
use super::tables::extract_tables;
use super::triggers::extract_triggers;
use super::types::extract_types;
use super::views::{extract_materialized_views, extract_views};
use crate::schema::SchemaObject;

pub struct SchemaExtractor<'a> {
    pool: &'a DbPool,
}

impl<'a> SchemaExtractor<'a> {
    pub fn new(pool: &'a DbPool) -> Self {
        Self { pool }
    }

    /// Extract every object type, each on its own pooled connection.
    pub async fn extract_all(&self) -> anyhow::Result<Vec<SchemaObject>> {
        let pool = self.pool;

        let (
            extensions,
            types,
            tables,
            views,
            materialized_views,
            functions,
            indexes,
            constraints,
            triggers,
            sequences,
        ) = tokio::try_join!(
            async { extract_extensions(pool.get().await?.client()).await },
            async { extract_types(pool.get().await?.client()).await },
            async { extract_tables(pool.get().await?.client()).await },
            async { extract_views(pool.get().await?.client()).await },
            async { extract_materialized_views(pool.get().await?.client()).await },
            async { extract_functions(pool.get().await?.client()).await },
            async { extract_indexes(pool.get().await?.client()).await },
            async { extract_constraints(pool.get().await?.client()).await },
            async { extract_triggers(pool.get().await?.client()).await },
            async { extract_sequences(pool.get().await?.client()).await },
        )?;

        // Extensions first (other objects may depend on them), then types, then the rest
        Ok([
            extensions,
            types,
            tables,
            views,
            materialized_views,
            functions,
            indexes,
            constraints,
            triggers,
            sequences,
        ]
        .into_iter()
        .flatten()
        .collect())
    }
}
