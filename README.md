# pgcmp

[![crates.io](https://img.shields.io/crates/v/pgcmp.svg)](https://crates.io/crates/pgcmp)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**PostgreSQL schema comparison and migration testing, built for AI-assisted migration authoring.**

pgcmp gives you (or your AI coding agent) a tight, verifiable feedback loop for writing schema migrations. Point it at two databases — the schema you *want* (`new`) and the schema you *have* (`old`) — then iterate on a single `MIGRATION.sql` file until `pgcmp test` reports zero differences. Every test run executes inside a transaction and rolls back, so nothing touches your database until you explicitly commit.

The `diff` and `test` commands emit structured XML on stdout, designed to be pasted into (or piped through) an AI agent's context so it can see exactly what its migration did and didn't accomplish — including per-table row count changes.

> **Project status:** this tool was largely implemented by AI (working under human direction) and is published because it has proven useful — not because it is finished. Expect rough edges, review its output before trusting it with production databases, and file issues when you hit problems.

## The workflow

```
┌──────────────┐     ┌───────────────────┐     ┌──────────────────────┐
│  pgcmp pull  │ ──▶ │ write MIGRATION.sql│ ──▶ │      pgcmp test      │
│  (snapshot   │     │  (human or agent)  │     │ (apply in txn, diff, │
│   schemas)   │     └───────────────────┘     │  rollback, report)   │
└──────────────┘               ▲               └──────────┬───────────┘
                               │        differences > 0   │
                               └──────────────────────────┤
                                                          │ differences = 0
                                                          ▼
                                              ┌──────────────────────┐
                                              │ pgcmp apply --commit │
                                              └──────────────────────┘
```

1. **`pgcmp init`** — interactive TUI to configure both database connections (writes `CONFIG.toml`).
2. **`pgcmp pull`** — snapshot both schemas to `new.database/` and `old.database/` as one SQL file per object. These directories are git-diffable and greppable — ideal context for an agent.
3. **Write `MIGRATION.sql`** — by hand, or by an AI agent working from the pulled schema files and diff output.
4. **`pgcmp test`** — applies the migration to the old database *inside a transaction*, compares the result against the new database, reports every remaining difference plus row-count changes as XML, then rolls back.
5. Iterate until `test` reports `<number_of_differences>0</number_of_differences>`.
6. **`pgcmp apply --commit`** — run the migration for real. Without `--commit`, apply also rolls back.

## Installation

```bash
cargo install pgcmp
```

Requires Rust 1.85+ and a C compiler (the build compiles [libpg_query](https://github.com/pganalyze/libpg_query), PostgreSQL's actual SQL parser, and libgit2).

## Quick start

```bash
mkdir my-migration && cd my-migration

# Interactive setup (TUI) — or pass connections directly:
pgcmp init --non-interactive \
  --new-connection "postgresql://user:pass@localhost:5432/myapp_dev" \
  --old-connection "postgresql://user:pass@localhost:5432/myapp_prod"

pgcmp pull      # snapshot both schemas to new.database/ and old.database/
pgcmp diff      # show current differences (exit code 2 if any)

# ... write MIGRATION.sql ...

pgcmp test      # safe dry-run: apply in txn, diff, show row counts, rollback
pgcmp apply --commit   # apply for real once test shows zero differences
```

## Commands

| Command | What it does |
|---------|--------------|
| `pgcmp init` | Interactive TUI setup: configure both connections, verify they work, write `CONFIG.toml`. Supports `--non-interactive` with `--new-connection` / `--old-connection`. |
| `pgcmp pull` | Extract both schemas to `new.database/` and `old.database/` directories (one file per object). `--new-only` / `--old-only` to limit. |
| `pgcmp diff` | Live comparison of the two databases. XML report on stdout. Exit code `2` if differences exist, `0` if schemas match. |
| `pgcmp test` | Validate and apply `MIGRATION.sql` to the old database in a transaction, capture before/after row counts, diff the migrated schema against the new database, print the XML report, then **roll back**. `--migration-file` to use a different file. |
| `pgcmp apply` | Apply `MIGRATION.sql` to the old database. **Rolls back by default**; pass `--commit` to actually persist. |

Progress and status messages go to stderr; the XML report goes to stdout, so you can redirect or pipe the report cleanly.

## Migration file format

pgcmp validates `MIGRATION.sql` with PostgreSQL's own parser before running it. The file must be wrapped in an explicit transaction that ends in `ROLLBACK`:

```sql
BEGIN TRANSACTION;

ALTER TABLE users ADD COLUMN email VARCHAR(255);
CREATE INDEX idx_users_email ON users(email);

ROLLBACK;
```

- The first statement must be `BEGIN TRANSACTION;` (or `BEGIN;`).
- The last statement must be `ROLLBACK;`.
- No `COMMIT`, extra `BEGIN`, or extra `ROLLBACK` anywhere in between (savepoints are allowed).

This makes the file safe by construction: running it in `psql` by accident applies nothing. Only `pgcmp apply --commit` swaps the final `ROLLBACK` for a `COMMIT` — and it prints an unmissable banner telling you which one happened.

## What it compares

Schemas, types (enums, composites, domains, ranges), tables, columns (type, nullability, defaults), views, materialized views, functions, indexes, constraints, triggers, and sequences.

For each difference, the report states the action needed to converge the old schema on the new one (`create table app.orders`, `alter column app.users.email`, `drop index app.stale_idx`, ...) with old/new details for modifications. `pgcmp test` additionally reports per-table row count changes so data migrations are verifiable too.

## Project layout

`pgcmp init` and `pgcmp pull` produce a small, self-contained project directory:

```
my-migration/
├── CONFIG.toml           # connection config for both databases
├── MIGRATION.sql         # the migration you're authoring
├── new.database/         # target schema (what you want)
│   └── {schema}.schema/
│       ├── users.table.sql
│       ├── active_users.view.sql
│       └── get_user.function.sql
└── old.database/         # current schema (what you have)
    └── {schema}.schema/
        └── ...
```

`CONFIG.toml`:

```toml
database_type = "postgresql"

[new]
host = "localhost"
port = 5432
user = "postgres"
password = "..."
database = "myapp_dev"
tls = "disable"   # or "require"

[old]
host = "localhost"
port = 5432
user = "postgres"
password = "..."
database = "myapp_prod"
tls = "disable"
```

## Using pgcmp with an AI agent

pgcmp was designed so an agent can drive the whole loop. A prompt (or `CLAUDE.md` entry) like this works well:

```markdown
Author a migration that converges the old database on the new schema.

1. Run `pgcmp pull`, then read `new.database/` and `old.database/` to
   understand both schemas.
2. Write `MIGRATION.sql` (BEGIN TRANSACTION; ... ROLLBACK;).
3. Run `pgcmp test` and read the XML report.
4. Repeat 2–3 until <number_of_differences> is 0 and the row count
   changes look correct.
5. Stop. A human runs `pgcmp apply --commit`.
```

The enforced `ROLLBACK` envelope means the agent can run `test` as many times as it needs without ever mutating the database.

## Notes

- The XML report includes both connection strings (including passwords) in its `<connections>` section — treat report output as sensitive.
- Comparison is name-based: an object renamed on one side shows up as a drop plus a create.
- If the two servers run different PostgreSQL major versions, the report includes a `<version_warning>`, since some differences can be artifacts of version-specific formatting.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
