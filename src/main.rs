use std::process::ExitCode;

use clap::Parser;

use dbdiff::ci::{self, CiReport};
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

    match run(args).await {
        Ok(code) => ExitCode::from(code),
        Err(code) => ExitCode::from(code),
    }
}

async fn run(args: Args) -> Result<u8, u8> {
    let target_source = args.target_source().map_err(|msg| {
        eprintln!("Error: {msg}");
        ci::EXIT_ERROR
    })?;

    let cfg = config::load_config(&args.config).map_err(|e| {
        eprintln!("Error: {e}");
        ci::EXIT_ERROR
    })?;

    let (mut left, mut right) = tokio::try_join!(
        loader::load_schema(&args.source),
        loader::load_schema(target_source),
    )
    .map_err(|e| {
        eprintln!("Error: {e}");
        ci::EXIT_ERROR
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
            return Err(ci::EXIT_ERROR);
        }
    };
    let statements = generate_migration(&diff, migration_dialect);
    let format = args.resolve_format(&cfg.output.format);

    // Build CI report (used for JSON/YAML output and exit code logic)
    let report = CiReport::from_diff(&diff, &statements);

    match format {
        OutputFormat::Pretty => {
            output::print_diff(&diff);
            if !statements.is_empty() {
                println!();
                output::print_migration(&statements);
            }
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report).unwrap_or_default();
            println!("{json}");
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&report).unwrap_or_default();
            print!("{yaml}");
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
                ci::EXIT_ERROR
            })?;
            eprintln!("Migration written to {path}");
        } else {
            eprintln!("Dry run: would write migration to {path}");
        }
    }

    // GitHub Actions annotations (auto-detect from env)
    if args.ci && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        report.emit_github_annotations();
    }

    // CI mode: determine exit code from report
    if args.ci {
        let code = report.exit_code(args.fail_on_blocking);
        if code != ci::EXIT_OK {
            return Err(code);
        }
    }

    Ok(ci::EXIT_OK)
}
