use crate::App;
use crate::cli::DiffArgs;
use crate::comparison::{analyze_databases, generate_xml, get_postgres_version};
use crate::config::Config;
use crate::db::postgres::DbPool;
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
    eprintln!("Connecting to left (new) and right (old) databases...");
    let (left_pool, right_pool) = tokio::try_join!(
        async {
            DbPool::connect(&new_config.connection_string())
                .await
                .context("Failed to connect to new database")
        },
        async {
            DbPool::connect(&old_config.connection_string())
                .await
                .context("Failed to connect to old database")
        },
    )?;

    // Version info and schema objects, from both databases at once
    eprintln!("Fetching schemas...");
    let (left_version, right_version, analysis) = tokio::try_join!(
        async { get_postgres_version(&*left_pool.get().await?).await },
        async { get_postgres_version(&*right_pool.get().await?).await },
        analyze_databases(&left_pool, &right_pool),
    )?;

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
