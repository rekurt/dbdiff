use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

use dbdiff::cli::{
    Cli, DiffParams, MigrationDirection, OutputFormat, ResolvedCommand, SslMode, TablesArgs,
    ValidateArgs,
};
use dbdiff::config;
use dbdiff::diff::diff_schemas;
use dbdiff::loader;
use dbdiff::loader::SqlDialect;
use dbdiff::migration;
use dbdiff::output;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let resolved = cli.resolve();

    let result = match resolved {
        ResolvedCommand::Diff(params) => run_diff(params).await,
        ResolvedCommand::Validate(args) => run_validate(args).await,
        ResolvedCommand::Tables(args) => run_tables(args).await,
        ResolvedCommand::Completions(args) => {
            Cli::generate_completions(args.shell);
            Ok(())
        }
        ResolvedCommand::Init => run_init(),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

async fn run_diff(params: DiffParams) -> Result<(), ExitCode> {
    if params.target_source.is_empty() {
        eprintln!("Error: Either a target DSN or --schema <file> is required");
        return Err(ExitCode::from(2));
    }

    let cfg = config::load_config(&params.config).map_err(|e| {
        eprintln!("Error: {e}");
        ExitCode::from(2)
    })?;

    let spinner = create_spinner("Loading schemas...");

    let timeout_duration = std::time::Duration::from_secs(params.timeout);
    let ssl_mode = resolve_ssl_mode(params.ssl_mode);

    let load_result = tokio::time::timeout(timeout_duration, async {
        tokio::try_join!(
            loader::load_schema_with_ssl(&params.source, ssl_mode),
            loader::load_schema_with_ssl(&params.target_source, ssl_mode),
        )
    })
    .await;

    spinner.finish_and_clear();

    let (mut left, mut right) = match load_result {
        Ok(Ok(schemas)) => schemas,
        Ok(Err(e)) => {
            eprintln!("Error: {e}");
            return Err(ExitCode::from(2));
        }
        Err(_) => {
            let err = dbdiff::error::DbDiffError::timeout(
                &format!("{} / {}", params.source, params.target_source),
                params.timeout,
            );
            eprintln!("Error: {err}");
            return Err(ExitCode::from(2));
        }
    };

    config::filter::apply_ignore(&mut left.schema, &cfg.ignore);
    config::filter::apply_ignore(&mut right.schema, &cfg.ignore);

    if let Some(false) = cfg.output.color {
        colored::control::set_override(false);
    }

    let diff = diff_schemas(&left.schema, &right.schema);

    // Check protected objects
    if let Err(msg) = check_protected(&diff, &cfg.protected) {
        eprintln!("Error: {msg}");
        return Err(ExitCode::from(2));
    }

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

    let up_statements = migration::generate_migration(&diff, migration_dialect);
    let down_statements = migration::generate_rollback(&diff, migration_dialect);
    let format = params.resolve_format(&cfg.output.format);

    match format {
        OutputFormat::Pretty => {
            output::print_diff(&diff);

            match params.direction {
                MigrationDirection::Up => {
                    if !up_statements.is_empty() {
                        println!();
                        output::print_migration(&up_statements);
                    }
                }
                MigrationDirection::Down => {
                    if !down_statements.is_empty() {
                        println!();
                        println!(
                            "{}",
                            colored::Colorize::bold(colored::Colorize::underline(
                                "Rollback migration"
                            ))
                        );
                        println!();
                        output::print_migration(&down_statements);
                    }
                }
                MigrationDirection::Both => {
                    if !up_statements.is_empty() {
                        println!();
                        output::print_migration(&up_statements);
                    }
                    if !down_statements.is_empty() {
                        println!();
                        println!(
                            "{}",
                            colored::Colorize::bold(colored::Colorize::underline(
                                "Rollback migration"
                            ))
                        );
                        println!();
                        output::print_migration(&down_statements);
                    }
                }
            }

            println!();
            output::print_summary(&diff);
        }
        OutputFormat::Json => {
            let json = serde_json::json!({
                "diff": diff,
                "migration": up_statements.iter().map(|s| &s.sql).collect::<Vec<_>>(),
                "rollback": down_statements.iter().map(|s| &s.sql).collect::<Vec<_>>(),
                "warnings": up_statements.iter()
                    .flat_map(|s| s.warnings.iter())
                    .collect::<Vec<_>>(),
                "has_changes": !diff.is_empty(),
                "summary": output::diff_summary(&diff),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
        }
        OutputFormat::Sql => match params.direction {
            MigrationDirection::Up => {
                print!("{}", output::migration_to_sql(&up_statements));
            }
            MigrationDirection::Down => {
                print!("{}", output::migration_to_sql(&down_statements));
            }
            MigrationDirection::Both => {
                println!("-- === UP ===");
                print!("{}", output::migration_to_sql(&up_statements));
                println!("-- === DOWN ===");
                print!("{}", output::migration_to_sql(&down_statements));
            }
        },
    }

    // Write migration file
    if let Some(ref path) = params.out {
        if params.should_write {
            let sql = match params.direction {
                MigrationDirection::Up => output::migration_to_sql(&up_statements),
                MigrationDirection::Down => output::migration_to_sql(&down_statements),
                MigrationDirection::Both => {
                    format!(
                        "-- === UP ===\n{}\n-- === DOWN ===\n{}",
                        output::migration_to_sql(&up_statements),
                        output::migration_to_sql(&down_statements)
                    )
                }
            };
            std::fs::write(path, sql).map_err(|e| {
                eprintln!("Error writing migration file: {e}");
                ExitCode::from(2)
            })?;
            eprintln!("Migration written to {path}");
        } else {
            eprintln!("Dry run: would write migration to {path} (use --write to save)");
        }
    }

    if params.ci && !diff.is_empty() {
        return Err(ExitCode::from(1));
    }

    Ok(())
}

fn check_protected(
    diff: &dbdiff::diff::SchemaDiff,
    protected: &config::ProtectedConfig,
) -> Result<(), String> {
    for table in &diff.removed_tables {
        if protected.tables.contains(&table.name) {
            return Err(format!(
                "Protected table '{}' would be dropped. Remove it from the protected list to allow this.",
                table.name
            ));
        }
    }

    for table_diff in &diff.modified_tables {
        for col in &table_diff.removed_columns {
            for pattern in &protected.columns {
                let matches = if let Some(col_name) = pattern.strip_prefix("*.") {
                    col.name == col_name
                } else if let Some((table, col_name)) = pattern.split_once('.') {
                    table_diff.table_name == table && col.name == col_name
                } else {
                    false
                };
                if matches {
                    return Err(format!(
                        "Protected column '{}.{}' would be dropped. Remove it from the protected list to allow this.",
                        table_diff.table_name, col.name
                    ));
                }
            }
        }
    }

    Ok(())
}

async fn run_validate(args: ValidateArgs) -> Result<(), ExitCode> {
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

async fn run_tables(args: TablesArgs) -> Result<(), ExitCode> {
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

fn run_init() -> Result<(), ExitCode> {
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

fn resolve_ssl_mode(mode: SslMode) -> loader::SslMode {
    match mode {
        SslMode::Disable => loader::SslMode::Disable,
        SslMode::Prefer => loader::SslMode::Prefer,
        SslMode::Require => loader::SslMode::Require,
    }
}

fn create_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}
