# Proposed grant work packages

[Back to roadmap](../README.md#roadmap-and-support-needs)

These work packages define reviewable outputs for a grant or in-kind support
proposal. They are **proposed, not funded or completed**. Scope, database
versions, effort, budget, and delivery dates need maintainer agreement before
an application commits to them.

Baseline inspected on 2026-10-27 at
[`49cb8a1`](https://github.com/rekurt/dbdiff/commit/49cb8a119eb3dc4f5f3b22172158adb30f4099d4).
The existing [offline demo](../examples/demo/README.md) already checks four
schema changes, CI exit codes, preview behavior, and generated files. It does
not establish live-backend compatibility.

## DB-1: SQL-file object coverage

**Current baseline.** The [SQL loader](../src/loader/sqlfile.rs) loads tables
and indexes. The [diff command](../src/commands/diff.rs) excludes views, enums,
and sequences from comparisons involving a SQL source.

**Proposed deliverable.** A documented subset of PostgreSQL schema-file
syntax for those objects, parser fixtures, and end-to-end comparison tests.

**Acceptance criteria:**

- Check in synthetic before/after fixtures containing a view, enum, and
  sequence, with exact expected parsed objects and add/change/remove diffs.
- Verify unchanged supported fixtures produce no drift and equivalent
  supported SQL-file/snapshot inputs compare consistently.
- Test unsupported syntax and document its handling; do not imply arbitrary
  SQL or migration-file parsing. Remove exclusions only for covered object
  types and keep remaining limitations explicit.
- Preserve existing table/index fixtures and CLI exit-code behavior.

**Review evidence.** Supported-syntax table, fixtures, focused regression
tests, and machine-readable expected/actual diffs.

## DB-2: Live PostgreSQL loader and snapshot verification

**Current baseline.** The [PostgreSQL loader](../src/loader/postgres.rs) reads
tables, constraints, indexes, views, enums, and sequences from `public`.
[CI](../.github/workflows/ci.yml) includes a MySQL 8.4 service, but its
[live test](../tests/mysql_live.rs) checks pool shutdown, not object fidelity.
There is no corresponding live PostgreSQL regression job in that workflow.

**Proposed deliverable.** A repeatable disposable PostgreSQL fixture,
loader/snapshot integration tests, and a documented local/CI runner.

**Acceptance criteria:**

- Build a synthetic `public` schema containing tables, constraints, indexes,
  a view, an enum, and a sequence; assert their expected names and definitions.
- Export and reload a snapshot without losing those modeled objects.
  Compare an unchanged live schema with its snapshot and assert zero drift.
- Modify the fixture and assert the exact expected drift in both live and
  snapshot comparisons. Exercise a connection failure without exposing a DSN
  credential in the retained report.
- Record server version, enabled features, fixture revision, commands, commit,
  and results. The runner must fail on mismatches and document cleanup.
  Test only disposable databases with synthetic data.

**Review evidence.** Fixture/setup/cleanup instructions, live test source,
snapshot and diff reports, and CI results for each agreed server version.
An ignored live test or a successful offline demo is not a completed live check.

## DB-3: Executable examples of SQLite migration limits

**Current baseline.** The [SQL generator](../src/migration/sql.rs) emits
unsupported-operation comments for constraint changes and warnings for
column type/nullability alterations.

**Proposed deliverable.** Small before/after SQLite examples and assertions
that explain the generated plan's limits. Use SQLite database inputs; SQL-file-only
comparisons generate PostgreSQL-style SQL and do not exercise the SQLite dialect.

**Acceptance criteria:**

- Cover constraint addition/removal and column type/nullability changes.
  Assert the expected warnings or unsupported-operation comments for each.
- Verify preview mode does not write a migration and explicit emission saves
  the expected plan. Document which cases need a separately reviewed table
  rebuild, without presenting comments as an executable migration.
- Provide a repeatable example command and expected output. Keep dbdiff's
  comparison/SQL-generation boundary: it must not apply migrations.

**Review evidence.** Synthetic fixtures, automated assertions, and a concise
walkthrough. No data-preservation or automatic rollback guarantee is implied.

## Verification and application handoff

Run the existing checks alongside each new package's documented runner:

```bash
cargo build --locked
cargo test --locked
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-Dwarnings" cargo doc --locked --no-deps --document-private-items --all-features
python3 examples/demo/run.py --bin target/debug/dbdiff
```

On Windows use `python` and `target/debug/dbdiff.exe`. Record the Rust version
and commit, and distinguish passed, failed, ignored, and unrun checks. These
commands are a verification recipe, not a claim that the proposed work exists.

Packages may be selected independently; DB-2 uses live schemas and JSON
snapshots and does not depend on DB-1. Confirm contributors, resources,
selected versions, and reporting expectations before submission. Do not
claim broader backend compatibility, adoption, or delivery commitments from
these proposed criteria.
