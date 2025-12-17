use crate::App;
use crate::cli::PullArgs;
use crate::config::Config;
use crate::db::{DbConnection, SchemaExtractor};
use crate::schema::write_objects_by_schema;
use anyhow::Context;
use std::fs;

pub async fn run_pull(app: &'static App, args: PullArgs) -> anyhow::Result<()> {
    let config = Config::load(&app.path).context("Failed to load CONFIG.toml")?;

    let pull_new = !args.old_only;
    let pull_old = !args.new_only;

    if pull_new {
        let new_config = config
            .new
            .as_ref()
            .context("No 'new' database configured in CONFIG.toml")?;

        println!("Connecting to new database...");
        let conn = DbConnection::connect(&new_config.connection_string())
            .await
            .context("Failed to connect to new database")?;

        println!("Extracting schema from new database...");
        let extractor = SchemaExtractor::new(&conn);
        let objects = extractor
            .extract_all()
            .await
            .context("Failed to extract schema from new database")?;

        let new_dir = app.path.join("new.database");
        // Clean existing directory
        if new_dir.exists() {
            fs::remove_dir_all(&new_dir)?;
        }
        fs::create_dir_all(&new_dir)?;

        write_objects_by_schema(&new_dir, &objects)?;
        println!("Wrote {} objects to {}", objects.len(), new_dir.display());
    }

    if pull_old {
        let old_config = config
            .old
            .as_ref()
            .context("No 'old' database configured in CONFIG.toml")?;

        println!("Connecting to old database...");
        let conn = DbConnection::connect(&old_config.connection_string())
            .await
            .context("Failed to connect to old database")?;

        println!("Extracting schema from old database...");
        let extractor = SchemaExtractor::new(&conn);
        let objects = extractor
            .extract_all()
            .await
            .context("Failed to extract schema from old database")?;

        let old_dir = app.path.join("old.database");
        // Clean existing directory
        if old_dir.exists() {
            fs::remove_dir_all(&old_dir)?;
        }
        fs::create_dir_all(&old_dir)?;

        write_objects_by_schema(&old_dir, &objects)?;
        println!("Wrote {} objects to {}", objects.len(), old_dir.display());
    }

    println!("Pull complete!");
    Ok(())
}
