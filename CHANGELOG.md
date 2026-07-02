# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/appcove/pgcmp/releases/tag/v0.1.0
