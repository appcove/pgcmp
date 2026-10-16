use crate::App;
use crate::cli::DiffArgs;
use crate::comparison::{analyze_databases, generate_xml, get_postgres_version};
use crate::config::Config;
use crate::db::postgres::DbConnection;
use anyhow::Context;

pub async fn run_diff(app: &'static App, _args: DiffArgs) -> anyhow::Result<()> {
    let config = Config::load(&app.path).context("Failed to load CONFIG.toml")?;

    let new_config = config
        .new
        .as_ref()
        .context("No 'new' database configured in CONFIG.toml")?;

    let old_config = config
        .old
        .as_ref()
        .context("No 'old' database configured in CONFIG.toml")?;

    // Connect to both databases
    // Left = new (source/what you want), Right = old (target/what you have)
    eprintln!("Connecting to left (new) database...");
    let left_conn = DbConnection::connect(&new_config.connection_string())
        .await
        .context("Failed to connect to new database")?;

    eprintln!("Connecting to right (old) database...");
    let right_conn = DbConnection::connect(&old_config.connection_string())
        .await
        .context("Failed to connect to old database")?;

    // Get version info
    let left_version = get_postgres_version(&left_conn).await?;
    let right_version = get_postgres_version(&right_conn).await?;

    // Fetch schema objects from both databases
    eprintln!("Fetching schemas...");
    let analysis = analyze_databases(&left_conn, &right_conn).await?;

    // Generate XML output
    let xml = generate_xml(
        &analysis,
        &new_config.connection_string(),
        &old_config.connection_string(),
        &left_version,
        &right_version,
        "",
    );

    println!("{}", xml);

    // Exit with code 2 if there are differences
    if analysis.has_differences() {
        std::process::exit(2);
    }

    Ok(())
}
