use std::process::ExitCode;

use dbdiff::cli::SnapshotArgs;
use dbdiff::loader;

use super::helpers::{create_spinner, resolve_ssl_mode};

pub async fn run_snapshot(args: SnapshotArgs) -> Result<(), ExitCode> {
    let spinner = create_spinner("Loading schema...");
    let ssl_mode = resolve_ssl_mode(args.ssl_mode);
    let timeout_duration = std::time::Duration::from_secs(args.timeout);

    let result = tokio::time::timeout(
        timeout_duration,
        loader::load_schema_with_ssl(&args.dsn, ssl_mode),
    )
    .await;

    spinner.finish_and_clear();

    match result {
        Ok(Ok(loaded)) => {
            let snapshot = dbdiff::model::SchemaSnapshot::from(&loaded.schema);
            let json = serde_json::to_string_pretty(&snapshot).map_err(|e| {
                eprintln!("Error: failed to serialize snapshot as JSON: {e}");
                ExitCode::from(2)
            })?;

            if let Some(ref path) = args.out {
                std::fs::write(path, &json).map_err(|e| {
                    eprintln!("Error writing snapshot: {e}");
                    ExitCode::from(2)
                })?;
                eprintln!("Snapshot written to {path}");
            } else {
                println!("{json}");
            }
            Ok(())
        }
        Ok(Err(e)) => {
            eprintln!("Error: {e}");
            Err(ExitCode::from(2))
        }
        Err(_) => {
            let err = dbdiff::error::DbDiffError::timeout(&args.dsn, args.timeout);
            eprintln!("Error: {err}");
            Err(ExitCode::from(2))
        }
    }
}
