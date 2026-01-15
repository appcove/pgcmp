use crate::App;
use crate::cli::ApplyArgs;
use crate::config::Config;
use crate::db::DbConnection;
use crate::migration::{
    execute_migration, load_and_validate_migration, print_commit_banner, print_rollback_banner,
};
use anyhow::Context;

pub async fn run_apply(app: &'static App, args: ApplyArgs) -> anyhow::Result<()> {
    let config = Config::load(&app.path).context("Failed to load CONFIG.toml")?;

    let old_config = config
        .old
        .as_ref()
        .context("No 'old' database configured in CONFIG.toml")?;

    // Read and validate migration file
    let migration_path = app.path.join(&args.migration_file);
    let migration_sql = load_and_validate_migration(&migration_path)?;

    eprintln!("Applying migration: {}", migration_path.display());

    // Connect to old database (where we'll apply the migration)
    eprintln!("Connecting to old (source) database...");
    let old_conn = DbConnection::connect(&old_config.connection_string())
        .await
        .context("Failed to connect to old database")?;

    // Apply migration
    eprintln!("Applying migration...");
    let result = execute_migration(&old_conn, &migration_sql, args.commit).await?;

    if result.committed {
        print_commit_banner();
    } else {
        print_rollback_banner();
    }

    Ok(())
}
