use std::process::ExitCode;

use clap::Parser;

use dbdiff::cli::{Args, OutputFormat};
use dbdiff::diff::diff_schemas;
use dbdiff::loader;
use dbdiff::migration::generate_migration;
use dbdiff::output;

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    if let Err(code) = run(args).await {
        return code;
    }

    ExitCode::SUCCESS
}

async fn run(args: Args) -> Result<(), ExitCode> {
    let target_source = args.target_source().map_err(|msg| {
        eprintln!("Error: {msg}");
        ExitCode::from(2)
    })?;

    let (left, right) = tokio::try_join!(
        loader::load_schema(&args.source),
        loader::load_schema(target_source),
    )
    .map_err(|e| {
        eprintln!("Error: {e}");
        ExitCode::from(2)
    })?;

    let diff = diff_schemas(&left, &right);
    let statements = generate_migration(&diff);

    match args.format {
        OutputFormat::Pretty => {
            output::print_diff(&diff);
            if !statements.is_empty() {
                println!();
                output::print_migration(&statements);
            }
        }
        OutputFormat::Json => {
            let json = serde_json::json!({
                "diff": diff,
                "migration": statements.iter().map(|s| &s.sql).collect::<Vec<_>>(),
                "warnings": statements.iter()
                    .flat_map(|s| s.warnings.iter())
                    .collect::<Vec<_>>(),
                "has_changes": !diff.is_empty(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
        }
        OutputFormat::Sql => {
            print!("{}", output::migration_to_sql(&statements));
        }
    }

    // Write migration file
    if let Some(ref path) = args.out {
        if !args.dry_run {
            let sql = output::migration_to_sql(&statements);
            std::fs::write(path, sql).map_err(|e| {
                eprintln!("Error writing migration file: {e}");
                ExitCode::from(2)
            })?;
            eprintln!("Migration written to {path}");
        } else {
            eprintln!("Dry run: would write migration to {path}");
        }
    }

    // CI mode: exit 1 if schemas differ
    if args.ci && !diff.is_empty() {
        return Err(ExitCode::from(1));
    }

    Ok(())
}
