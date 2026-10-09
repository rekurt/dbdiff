# Contributing to dbdiff

[Back to README](README.md) · [Architecture](doc/architecture.md) · [CLI guide](doc/cli.md)

## Get started

Use Rust 1.88+ with `rustfmt` and `clippy`. PostgreSQL/MySQL builds may need TLS development libraries (Ubuntu: `libssl-dev pkg-config`). SQLite is bundled. No database server is required for the checked-in test suite or offline demo.

```bash
git clone https://github.com/rekurt/dbdiff.git
cd dbdiff
git switch -c your-change
cargo build --locked
cargo test --locked
python3 examples/demo/run.py --bin target/debug/dbdiff
```

For Windows, use `python` and `target/debug/dbdiff.exe`. The demo requires Python 3.9+ and uses only its standard library.

## Check your change

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-Dwarnings" cargo doc --no-deps --document-private-items --all-features
```

CI runs tests and the offline demo on Linux, macOS, and Windows with stable Rust, plus Rust 1.88 on Linux. The minimum version matches the current locked dependency requirements.

For README or demo changes, run the [demo verification and visual regeneration](examples/demo/README.md). Keep examples aligned with actual CLI output, and check local documentation links and image paths.

## Make a focused pull request

1. Create a branch from `master` (fork first if you do not have write access).
2. Describe the problem and resulting behavior.
3. Add meaningful tests for behavior changes; update affected documentation.
4. Run the relevant checks above and include the results in your PR.
5. Open a PR against `master`. Discuss large changes in an issue first.

Use Conventional Commits, for example `fix: handle quoted SQL identifiers` or `docs: refresh offline demo`.

## Project layout

```text
src/
  main.rs             Command dispatch
  cli.rs              Arguments, profiles, normalized diff options
  commands/           Diff, validate, tables, init, snapshot handlers
  model.rs            Shared schema and snapshot representation
  loader/             PostgreSQL, MySQL, SQLite, SQL-file loading
  diff.rs             Schema comparison and rename inference
  migration/          Forward / rollback generation and dialect helpers
  output.rs           Pretty and SQL rendering
  ci.rs               Structured reports, exit codes, annotations
  config/             Configuration and ignore filters
  error.rs            Error types and DSN sanitization
tests/                CLI, config, and constraint integration tests
examples/demo/        Offline schemas and verified demo runner
doc/                  CLI guide, architecture, and README visuals
scripts/              Demo visual renderer
```

## Backend changes

A loader converts its source into the common `Schema` model. Backend work also requires source dispatch, feature gating, dialect-specific SQL generation, warnings, and tests; do not assume the migration generator works unchanged for a new database. Start with [architecture](doc/architecture.md) and an existing loader.

Use disposable local databases for manual live checks. The repository's optional live schema workflow requires `ENABLE_LIVE_SCHEMA_CHECK=true` in repository variables and `PROD_DSN` / `STAGING_DSN` secrets. It is separate from the offline CI checks.

## Issues and conduct

Use the [bug report](https://github.com/rekurt/dbdiff/issues/new?template=bug_report.yml) or [feature request](https://github.com/rekurt/dbdiff/issues/new?template=feature_request.yml) template. Include the dbdiff version and a small reproducible schema; redact DSN credentials.

Follow the [code of conduct](CODE_OF_CONDUCT.md). See the [security policy](SECURITY.md) for vulnerability reports. Contributions are licensed under [MIT](LICENSE).
