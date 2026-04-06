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
        .stdout(predicate::str::contains("\"has_changes\": true"))
        .stdout(predicate::str::contains("\"diff\""));
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
fn out_flag_writes_migration_file() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("migration.sql");

    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--out",
            out_path.to_str().unwrap(),
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
