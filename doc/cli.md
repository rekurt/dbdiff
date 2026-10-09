# CLI guide

[Back to README](../README.md) · [Offline demo](../examples/demo/README.md)

## Sources and direction

```text
dbdiff [OPTIONS] SOURCE [TARGET]
dbdiff diff [OPTIONS] SOURCE [TARGET]
dbdiff SOURCE --schema FILE
```

`SOURCE` is the current state; `TARGET` (or `--schema FILE`) is the desired state. The default `up` direction produces SQL to change current → desired. `down` produces the reverse schema plan. `both` includes labeled UP and DOWN sections; apply only the intended section.

| Source | Example |
| --- | --- |
| PostgreSQL | `postgres://user:password@host/database` or `postgresql://…` |
| MySQL / MariaDB | `mysql://user:password@host/database` or `mariadb://…` |
| SQLite | `app.db`, `app.sqlite`, `app.sqlite3`, or `sqlite://…` |
| SQL schema file | `schema.sql` |
| JSON snapshot | `schema.json`, exported by `dbdiff snapshot` |

Use matching live database backends. SQL files or JSON snapshots can be used for offline comparisons. SQL-only plans use PostgreSQL-style DDL. SQL files describe complete schemas with CREATE statements; they are not migration histories. Views, enums, and sequences are excluded from comparisons involving SQL files.

## Output and file writes

| Option | Behavior |
| --- | --- |
| `--format pretty` | Human-readable diff and migration; default |
| `--format json` / `yaml` | Structured drift report |
| `--format sql` | Migration SQL on stdout |
| `--format ci` | Compact CI report |
| `--color auto\|always\|never` | Terminal color control |
| `--explain` | Explain migration statements in pretty output |
| `--out FILE` | Preview the intended file write by default |
| `--out FILE --write` | Save SQL to a file |
| `--emit FILE` | Shortcut for `--out FILE --write` |
| `--dry-run` / `--plan` | Preview without saving migration files |
| `--direction up\|down\|both` | Forward, rollback, or both schema plans |

Shell redirection (`--format sql > migration.sql`) writes through your shell. `--emit` and `--out --write` are dbdiff's explicit write options. Existing output files can be overwritten.

## CI behavior

`--ci` changes exit status: `0` for equality, `1` for drift. Errors return `2`. With `--ci --fail-on-blocking`, blocking changes return `3`. `--fail-on-blocking` requires `--ci`.

```bash
dbdiff current.sql desired.sql --ci --format json > report.json
dbdiff current.sql desired.sql --ci --fail-on-blocking --format ci
```

The report includes equality, a summary, changes, and blocking classifications. Under `GITHUB_ACTIONS=true`, CI mode also emits GitHub annotations. Classification is based on the planned operations; actual locks and runtime depend on the database and its workload.

## Migration options

| Option | Purpose |
| --- | --- |
| `--no-transaction` | Omit BEGIN/COMMIT wrapping in SQL output |
| `--concurrently` | PostgreSQL CREATE INDEX CONCURRENTLY; implies no transaction wrapping |
| `--detect-renames` | Experimental table/column rename inference |
| `--profile dev\|ci\|safe-migrate` | Presets for common workflows; see `dbdiff --help` |

`--force` can insert **TRUNCATE TABLE** when PostgreSQL duplicate data would prevent creation of a unique index. It is deliberately excluded from the demo; inspect the generated SQL and its data-loss implications before using it.

Generated rollback SQL restores definitions, not deleted data. SQLite cannot directly execute some ALTER operations; read warnings and unsupported-operation comments in the generated plan.

## Configuration

```bash
dbdiff init
dbdiff current.sql desired.sql --config custom.yml
```

By default, dbdiff reads `.dbdiff.yml` in the working directory. A missing file uses defaults. A present but invalid file returns an error. CLI format options override configured output format.

```yaml
ignore:
  tables: [_migrations, schema_version]
  columns: ["*.created_at", "sessions.*", "users.internal_note"]
protected:
  tables: [payments]
  columns: ["*.id", "users.email"]
output:
  format: pretty
  color: true
```

- `ignore.tables`: exact table names; filtered from both sources.
- `ignore.columns`: `*.column`, `table.*`, or `table.column` patterns.
- `protected.tables`: reject drops of the listed tables.
- `protected.columns`: reject drops of matching columns, including when their table is dropped. Protection is checked for the requested migration direction.
- `output.color: false`: disable colored output.

## Inspect and snapshot

```bash
# Load a schema and report connectivity / schema counts
dbdiff validate "$DATABASE_DSN"

# List tables
dbdiff tables "$DATABASE_DSN"

# Capture schema metadata for an offline comparison
dbdiff snapshot "$DATABASE_DSN" --out schema.json

# Compare two exported snapshots
dbdiff current.json desired.json

# Generate shell completions
dbdiff completions bash > dbdiff.bash
```

Snapshots contain schema metadata, not table data. These commands can also load supported file sources. `validate`, `tables`, and `snapshot` support `--timeout SECONDS` and `--ssl-mode disable|prefer|require`. PostgreSQL honors the SSL mode; do not assume identical TLS behavior in other backends.

## Help

```bash
dbdiff --help
dbdiff diff --help
dbdiff snapshot --help
```

[Architecture](architecture.md) explains the loaders, comparison engine, and SQL generator.
