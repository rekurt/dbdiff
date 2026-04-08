use std::process::ExitCode;

use dbdiff::cli::TablesArgs;
use dbdiff::loader;

use super::helpers::{create_spinner, resolve_ssl_mode};

pub async fn run_tables(args: TablesArgs) -> Result<(), ExitCode> {
    let spinner = create_spinner("Loading tables...");
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
            if loaded.schema.tables.is_empty() {
                println!("No tables found.");
                return Ok(());
            }

            println!("{:<30} {:>8} {:>8}", "TABLE", "COLUMNS", "INDEXES");
            println!("{}", "-".repeat(48));

            for (name, table) in &loaded.schema.tables {
                println!(
                    "{:<30} {:>8} {:>8}",
                    name,
                    table.columns.len(),
                    table.indexes.len()
                );
            }

            println!("{}", "-".repeat(48));
            println!("{} table(s) total", loaded.schema.tables.len());
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
