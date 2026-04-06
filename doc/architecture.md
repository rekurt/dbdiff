# Architecture

This document describes the high-level architecture of dbdiff.

## Overview

```
┌─────────┐     ┌──────────┐     ┌──────────┐     ┌────────────┐     ┌──────────┐
│  CLI     │────▶│ Loaders  │────▶│  Diff    │────▶│ Migration  │────▶│ Output   │
│ (clap)   │     │          │     │  Engine  │     │ Generator  │     │          │
└─────────┘     └──────────┘     └──────────┘     └────────────┘     └──────────┘
                 │        │
           ┌─────┘        └─────┐
           ▼                    ▼
    ┌────────────┐      ┌────────────┐
    │ PostgreSQL │      │ SQL File   │
    │ Loader     │      │ Parser     │
    └────────────┘      └────────────┘
```

## Module Responsibilities

### `cli.rs`
Defines the command-line interface using `clap`. Handles argument parsing and validation.

### `model.rs`
Core data structures: `Schema`, `Table`, `Column`, `Index`. These are the canonical representation that all other modules work with. Uses `BTreeMap` for deterministic ordering.

### `loader/`
Schema loading from different sources. Each loader converts a source into a `Schema`:
- **`postgres.rs`** — Connects to a live PostgreSQL database and queries `information_schema` and `pg_indexes`
- **`sqlfile.rs`** — Parses `.sql` files containing `CREATE TABLE` and `CREATE INDEX` statements
- **`mod.rs`** — Dispatch logic that routes to the correct loader based on the source string

### `diff.rs`
Pure function `diff_schemas(left, right) -> SchemaDiff`. No I/O, no side effects. Compares two schemas and produces a structured diff with added/removed/modified tables, columns, and indexes.

### `migration.rs`
Takes a `SchemaDiff` and generates ordered SQL statements. Handles statement ordering for safe execution (drops before creates, indexes after columns). Includes safety warnings for dangerous operations.

### `output.rs`
Terminal rendering with colored output. Also handles JSON and plain SQL output formats.

### `error.rs`
Unified error type `DbDiffError` with variants for each error source.

## Data Flow

1. CLI parses arguments → determines source and target
2. Loaders convert sources into `Schema` structs
3. Diff engine compares the two schemas
4. Migration generator produces SQL statements from the diff
5. Output module renders results to terminal or file

## Adding a New Database

1. Create `src/loader/yourdb.rs` with a `pub async fn load(dsn: &str) -> Result<Schema, DbDiffError>`
2. Add DSN pattern detection in `src/loader/mod.rs`
3. The diff engine, migration generator, and output modules work unchanged — they operate on the abstract `Schema` model
