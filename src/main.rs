use clap::Parser;
use pgcmp::App;
use pgcmp::cli::{Cli, Command};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let app = App::new(env::current_dir()?).leak();

    match cli.command {
        Command::Init(args) => pgcmp::commands::run_init(app, args).await,
        Command::Pull(args) => pgcmp::commands::run_pull(app, args).await,
        Command::Diff(args) => pgcmp::commands::run_diff(app, args).await,
        Command::Test(args) => pgcmp::commands::run_test(app, args).await,
        Command::Apply(args) => pgcmp::commands::run_apply(app, args).await,
    }
}
