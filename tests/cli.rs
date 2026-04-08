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
        .stdout(predicate::str::contains("\"changes\""))
        .stdout(predicate::str::contains("\"is_blocking\""))
        .stdout(predicate::str::contains("\"summary\""));
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

// ─── CI Integration tests ───────────────────────────────────────

#[test]
fn ci_mode_drift_exits_1() {
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
fn ci_mode_identical_exits_0() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_a.sql",
            "--ci",
        ])
        .assert()
        .success()
        .code(0);
}

#[test]
fn ci_fail_on_blocking_exits_3() {
    // schema_b has NOT NULL column added (blocking) and new index (blocking)
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
    // Create a scenario with only non-blocking changes
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

    // Adding a nullable column is NOT blocking → exit 1 (drift), not 3 (unsafe)
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
        .stdout(predicate::str::contains("equal: false"))
        .stdout(predicate::str::contains("is_blocking:"))
        .stdout(predicate::str::contains("changes:"));
}

#[test]
fn json_output_has_blocking_array() {
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
        .stdout(predicate::str::contains("\"blocking\""))
        .stdout(predicate::str::contains("\"is_blocking\": true"));
}

#[test]
fn json_output_identical_schemas() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_a.sql",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"equal\": true"))
        .stdout(predicate::str::contains("Schemas are identical"));
}

#[test]
fn github_annotations_emitted_in_ci_mode() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
            "--ci",
        ])
        .env("GITHUB_ACTIONS", "true")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "::error title=Blocking migration::",
        ))
        .stderr(predicate::str::contains("::warning title=Schema drift::"));
}

#[test]
fn github_annotations_not_emitted_without_ci_flag() {
    cmd()
        .args([
            "tests/fixtures/schema_a.sql",
            "--schema",
            "tests/fixtures/schema_b.sql",
        ])
        .env("GITHUB_ACTIONS", "true")
        .assert()
        .success()
        .stderr(predicate::str::contains("::error").not())
        .stderr(predicate::str::contains("::warning").not());
}

#[test]
fn error_exit_code_is_2() {
    cmd()
        .args([
            "nonexistent_db_connection",
            "--schema",
            "tests/fixtures/schema_a.sql",
        ])
        .assert()
        .failure()
        .code(2);
}
