# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `pgcmp test --row-counts-exclude <PATTERN>` skips schemas matching a glob pattern (`*`, `?`) when counting rows. Repeatable; requires `--row-counts`. The report lists the schemas each pattern excluded under `<row_counts><excluded_schemas>`.

### Changed

- `pull`, `diff`, `test`, and the `init` pull are much faster: each database is read over up to 8 connections, so object types are fetched in parallel, both databases are read at the same time, and table columns come from one query instead of one per table. `test` fetches the new database while the migration runs on the old one, and `--row-counts` takes the "before" counts in parallel.
- `pgcmp init` handles git itself. It commits any uncommitted changes when it starts, and Save no longer refuses a dirty working tree: it commits changes made in the meantime, then commits what it wrote. In a directory without a repository, Save runs `git init` first. A failed commit is now reported instead of ignored. Existing changes are not auto-committed while `CONFIG.toml` is present but not git-ignored.
- The `pgcmp init` Instruct tab shows `INSTRUCT.md` read-only and edits it in a dialog opened with `[ Edit ]` (Ctrl+S applies, Esc cancels, with a second Esc required to discard edits). The tab no longer captures keyboard focus. `[ Revert ]` drops staged edits.

### Fixed

- The `pgcmp init` Instruct editor starts from the saved `INSTRUCT.md` instead of an empty buffer, so saving no longer overwrites existing instructions with nothing.

## [0.2.1] - 2026-10-16

### Fixed

- Same-named triggers on different tables in one schema are no longer compared against each other; triggers are matched by schema, table, and name.
- Overloaded functions are no longer compared against each other; functions are matched by their identity arguments and reported as `schema.name(args)`.
- Views whose `IN (...)` lists on `varchar` columns deparse differently after being re-created from `pg_get_viewdef` output are no longer reported as changed.

### Changed

- `pgcmp test` no longer counts rows by default, since counting every table is slow on large databases. Pass `--row-counts` for the before/after row count report, e.g. as the final check once the schema matches.
- Changed views, materialized views, and functions show a unified diff of the full definitions instead of a truncated prefix or "definition changed". Changed triggers show both full definitions.

## [0.2.0] - 2026-10-15

### Added

- Extension support: compare installed extensions (name, version, install schema) in `pull`, `diff`, and `test`.
- Extension-owned objects are excluded from individual object lists so diffs report `create/drop/alter extension` instead of member noise.

### Changed

- Schemas starting with the literal, case-sensitive prefix `x_` are automatically excluded from schema extraction, comparisons, and migration test row counts in both databases.

## [0.1.0] - 2026-07-02

Initial release.

### Added

- `pgcmp init` — interactive TUI setup for the new/old database connections, with `--non-interactive` support.
- `pgcmp pull` — extract both schemas to `new.database/` and `old.database/` directories, one SQL file per object.
- `pgcmp diff` — live schema comparison with an XML report; exits with code 2 when differences exist.
- `pgcmp test` — apply `MIGRATION.sql` in a transaction, diff the migrated schema against the target, report differences and per-table row count changes, then roll back.
- `pgcmp apply` — apply `MIGRATION.sql` to the old database; rolls back by default, `--commit` to persist.
- Migration file validation using PostgreSQL's own parser (libpg_query): enforced `BEGIN TRANSACTION; ... ROLLBACK;` envelope.
- Comparison coverage: schemas, types (enums, composites, domains, ranges), tables, columns, views, materialized views, functions, indexes, constraints, triggers, and sequences.

[Unreleased]: https://github.com/appcove/pgcmp/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/appcove/pgcmp/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/appcove/pgcmp/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/appcove/pgcmp/releases/tag/v0.1.0
