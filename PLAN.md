# pgcmp - PostgreSQL Schema Comparison Tool (Rust)

## Overview

`pgcmp` is a CLI tool for comparing PostgreSQL database schemas and facilitating Claude-assisted migration authoring.

## Commands

| Command | Run by | Purpose |
|---------|--------|---------|
| `pgcmp init` | Human | Interactive setup: create directory structure, prompt for connection strings, verify connections work, then auto-pull |
| `pgcmp pull` | Human | Sync both databases → `new.database/` and `old.database/` files |
| `pgcmp diff` | Human/CI | Connect to both databases live, compare schemas, print differences |
| `pgcmp test` | Human/CI | Apply `MIGRATION.sql` in transaction, show diff + row counts, rollback |

## Workflow

```
pgcmp init                    # One-time setup
    ↓
pgcmp pull                    # Dump schemas to files
    ↓
Claude writes MIGRATION.sql   # AI analyzes diff, writes migration
    ↓
pgcmp test                    # Validate migration (rolled back)
    ↓
Apply MIGRATION.sql for real  # Outside of pgcmp
```

## User Project Structure

Created by `pgcmp init`:

```
my-project/
├── MIGRATION.sql
├── new.database/
│   ├── public.schema/
│   │   ├── users.table.sql
│   │   ├── orders.table.sql
│   │   ├── active_users.view.sql
│   │   ├── get_user.function.sql
│   │   ├── users_email_idx.index.sql
│   │   ├── orders_user_fk.constraint.sql
│   │   ├── audit_trigger.trigger.sql
│   │   └── users_id_seq.sequence.sql
│   └── inventory.schema/
│       └── products.table.sql
├── old.database/
│   └── public.schema/
│       └── ...
├── .git/
├── .claudeignore
├── CONFIG.toml
└── CLAUDE.md
```

## File Naming Conventions

- Database directories: `{name}.database/`
- Schema directories: `{schema_name}.schema/`
- Object files: `{object_name}.{type}.sql`

### Object Types

| Type | File Pattern | Example |
|------|--------------|---------|
| Table | `{name}.table.sql` | `users.table.sql` |
| View | `{name}.view.sql` | `active_users.view.sql` |
| Materialized View | `{name}.matview.sql` | `summary.matview.sql` |
| Function | `{name}.function.sql` | `get_user.function.sql` |
| Index | `{name}.index.sql` | `users_email_idx.index.sql` |
| Constraint | `{name}.constraint.sql` | `orders_user_fk.constraint.sql` |
| Trigger | `{name}.trigger.sql` | `audit_trigger.trigger.sql` |
| Sequence | `{name}.sequence.sql` | `users_id_seq.sequence.sql` |

## Configuration

### CONFIG.toml

```toml
new_connection = "postgresql://localhost/myapp_dev"
old_connection = "postgresql://localhost/myapp_prod"
```

### .claudeignore

```
CONFIG.toml
```

### CLAUDE.md (auto-generated)

```markdown
# pgcmp Project

This directory contains PostgreSQL schema snapshots for comparison.

## Structure

- `new.database/` - Target state (what we want)
- `old.database/` - Current state (what we have)
- `MIGRATION.sql` - Write migration SQL here to transform old → new

## Task

Compare schemas in `new.database/` and `old.database/`. Write SQL
statements in `MIGRATION.sql` that will alter the old database to
match the new database structure.
```

## Command Details

### `pgcmp init`

Interactive initialization:

1. Check if `.git/` exists, if not initialize git repository
2. Prompt for `new_connection` string
3. Verify connection works
4. Prompt for `old_connection` string
5. Verify connection works
6. Create `CONFIG.toml`
7. Create `.claudeignore`
8. Create `CLAUDE.md`
9. Create `MIGRATION.sql` (empty)

### `pgcmp pull`

Dump schemas to files:

1. Read `CONFIG.toml`
2. Connect to new database
3. Extract all schema objects → `new.database/`
4. Connect to old database
5. Extract all schema objects → `old.database/`
6. Git commit (optional? automatic?)

### `pgcmp diff`

Live schema comparison:

1. Read `CONFIG.toml`
2. Connect to both databases
3. Compare schemas (like Python version)
4. Print differences table

### `pgcmp test`

Test migration in transaction:

1. Read `CONFIG.toml`
2. Connect to old database
3. BEGIN transaction
4. Capture row counts (before)
5. Apply `MIGRATION.sql`
6. Capture row counts (after)
7. Connect to new database
8. Run schema diff (new vs old-with-migration-applied)
9. Print diff + row count changes
10. ROLLBACK transaction

## Schema Extraction

Objects are extracted via SQL queries against PostgreSQL catalogs:

| Object | Source | DDL Function |
|--------|--------|--------------|
| Table | `pg_class` + `pg_attribute` | Manual construction |
| View | `pg_views` | `pg_get_viewdef()` |
| Materialized View | `pg_matviews` | `pg_get_viewdef()` |
| Function | `pg_proc` | `pg_get_functiondef()` |
| Index | `pg_index` | `pg_get_indexdef()` |
| Constraint | `pg_constraint` | `pg_get_constraintdef()` |
| Trigger | `pg_trigger` | `pg_get_triggerdef()` |
| Sequence | `pg_sequence` | Manual construction |

No `psql` or `pg_dump` required - uses direct PostgreSQL wire protocol via Rust `postgres` crate.

## Rust Project Structure

```
pgcmp/
├── Cargo.toml
├── Cargo.lock
├── PLAN.md
├── src/
│   ├── main.rs                 # CLI entry point, clap setup
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── init.rs             # pgcmp init
│   │   ├── pull.rs             # pgcmp pull
│   │   ├── diff.rs             # pgcmp diff
│   │   └── test.rs             # pgcmp test
│   ├── db/
│   │   ├── mod.rs
│   │   ├── connection.rs       # PostgreSQL connection handling
│   │   ├── extraction.rs       # Schema extraction coordinator
│   │   ├── tables/
│   │   │   ├── mod.rs
│   │   │   ├── fetch.rs        # SQL queries to get table metadata
│   │   │   └── format.rs       # Format table metadata into DDL
│   │   ├── views/
│   │   │   ├── mod.rs
│   │   │   ├── fetch.rs
│   │   │   └── format.rs
│   │   ├── functions/
│   │   │   ├── mod.rs
│   │   │   ├── fetch.rs
│   │   │   └── format.rs
│   │   ├── indexes/
│   │   │   ├── mod.rs
│   │   │   ├── fetch.rs
│   │   │   └── format.rs
│   │   ├── constraints/
│   │   │   ├── mod.rs
│   │   │   ├── fetch.rs
│   │   │   └── format.rs
│   │   ├── triggers/
│   │   │   ├── mod.rs
│   │   │   ├── fetch.rs
│   │   │   └── format.rs
│   │   └── sequences/
│   │       ├── mod.rs
│   │       ├── fetch.rs
│   │       └── format.rs
│   ├── schema/
│   │   ├── mod.rs
│   │   ├── writer.rs           # Write objects to files
│   │   └── reader.rs           # Read objects from files
│   ├── comparison/
│   │   ├── mod.rs
│   │   └── diff.rs             # Schema comparison logic
│   └── git/
│       ├── mod.rs
│       └── operations.rs       # Git operations via git2
└── tests/
```

## Dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
postgres = "0.19"
toml = "0.8"
git2 = "0.18"
anyhow = "1"
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success / No differences |
| 1 | Error (connection, file, etc.) |
| 2 | Differences detected |
