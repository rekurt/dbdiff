# Contributing to dbdiff

Thanks for your interest in contributing! This document covers everything you need to get started.

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) 1.75 or later
- PostgreSQL 12+ (for integration tests)
- Docker (optional, for running test databases)

### Getting started

```bash
git clone https://github.com/rekurt/dbdiff.git
cd dbdiff
cargo build
cargo test
```

### Running a local Postgres for testing

```bash
docker run -d --name dbdiff-test \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=test \
  -e POSTGRES_DB=dbdiff_test \
  postgres:16

# Run integration tests
DATABASE_URL=postgres://postgres:test@localhost/dbdiff_test cargo test
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` and fix all warnings
- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)

## Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add MySQL driver support
fix: handle quoted identifiers in SQL parser
docs: update installation instructions
test: add integration tests for index diff
refactor: extract column normalization logic
```

## Pull Request Process

1. Fork the repository and create a feature branch from `main`
2. Make your changes with clear, focused commits
3. Add or update tests for your changes
4. Ensure `cargo test`, `cargo clippy`, and `cargo fmt --check` all pass
5. Open a PR with a clear description of what and why

## Adding a New Database Driver

To add support for a new database, implement a loader in `src/loader/`:

1. Create `src/loader/yourdb.rs`
2. Implement a `pub async fn load(dsn: &str) -> Result<Schema, DbDiffError>` function
3. Add detection logic in `src/loader/mod.rs`
4. Add integration tests in `tests/`

See `src/loader/postgres.rs` as a reference implementation.

## Reporting Issues

- Use the [bug report template](https://github.com/rekurt/dbdiff/issues/new?template=bug_report.yml) for bugs
- Use the [feature request template](https://github.com/rekurt/dbdiff/issues/new?template=feature_request.yml) for ideas

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
