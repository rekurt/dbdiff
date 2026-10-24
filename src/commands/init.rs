use std::io::{ErrorKind, Write};
use std::process::ExitCode;

use dbdiff::config;

pub fn run_init() -> Result<(), ExitCode> {
    let path = ".dbdiff.yml";
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == ErrorKind::AlreadyExists {
                eprintln!("Config file '{path}' already exists. Remove it first to regenerate.");
            } else {
                eprintln!("Error writing config file: {e}");
            }
            ExitCode::from(2)
        })?;

    file.write_all(config::DEFAULT_CONFIG_TEMPLATE.as_bytes())
        .map_err(|e| {
            eprintln!("Error writing config file: {e}");
            ExitCode::from(2)
        })?;

    eprintln!("Created {path}");
    Ok(())
}
