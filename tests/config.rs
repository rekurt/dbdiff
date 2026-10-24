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

#[test]
fn config_ci_format_and_cli_override() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.yml");
    std::fs::write(&config, "output:\n  format: ci\n").unwrap();
    for override_format in [false, true] {
        let mut command = cmd();
        command
            .args([
                "tests/fixtures/schema_a.sql",
                "tests/fixtures/schema_b.sql",
                "--config",
            ])
            .arg(&config);
        if override_format {
            command.args(["--format", "json"]);
            let output = command.assert().success().get_output().stdout.clone();
            assert!(serde_json::from_slice::<serde_json::Value>(&output).is_ok());
        } else {
            command
                .assert()
                .success()
                .stdout(predicate::str::contains("CI summary:"));
        }
    }
}

#[test]
fn explicit_color_mode_overrides_config() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.yml");
    for configured in [false, true] {
        std::fs::write(&config, format!("output:\n  color: {configured}\n")).unwrap();
        for mode in ["always", "never", "auto"] {
            let output = cmd()
                .args([
                    "tests/fixtures/schema_a.sql",
                    "tests/fixtures/schema_b.sql",
                    "--color",
                    mode,
                    "--config",
                ])
                .arg(&config)
                .assert()
                .success()
                .get_output()
                .stdout
                .clone();
            assert_eq!(
                String::from_utf8(output).unwrap().contains("\x1b["),
                mode == "always",
                "config={configured}, mode={mode}"
            );
        }
    }
}
