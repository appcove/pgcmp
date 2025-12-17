use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pgcmp")]
#[command(about = "PostgreSQL schema comparison tool for Claude-assisted migrations")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new pgcmp project
    Init(InitArgs),

    /// Pull schemas from both databases to local files
    Pull(PullArgs),

    /// Compare schemas between databases and show differences
    Diff(DiffArgs),

    /// Test MIGRATION.sql in a transaction (with rollback)
    Test(TestArgs),
}

#[derive(Parser)]
pub struct InitArgs {
    /// New database connection string (target state)
    #[arg(long)]
    pub new_connection: Option<String>,

    /// Old database connection string (current state)
    #[arg(long)]
    pub old_connection: Option<String>,

    /// Skip interactive prompts (requires connection strings)
    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(Parser)]
pub struct PullArgs {
    /// Only pull from the new database
    #[arg(long)]
    pub new_only: bool,

    /// Only pull from the old database
    #[arg(long)]
    pub old_only: bool,
}

#[derive(Parser)]
pub struct DiffArgs {}

#[derive(Parser)]
pub struct TestArgs {
    /// Migration file to test
    #[arg(long, default_value = "MIGRATION.sql")]
    pub migration_file: PathBuf,
}
