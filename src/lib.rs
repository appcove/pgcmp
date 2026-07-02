//! PostgreSQL schema comparison and migration testing, built for AI-assisted
//! migration authoring.
//!
//! `pgcmp` is primarily a CLI tool (`cargo install pgcmp`). It compares a
//! "new" database (the target schema you want) against an "old" database (the
//! current schema you have) and provides a verifiable feedback loop for
//! authoring a `MIGRATION.sql` that converges the old schema on the new one:
//!
//! 1. `pgcmp pull` — snapshot both schemas to local SQL files.
//! 2. Write `MIGRATION.sql` (by hand or with an AI agent).
//! 3. `pgcmp test` — apply the migration in a transaction, diff the result
//!    against the target, report differences and row-count changes, roll back.
//! 4. Iterate until zero differences, then `pgcmp apply --commit`.
//!
//! Migration files are validated with PostgreSQL's own parser and must be
//! wrapped in a `BEGIN TRANSACTION; ... ROLLBACK;` envelope, so they are safe
//! to test repeatedly — nothing persists until `apply --commit` swaps the
//! final `ROLLBACK` for a `COMMIT`.
//!
//! See the [README](https://github.com/appcove/pgcmp) for full documentation.

use std::path::PathBuf;

/// Clap argument definitions for the `pgcmp` CLI.
pub mod cli;
/// Implementations of the `init`, `pull`, `diff`, `test`, and `apply` commands.
pub mod commands;
/// Schema diffing logic.
pub mod comparison;
/// `CONFIG.toml` parsing: connection settings for the new and old databases.
pub mod config;
/// Database connectivity, schema extraction, and migration execution.
pub mod db;
/// Git operations for schema snapshot directories.
pub mod git;
/// In-memory filesystem helpers.
pub mod memfs;
/// Schema object types and snapshot file writing.
pub mod schema;

/// Application context shared by all commands.
///
/// Holds the project directory (where `CONFIG.toml`, `MIGRATION.sql`, and the
/// schema snapshot directories live). Leaked to `&'static` at startup so it
/// can be shared freely across async tasks.
pub struct App {
    /// The pgcmp project directory (usually the current working directory).
    pub path: PathBuf,
}

impl App {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Leak the app into a `&'static` reference for cheap sharing across
    /// async boundaries. Called once at startup.
    pub fn leak(self) -> &'static Self {
        Box::leak(Box::new(self))
    }
}
