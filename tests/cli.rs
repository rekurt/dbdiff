use assert_cmd::Command;
use predicates::prelude::*;

fn cmd() -> Command {
    Command::cargo_bin("dbdiff").unwrap()
}

#[test]
fn no_args_shows_error() {
    cmd()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn help_flag_works() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Compare database schemas"));
}

#[test]
fn version_flag_works() {
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("dbdiff"));
}

#[test]
fn compare_two_sql_files_shows_diff() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("table:"))
        .stdout(predicate::str::contains("column"));
}

#[test]
fn compare_two_sql_files_ci_mode_detects_drift() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--ci",
        ])
        .assert()
        .code(1);
}

#[test]
fn compare_identical_files_ci_mode_succeeds() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_a.sql",
            "--ci",
        ])
        .assert()
        .success();
}

#[test]
fn json_output_format() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"equal\": false"))
        .stdout(predicate::str::contains("\"changes\""));
}

#[test]
fn sql_output_format() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "sql",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ALTER TABLE"));
}

#[test]
fn dry_run_does_not_write_file() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("migration.sql");

    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--out",
            out_path.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success();

    assert!(!out_path.exists());
}

#[test]
fn safe_by_default_does_not_write_without_write_flag() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("migration.sql");

    // --out without --write should NOT create the file (safe by default)
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--out",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Dry run"));

    assert!(!out_path.exists());
}

#[test]
fn out_flag_with_write_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("migration.sql");

    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--out",
            out_path.to_str().unwrap(),
            "--write",
        ])
        .assert()
        .success();

    assert!(out_path.exists());
    let content = std::fs::read_to_string(&out_path).unwrap();
    assert!(content.contains("ALTER TABLE"));
}

#[test]
fn invalid_source_returns_error() {
    cmd()
        .args([
            "nonexistent_file.txt",
            "--schema",
            "tests/fixtures/schema_a.sql",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn diff_subcommand_works() {
    cmd()
        .args([
            "diff",
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("table:"));
}

#[test]
fn completions_subcommand_works() {
    cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dbdiff"));
}

#[test]
fn summary_shown_in_pretty_output() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Summary:"));
}

#[test]
fn summary_in_json_output() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"is_blocking\""));
}

#[test]
fn timeout_flag_accepted() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--timeout",
            "5",
        ])
        .assert()
        .success();
}

#[test]
fn direction_up_flag_works() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "sql",
            "--direction",
            "up",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ALTER TABLE"));
}

#[test]
fn direction_down_generates_rollback() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "sql",
            "--direction",
            "down",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("DROP").or(predicate::str::contains("ADD COLUMN")));
}

#[test]
fn direction_both_generates_up_and_down() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "sql",
            "--direction",
            "both",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("-- === UP ==="))
        .stdout(predicate::str::contains("-- === DOWN ==="));
}

#[test]
fn json_output_includes_changes() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changes\""));
}

#[test]
fn init_creates_config_file() {
    let dir = tempfile::tempdir().unwrap();
    cmd().current_dir(dir.path()).arg("init").assert().success();

    let config_path = dir.path().join(".dbdiff.yml");
    assert!(config_path.exists());
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("ignore:"));
    assert!(content.contains("protected:"));
}

#[test]
fn init_refuses_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".dbdiff.yml"), "existing").unwrap();
    cmd()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn snapshot_from_sql_file_to_json() {
    // Snapshot a SQL file schema to JSON, then use that JSON as a source for diff
    let dir = tempfile::tempdir().unwrap();
    let snap_path = dir.path().join("schema.json");

    // First, create a snapshot from a SQL file
    // (snapshot command needs a DSN, but we can test JSON round-trip by creating one manually)
    let schema_sql = std::fs::read_to_string("tests/fixtures/schema_a.sql").unwrap();

    // Use diff with sql file as source and verify json output has the right structure
    let output = cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_a.sql",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"equal\": true"));

    // Verify that a JSON snapshot file can be used as a diff source
    // Create a minimal snapshot JSON
    let snap_json = r#"{
        "tables": {
            "test_table": {
                "name": "test_table",
                "columns": {
                    "id": {"name": "id", "data_type": "integer", "is_nullable": false, "default": null}
                },
                "indexes": {},
                "constraints": {}
            }
        },
        "views": {},
        "enums": {},
        "sequences": {}
    }"#;
    std::fs::write(&snap_path, snap_json).unwrap();

    // Use JSON snapshot as a diff source
    cmd()
        .args([
            snap_path.to_str().unwrap(),
            "--schema",
            "tests/fixtures/schema_a.sql",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"equal\""));

    // Verify _ is not needed
    drop(schema_sql);
}

#[test]
fn tables_subcommand_with_invalid_source() {
    // tables with an unreachable DSN should fail gracefully
    cmd()
        .args([
            "tables",
            "postgres://invalid:invalid@localhost:1/noexist",
            "--timeout",
            "1",
        ])
        .assert()
        .failure()
        .code(2);
}

