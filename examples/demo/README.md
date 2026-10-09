# Offline demo

Compare a small application's current schema with its desired schema, generate reviewable SQL, and check CI behavior. No database server or credentials required.

## Run the complete demo

From the repository root, with Rust 1.88+ and Python 3.9+:

```bash
cargo build --locked
python3 examples/demo/run.py --bin target/debug/dbdiff
```

For an installed binary, use `python3 examples/demo/run.py`. On Windows, use `python` and `--bin target/debug/dbdiff.exe`.

The runner uses only Python's standard library. It isolates configuration and temporary files, verifies the results, and writes reusable artifacts to the ignored `target/demo/` directory. An unexpected result makes the runner fail.

## The scenario

| Object | Before | After |
| --- | --- | --- |
| `orders.payment_date` | Free-form text | Removed |
| `orders.paid_at` | Absent | Nullable timestamp |
| `idx_orders_paid_at` | Absent | Index on `orders(paid_at)` |
| `users.deleted_at` | Absent | Nullable timestamp for soft deletion |

The drop is deliberate: it demonstrates a destructive-operation warning. A real rollout should backfill the new timestamp before removing the old field.

## Explore each step

Run from the repository root with `dbdiff` on your `PATH` (or replace it with `target/debug/dbdiff`):

```bash
# Current → desired: inspect the diff and generated SQL
dbdiff examples/demo/before.sql examples/demo/after.sql

# Save a machine-readable report
dbdiff examples/demo/before.sql examples/demo/after.sql --format json

# Preview: migration.sql is not written
dbdiff examples/demo/before.sql examples/demo/after.sql --out migration.sql

# Explicitly save migration SQL
dbdiff examples/demo/before.sql examples/demo/after.sql --emit migration.sql

# Generate a separate rollback plan
dbdiff examples/demo/before.sql examples/demo/after.sql \
  --direction down --emit rollback.sql

# Drift: expected exit 1
dbdiff examples/demo/before.sql examples/demo/after.sql --ci --format ci

# Blocking operations: expected exit 3
dbdiff examples/demo/before.sql examples/demo/after.sql \
  --ci --fail-on-blocking --format ci

# Matching schemas: expected exit 0
dbdiff examples/demo/after.sql examples/demo/after.sql --ci --format ci
```

The runner treats the expected nonzero CI codes as successful demo checks. It also verifies all four JSON changes, preview behavior, actual file emission, transaction wrapping, and a rollback statement. It never applies migrations to a database.

## Generated artifacts

| File in `target/demo/` | Contents |
| --- | --- |
| `transcript.txt` | Actual CLI output and CI exit codes |
| `report.json` | Structured drift report |
| `migration.sql` | Forward migration |
| `rollback.sql` | Schema rollback plan; does not restore lost data |

## Regenerate the README animation

After running the demo:

```bash
python3 -m venv target/demo-venv
# macOS / Linux (Windows: target/demo-venv/Scripts/python)
target/demo-venv/bin/python -m pip install Pillow==12.3.0
target/demo-venv/bin/python scripts/render-demo.py
```

The renderer reads the real transcript, selects diff/SQL/CI excerpts, and produces `doc/assets/demo.gif` plus a static `doc/assets/demo.png`. It does not invent CLI output. Use `--font /path/to/monospace.ttf` if a system monospace font cannot be found. Pillow is needed only to regenerate the visuals, not to run the demo or dbdiff.
