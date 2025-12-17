# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build          # Build the project
cargo run -- <cmd>   # Run a command (init, pull, diff, test)
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

### Module Structure

- **`src/cli/`** - Clap CLI argument definitions
- **`src/commands/`** - Command implementations (init, pull, diff, test)
- **`src/db/`** - Database connectivity and schema extraction
  - `connection.rs` - tokio-postgres connection wrapper with 2s timeout
  - `extraction.rs` - Coordinates extraction of all object types
  - Individual files for each object type (tables, views, functions, indexes, constraints, triggers, sequences)
- **`src/schema/`** - Schema object types and file writing
  - `ObjectType` enum maps to file extensions (e.g., `.table.sql`, `.view.sql`)
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

### File Naming Convention

Schema objects are written to files as `{object_name}.{type}.sql`:
- Tables: `users.table.sql`
- Views: `active_users.view.sql`
- Functions: `get_user.function.sql`
- Indexes: `users_email_idx.index.sql`
- Constraints: `orders_user_fk.constraint.sql`
- Triggers: `audit_trigger.trigger.sql`
- Sequences: `users_id_seq.sequence.sql`

### User Project Structure (created by init)

```
project/
├── CONFIG.toml           # Database connection config
├── MIGRATION.sql         # User writes migration SQL here
├── new.database/         # Target schema (what we want)
│   └── {schema}.schema/
│       └── {object}.{type}.sql
└── old.database/         # Current schema (what we have)
    └── {schema}.schema/
        └── {object}.{type}.sql
```