// === CI integration tests (from master) ===

#[test]
fn ci_fail_on_blocking_exits_3() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--ci",
            "--fail-on-blocking",
        ])
        .assert()
        .code(3);
}

#[test]
fn ci_fail_on_blocking_no_blocking_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let schema_left = dir.path().join("left.sql");
    std::fs::write(
        &schema_left,
        "CREATE TABLE users (id serial NOT NULL, email text NOT NULL);",
    )
    .unwrap();
    let schema_right = dir.path().join("right.sql");
    std::fs::write(
        &schema_right,
        "CREATE TABLE users (id serial NOT NULL, email text NOT NULL, bio text);",
    )
    .unwrap();
    cmd()
        .args([
            schema_left.to_str().unwrap(),
            "--schema",
            schema_right.to_str().unwrap(),
            "--ci",
            "--fail-on-blocking",
        ])
        .assert()
        .code(1);
}

#[test]
fn yaml_output_format() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "yaml",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("equal: false"));
}

#[test]
fn emit_flag_writes_file_without_write() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("migration_emit.sql");
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--emit",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(out_path.exists());
}

#[test]
fn plan_flag_keeps_plan_mode() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("migration_plan.sql");
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--emit",
            out_path.to_str().unwrap(),
            "--plan",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Dry run"));
    assert!(!out_path.exists());
}

#[test]
fn fail_on_blocking_requires_ci() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--fail-on-blocking",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("requires --ci"));
}

#[test]
fn ci_compact_output_format() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--format",
            "ci",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("CI summary:"));
}

#[test]
fn safe_migrate_enables_ci_without_writing_files() {
    let schemas = tempfile::tempdir().unwrap();
    let nonblocking = schemas.path().join("nonblocking.sql");
    let mut sql = std::fs::read_to_string("tests/fixtures/schema_a.sql").unwrap();
    sql.push_str("\nCREATE TABLE audit_log (id integer);\n");
    std::fs::write(&nonblocking, sql).unwrap();
    for subcommand in [false, true] {
        for (target, expected_code) in [
            ("tests/fixtures/schema_a.sql", 0),
            ("tests/fixtures/schema_b.sql", 3),
            (nonblocking.to_str().unwrap(), 1),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let out = dir.path().join("migration.sql");
            std::fs::write(&out, "keep existing migration").unwrap();
            let mut command = cmd();
            if subcommand {
                command.arg("diff");
            }
            command
                .args([
                    "tests/fixtures/schema_a.sql",
                    target,
                    "--profile",
                    "safe-migrate",
                    "--emit",
                ])
                .arg(&out)
                .assert()
                .code(expected_code)
                .stderr(predicate::str::contains("requires --ci").not());
            assert_eq!(
                std::fs::read_to_string(out).unwrap(),
                "keep existing migration"
            );
        }
    }
}

#[test]
fn invalid_ci_flags_do_not_echo_dsn_credentials() {
    for subcommand in [false, true] {
        let mut command = cmd();
        if subcommand {
            command.arg("diff");
        }
        command
            .args([
                "postgres://alice:source-secret@localhost/source",
                "mysql://bob:target-secret@localhost/target",
                "--fail-on-blocking",
            ])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("requires --ci"))
            .stderr(predicate::str::contains("source-secret").not())
            .stderr(predicate::str::contains("target-secret").not());
    }
}

#[test]
fn next_steps_do_not_echo_source_paths() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("private-source.sql");
    let target = dir.path().join("private-target.sql");
    for path in [&source, &target] {
        std::fs::write(path, "CREATE TABLE users (id integer);").unwrap();
    }
    cmd()
        .arg(source)
        .arg(target)
        .assert()
        .success()
        .stderr(predicate::str::contains("dbdiff <SOURCE> <TARGET> --ci"))
        .stderr(predicate::str::contains("private-source").not())
        .stderr(predicate::str::contains("private-target").not());
}

#[test]
fn target_and_schema_are_mutually_exclusive() {
    for subcommand in [false, true] {
        let mut command = cmd();
        if subcommand {
            command.arg("diff");
        }
        command
            .args([
                "missing-source.sql",
                "missing-target.sql",
                "--schema",
                "missing-schema.sql",
            ])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("cannot be used with"))
            .stderr(predicate::str::contains("--schema"))
            .stdout(predicate::str::is_empty());
    }
}

#[test]
fn either_target_form_works_with_both_cli_forms() {
    for subcommand in [false, true] {
        for schema_flag in [false, true] {
            let mut command = cmd();
            if subcommand {
                command.arg("diff");
            }
            command.arg("tests/fixtures/schema_a.sql");
            if schema_flag {
                command.arg("--schema");
            }
            command
                .args(["tests/fixtures/schema_a.sql", "--ci"])
                .assert()
                .success();
        }
    }
}
