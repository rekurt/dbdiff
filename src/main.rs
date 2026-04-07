use std::process::ExitCode;

use clap::Parser;

use dbdiff::cli::{Args, OutputFormat};
use dbdiff::config;
use dbdiff::diff::diff_schemas;
use dbdiff::loader;
use dbdiff::loader::SqlDialect;
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

    let cfg = config::load_config(&args.config).map_err(|e| {
        eprintln!("Error: {e}");
        ExitCode::from(2)
    })?;

    let (mut left, mut right) = tokio::try_join!(
        loader::load_schema(&args.source),
        loader::load_schema(target_source),
    )
    .map_err(|e| {
        eprintln!("Error: {e}");
        ExitCode::from(2)
    })?;

    config::filter::apply_ignore(&mut left.schema, &cfg.ignore);
    config::filter::apply_ignore(&mut right.schema, &cfg.ignore);

    // Apply config color setting (CLI could override later with --color flag)
    if let Some(false) = cfg.output.color {
        colored::control::set_override(false);
    }

    let diff = diff_schemas(&left.schema, &right.schema);
    let migration_dialect = match (left.dialect, right.dialect) {
        (SqlDialect::SqlFile, other) | (other, SqlDialect::SqlFile) => other,
        (l, r) if l == r => l,
        (l, r) => {
            eprintln!(
                "Error: Cannot generate migration for mixed backends ({l:?} vs {r:?}). \
                 Compare like-for-like backends or use a .sql file for one side."
            );
            return Err(ExitCode::from(2));
        }
    };
    let statements = generate_migration(&diff, migration_dialect);
    let format = args.resolve_format(&cfg.output.format);

    match format {
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
