use crate::App;
use crate::cli::PullArgs;
use crate::config::{Config, DbConfig};
use crate::db::postgres::{DbPool, SchemaExtractor};
use crate::schema::write_objects_by_schema;
use anyhow::Context;
use std::fs;
use std::path::Path;

pub async fn run_pull(app: &'static App, args: PullArgs) -> anyhow::Result<()> {
    let config = Config::load(&app.path).context("Failed to load CONFIG.toml")?;

    let new_dir = app.path.join("new.database");
    let old_dir = app.path.join("old.database");

    // Both databases are pulled concurrently
    tokio::try_join!(
        async {
            if args.old_only {
                return Ok(());
            }
            pull_database("new", config.new.as_ref(), &new_dir).await
        },
        async {
            if args.new_only {
                return Ok(());
            }
            pull_database("old", config.old.as_ref(), &old_dir).await
        },
    )?;

    println!("Pull complete!");
    Ok(())
}

/// Extract one database's schema and write it to `dir`, replacing its contents.
/// `label` is "new" or "old".
async fn pull_database(label: &str, config: Option<&DbConfig>, dir: &Path) -> anyhow::Result<()> {
    let config =
        config.with_context(|| format!("No '{label}' database configured in CONFIG.toml"))?;

    println!("Connecting to {label} database...");
    let pool = DbPool::connect(&config.connection_string())
        .await
        .with_context(|| format!("Failed to connect to {label} database"))?;

    println!("Extracting schema from {label} database...");
    let objects = SchemaExtractor::new(&pool)
        .extract_all()
        .await
        .with_context(|| format!("Failed to extract schema from {label} database"))?;

    // Clean existing directory
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir)?;

    write_objects_by_schema(dir, &objects)?;
    println!("Wrote {} objects to {}", objects.len(), dir.display());
    Ok(())
}
