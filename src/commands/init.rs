use std::process::ExitCode;

use dbdiff::config;

pub fn run_init() -> Result<(), ExitCode> {
    let path = ".dbdiff.yml";
    if std::path::Path::new(path).exists() {
        eprintln!("Config file '{path}' already exists. Remove it first to regenerate.");
        return Err(ExitCode::from(2));
    }

    std::fs::write(path, config::DEFAULT_CONFIG_TEMPLATE).map_err(|e| {
        eprintln!("Error writing config file: {e}");
        ExitCode::from(2)
    })?;

    eprintln!("Created {path}");
    Ok(())
}
