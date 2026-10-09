#!/usr/bin/env python3
"""Run the offline demo and verify its output with Python's standard library."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BEFORE = "examples/demo/before.sql"
AFTER = "examples/demo/after.sql"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin", default="dbdiff", help="dbdiff executable (default: PATH)")
    args = parser.parse_args()
    binary = shutil.which(args.bin)
    if binary is None:
        parser.error("dbdiff not found; run cargo build --locked and pass --bin target/debug/dbdiff")
    env = {**os.environ, "NO_COLOR": "1", "GITHUB_ACTIONS": "false"}
    transcript = []
    # An explicit missing config keeps local .dbdiff.yml settings out of the demo.
    with tempfile.TemporaryDirectory(prefix="dbdiff-demo-") as tmp:
        config = str(Path(tmp) / "no-config.yml")

        def run(arguments, expected=0, visible=True):
            command = [binary, *arguments, "--color", "never", "--config", config]
            result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True, encoding="utf-8")
            if result.returncode != expected:
                raise RuntimeError(
                    f"{' '.join(arguments)}: expected exit {expected}, got {result.returncode}\n"
                    f"{result.stdout}{result.stderr}"
                )
            if visible:
                block = "$ dbdiff " + " ".join(arguments) + "\n" + result.stdout + result.stderr
                transcript.append(block.rstrip())
                print(block, end="" if block.endswith("\n") else "\n", flush=True)
                if "--ci" in arguments:
                    line = f"exit code: {result.returncode}"
                    transcript.append(line)
                    print(line, flush=True)
                print(flush=True)
            return result

        run([BEFORE, AFTER])
        report = json.loads(run([BEFORE, AFTER, "--format", "json"], visible=False).stdout)
        expected_changes = {
            ("ADD", "COLUMN", "orders", "paid_at"),
            ("DROP", "COLUMN", "orders", "payment_date"),
            ("ADD", "COLUMN", "users", "deleted_at"),
            ("ADD", "INDEX", "orders", "idx_orders_paid_at"),
        }
        actual = {(c["type"], c["object"], c["table"], c["name"]) for c in report["changes"]}
        if report["equal"] or actual != expected_changes:
            raise RuntimeError(f"Unexpected demo changes: {report}")
        run([BEFORE, AFTER, "--ci", "--format", "ci"], expected=1)
        run([BEFORE, AFTER, "--ci", "--fail-on-blocking", "--format", "ci"], expected=3)
        run([AFTER, AFTER, "--ci", "--format", "ci"])

        preview = Path(tmp) / "preview.sql"
        run([BEFORE, AFTER, "--out", str(preview)], visible=False)
        if preview.exists():
            raise RuntimeError("--out unexpectedly wrote a file without --write")
        migration = Path(tmp) / "migration.sql"
        run([BEFORE, AFTER, "--emit", str(migration), "--format", "sql"], visible=False)
        up = migration.read_text(encoding="utf-8")
        down = run([BEFORE, AFTER, "--direction", "down", "--format", "sql"], visible=False).stdout
        if "BEGIN;" not in up or "ADD COLUMN payment_date" not in down:
            raise RuntimeError("Missing transaction wrapper or rollback statement")

        output = ROOT / "target" / "demo"
        output.mkdir(parents=True, exist_ok=True)
        (output / "transcript.txt").write_text("\n\n".join(transcript) + "\n", encoding="utf-8")
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        (output / "migration.sql").write_text(up, encoding="utf-8")
        (output / "rollback.sql").write_text(down, encoding="utf-8")
        print("Demo verified: 4 changes; CI exits 1 / 3 / 0; preview writes nothing.")
        print("Artifacts: target/demo/{transcript.txt,report.json,migration.sql,rollback.sql}")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        print(f"Demo failed: {error}", file=sys.stderr)
        sys.exit(1)
