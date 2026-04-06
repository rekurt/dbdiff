use assert_cmd::Command;
use predicates::prelude::*;

fn cmd() -> Command {
    Command::cargo_bin("dbdiff").unwrap()
}

#[test]
fn config_ignores_table() {
    // The fixture dbdiff.yml ignores audit_log table.
    // schema_b.sql adds audit_log, so without config it would show up in diff.
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--config",
            "tests/fixtures/dbdiff.yml",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("audit_log").not());
}

#[test]
fn config_ignores_column() {
    // The fixture dbdiff.yml ignores *.created_at columns.
    // Both schemas have created_at columns that should be filtered out.
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--config",
            "tests/fixtures/dbdiff.yml",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("created_at").not());
}

#[test]
fn missing_config_file_uses_defaults() {
    // Non-existent config file should not cause an error — just use defaults.
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--config",
            "/nonexistent/path/.dbdiff.yml",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("audit_log"));
}
