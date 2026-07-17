# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build          # Build the project
cargo run -- <cmd>   # Run a command (init, pull, diff, test, apply)
cargo test           # Run tests
```

## Architecture

pgcmp is a PostgreSQL schema comparison CLI tool written in Rust. It compares "new" (target/dev) and "old" (current/prod) database schemas to facilitate migration authoring.

### Commands

| Command | Purpose |
|---------|---------|
| `pgcmp init` | Interactive TUI setup - configure connections, verify they work |
| `pgcmp pull` | Extract schemas from both databases to `new.database/` and `old.database/` directories |
| `pgcmp diff` | Live comparison between databases, print differences |
| `pgcmp test` | Apply `MIGRATION.sql` in transaction, show diff + row counts, rollback |
| `pgcmp apply` | Apply `MIGRATION.sql` to old database (rollback by default, use `--commit` to persist) |

### Module Structure

- **`src/cli/`** - Clap CLI argument definitions
- **`src/commands/`** - Command implementations (init, pull, diff, test, apply)
- **`src/migration.rs`** - Shared migration validation and execution (validates BEGIN/ROLLBACK structure)
- **`src/db/`** - Database connectivity and schema extraction
  - `connection.rs` - tokio-postgres connection wrapper with 2s timeout
  - `extraction.rs` - Coordinates extraction of all object types
  - Individual files for each object type (extensions, types, tables, views, functions, indexes, constraints, triggers, sequences)
- **`src/schema/`** - Schema object types and file writing
  - `ObjectType` enum includes Extension, Type, Table, View, MaterializedView, Function, Index, Constraint, Trigger, Sequence
  - `SchemaObject` represents extracted DDL with schema/object names
- **`src/config.rs`** - CONFIG.toml parsing with `DbConfig` (host, port, user, password, database, tls)
- **`src/comparison/`** - Schema diffing logic
- **`src/git/`** - Git operations via git2

### Key Patterns

- Uses tokio async runtime with tokio-postgres for database operations
- `App` struct is leaked to `&'static` for easy sharing across async boundaries
- `init` command uses ratatui TUI with crossterm backend for interactive configuration
- Connection strings use `sslmode` parameter based on `TlsMode` (disable/require)
- Background connection testing uses `tokio::sync::oneshot` channels

### Schema File Layout

`pull` writes one `.sql` file per PostgreSQL schema (`src/schema/writer.rs`). Within each file, objects are grouped into sections by type in dependency-friendly order: extensions, types, sequences, tables, indexes, constraints, views, materialized views, functions, triggers.

### User Project Structure (created by init)

```
project/
├── CONFIG.toml           # Database connection config
├── MIGRATION.sql         # User writes migration SQL here
├── new.database/         # Target schema (what we want)
│   └── {schema}.sql      # e.g. public.sql
└── old.database/         # Current schema (what we have)
    └── {schema}.sql
```

### Migration File Format

Migration files MUST have the following structure:

```sql
BEGIN TRANSACTION;

-- Your migration SQL here
ALTER TABLE users ADD COLUMN email VARCHAR(255);
CREATE INDEX idx_users_email ON users(email);

ROLLBACK;
```

- First statement must be `BEGIN TRANSACTION;` (or `BEGIN;`)
- Last statement must be `ROLLBACK;`
- No COMMIT, extra BEGIN, or extra ROLLBACK statements allowed in the middle
- This format ensures migrations are always safe to test without accidentally applying them
