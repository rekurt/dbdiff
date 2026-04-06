# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2024-04-06

### Added

- Initial release
- PostgreSQL schema introspection via `information_schema` and `pg_indexes`
- SQL file parser for `CREATE TABLE` and `CREATE INDEX` statements
- Schema diff engine: tables, columns, indexes (added / removed / modified)
- Migration SQL generation with safe ordering (DROP INDEX → DROP COLUMN → DROP TABLE → CREATE TABLE → ADD COLUMN → ALTER COLUMN → CREATE INDEX)
- Colored terminal output with diff markers (`+` / `-` / `~`)
- `--ci` flag with non-zero exit code on schema drift
- `--out` flag to write migration SQL to a file
- `--dry-run` flag for preview without side effects
- `--format json` for machine-readable output
- Locking warnings for dangerous ALTER operations
- `.dbdiff.yml` configuration file support (ignore tables/columns)

[Unreleased]: https://github.com/rekurt/dbdiff/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rekurt/dbdiff/releases/tag/v0.1.0
