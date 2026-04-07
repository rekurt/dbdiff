# dbdiff

[![Rust](https://img.shields.io/badge/rust-1.75+-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)
[![CI](https://img.shields.io/github/actions/workflow/status/rekurt/dbdiff/ci.yml?style=flat-square&label=CI)](https://github.com/rekurt/dbdiff/actions)
[![Release](https://img.shields.io/github/v/release/rekurt/dbdiff?style=flat-square)](https://github.com/rekurt/dbdiff/releases)

Compare database schemas across environments and generate safe migration SQL — in one command.

```
$ dbdiff postgres://prod/myapp postgres://staging/myapp

~ table: orders
  + column  paid_at        timestamptz NOT NULL DEFAULT now()
  + index   idx_orders_paid_at  ON orders(paid_at)
  - column  payment_date   varchar(32)

~ table: users
  + column  deleted_at     timestamptz

Generated migration → migration_20240406_143201.sql
```

---

## Why dbdiff?

Most teams discover schema drift at the worst possible moment — right before a deploy. Existing tools either support only one database, require heavy setup, or can't compare a live DB against a `.sql` file.

`dbdiff` is a single binary that works anywhere CI runs.

- **Zero dependencies** — one static binary, no runtime, no Docker required
- **DSN vs DSN** or **DSN vs SQL file** — compare any two sources
- **CI-native** — non-zero exit code on drift, structured output, GitHub Actions support
- **Safe migrations** — warns about locking operations before you run them
- **Multi-database** — Postgres today, MySQL and SQLite on the roadmap

---

## Install

**Cargo:**
```bash
cargo install dbdiff
```

**Binary** — download from [Releases](https://github.com/rekurt/dbdiff/releases) and put it in your `$PATH`:

```bash
# Linux (x86_64)
curl -sSL https://github.com/rekurt/dbdiff/releases/latest/download/dbdiff-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo mv dbdiff /usr/local/bin/

# macOS (Apple Silicon)
curl -sSL https://github.com/rekurt/dbdiff/releases/latest/download/dbdiff-aarch64-apple-darwin.tar.gz | tar xz
sudo mv dbdiff /usr/local/bin/
```

**From source:**
```bash
git clone https://github.com/rekurt/dbdiff.git
cd dbdiff
cargo build --release
# Binary is at ./target/release/dbdiff
```

---

## Usage

### Compare two live databases

```bash
dbdiff postgres://user:pass@prod-host/myapp \
       postgres://user:pass@staging-host/myapp
```

### Compare a live database against a schema file

```bash
dbdiff postgres://user:pass@prod-host/myapp --schema ./schema.sql
```

Useful during code review — verify that a migration file actually matches what's in production.

### Compare MySQL databases

```bash
dbdiff mysql://user:pass@prod-host/myapp mysql://user:pass@staging-host/myapp
```

### Compare a SQLite database against a schema file

```bash
dbdiff myapp.db --schema ./schema.sql
```

### Save the generated migration to a file

```bash
dbdiff postgres://prod/myapp postgres://staging/myapp \
  --out migration.sql
```

### CI mode — exit 1 if schemas differ

```bash
dbdiff postgres://prod/myapp postgres://staging/myapp --ci
```

Returns exit code `0` if schemas match, `1` if they differ. Use this in GitHub Actions, GitLab CI, or any pipeline.

### Use a custom config file

```bash
dbdiff postgres://prod/myapp postgres://staging/myapp --config ./my-config.yml
```

### JSON output for custom tooling

```bash
dbdiff postgres://prod/myapp postgres://staging/myapp --format json
```

---

## GitHub Actions

Add schema drift detection to every PR:

```yaml
# .github/workflows/schema-check.yml
name: Schema check

on: [pull_request]

jobs:
  schema-drift:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install dbdiff
        run: |
          curl -sSL https://github.com/rekurt/dbdiff/releases/latest/download/dbdiff-x86_64-unknown-linux-musl.tar.gz | tar xz
          sudo mv dbdiff /usr/local/bin/

      - name: Check for schema drift
        env:
          PROD_DSN: ${{ secrets.PROD_DSN }}
          STAGING_DSN: ${{ secrets.STAGING_DSN }}
        run: dbdiff "$PROD_DSN" "$STAGING_DSN" --ci
```

---

## What dbdiff detects

| Object         | Added | Removed | Modified |
|----------------|:-----:|:-------:|:--------:|
| Tables         | ✓     | ✓       | —        |
| Columns        | ✓     | ✓       | ✓        |
| Column types   | —     | —       | ✓        |
| Indexes        | ✓     | ✓       | ✓        |
| Unique constraints | ✓ | ✓       | —        |

### Locking warnings

Some ALTER operations lock the table on Postgres. `dbdiff` marks them explicitly:

```
⚠  ALTER TABLE orders ADD COLUMN paid_at timestamptz NOT NULL
   This operation will rewrite the table and acquire AccessExclusiveLock.
   Consider: ADD COLUMN ... DEFAULT NULL first, then backfill.
```

---

## Configuration

Create `.dbdiff.yml` in your project root:

```yaml
# .dbdiff.yml
ignore:
  tables:
    - _migrations
    - schema_version
  columns:
    - "*.created_at"   # ignore created_at in all tables
    - "sessions.*"     # ignore all columns in sessions table

output:
  format: pretty       # pretty | json | sql
  color: true
```

---

## Supported databases

| Database        | Status     | Version |
|-----------------|-----------|---------|
| PostgreSQL      | ✅ stable  | 12+     |
| MySQL / MariaDB | ✅ stable  | 8.0+    |
| SQLite          | ✅ stable  | 3.x     |

---

## Feature flags

Database backends are optional and can be toggled via Cargo features:

```bash
# Install with only PostgreSQL support
cargo install dbdiff --no-default-features --features postgres

# Install with PostgreSQL and SQLite only
cargo install dbdiff --no-default-features --features postgres,sqlite
```

All backends (`postgres`, `mysql`, `sqlite`) are enabled by default.

---

## Development

```bash
git clone https://github.com/rekurt/dbdiff
cd dbdiff
cargo build

# Run tests
cargo test

# Run with local SQL files
cargo run -- tests/fixtures/schema_a.sql --schema tests/fixtures/schema_b.sql

# Run with a local Postgres
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=pass postgres:16
cargo run -- postgres://postgres:pass@localhost/myapp --schema tests/fixtures/schema_b.sql
```

### Project layout

```
src/
  main.rs          CLI entry point
  lib.rs           Library root
  cli.rs           Argument parsing (clap)
  model.rs         Schema / Table / Column / Index structs
  error.rs         Error types
  diff.rs          Schema comparison engine
  migration.rs     SQL migration generator
  output.rs        Terminal rendering (colored diff)
  config/
    mod.rs         Config file parsing (.dbdiff.yml)
    filter.rs      Schema filtering (ignore tables/columns)
  loader/
    mod.rs         Source dispatch logic
    postgres.rs    PostgreSQL introspection
    mysql.rs       MySQL / MariaDB introspection
    sqlite.rs      SQLite introspection
    sqlfile.rs     .sql file parser
tests/
  cli.rs           Integration tests
  config.rs        Config integration tests
  fixtures/        SQL test schemas + config fixtures
```

---

## Contributing

Issues and PRs are welcome. Please open an issue before working on a large change.

For a new database driver, see [`src/loader/postgres.rs`](src/loader/postgres.rs) — implement a `load` function returning `Schema` and add detection logic in `src/loader/mod.rs`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for full details.

---

## License

MIT © [Nikita](https://github.com/rekurt)
