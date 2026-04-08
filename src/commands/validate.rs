use std::process::ExitCode;
use std::time::Instant;

use dbdiff::cli::ValidateArgs;
use dbdiff::loader;

use super::helpers::{create_spinner, resolve_ssl_mode};

pub async fn run_validate(args: ValidateArgs) -> Result<(), ExitCode> {
    let spinner = create_spinner("Connecting...");
    let start = Instant::now();

    let timeout_duration = std::time::Duration::from_secs(args.timeout);
    let ssl_mode = resolve_ssl_mode(args.ssl_mode);

    let result = tokio::time::timeout(
        timeout_duration,
        loader::load_schema_with_ssl(&args.dsn, ssl_mode),
    )
    .await;

    let latency = start.elapsed();
    spinner.finish_and_clear();

    match result {
        Ok(Ok(loaded)) => {
            let table_count = loaded.schema.tables.len();
            let column_count: usize = loaded.schema.tables.values().map(|t| t.columns.len()).sum();
            let index_count: usize = loaded.schema.tables.values().map(|t| t.indexes.len()).sum();

            use colored::Colorize;
            println!(
                "{}",
                format!("  Connected to {:?} database", loaded.dialect).green()
            );
            println!("  Tables:  {table_count}");
            println!("  Columns: {column_count}");
            println!("  Indexes: {index_count}");
            println!("  Latency: {}ms", latency.as_millis());
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
