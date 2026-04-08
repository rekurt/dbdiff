use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};

use dbdiff::ci::{self, CiReport};
use dbdiff::cli::{
    Cli, ColorMode, DiffParams, MigrationDirection, OutputFormat, ResolvedCommand, SnapshotArgs,
    SslMode, TablesArgs, ValidateArgs,
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
        ResolvedCommand::Snapshot(args) => run_snapshot(args).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

async fn run_diff(params: DiffParams) -> Result<(), ExitCode> {
    // Apply color mode
    apply_color_mode(params.color);

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
            let safe_source = dbdiff::error::sanitize_dsn(&params.source);
            let safe_target = dbdiff::error::sanitize_dsn(&params.target_source);
            let err = dbdiff::error::DbDiffError::timeout(
                &format!("{safe_source} / {safe_target}"),
                params.timeout,
            );
            eprintln!("Error: {err}");
            return Err(ExitCode::from(2));
        }
    };

    config::filter::apply_ignore(&mut left.schema, &cfg.ignore);
    config::filter::apply_ignore(&mut right.schema, &cfg.ignore);

    // When comparing against SQL files (not JSON snapshots), clear object types
    // that the SQL parser cannot load (views, enums, sequences) to avoid false diffs.
    // JSON snapshots (SqlDialect::Snapshot) DO carry these objects, so don't clear them.
    if left.dialect == SqlDialect::SqlFile {
        right.schema.views.clear();
        right.schema.enums.clear();
        right.schema.sequences.clear();
    }
    if right.dialect == SqlDialect::SqlFile {
        left.schema.views.clear();
        left.schema.enums.clear();
        left.schema.sequences.clear();
    }

    if let Some(false) = cfg.output.color {
        colored::control::set_override(false);
    }

    let diff = diff_schemas(&left.schema, &right.schema);

    // Check protected objects based on migration direction
    if let Err(msg) = check_protected(&diff, &cfg.protected, params.direction) {
        eprintln!("Error: {msg}");
        return Err(ExitCode::from(2));
    }

    let migration_dialect = match (left.dialect, right.dialect) {
        (SqlDialect::SqlFile | SqlDialect::Snapshot, other)
        | (other, SqlDialect::SqlFile | SqlDialect::Snapshot) => other,
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

    // Build CI report from the statements matching the selected direction
    let ci_statements = match params.direction {
        MigrationDirection::Down => &down_statements,
        _ => &up_statements,
    };
    let report = CiReport::from_diff(&diff, ci_statements);

    match format {
        OutputFormat::Pretty => {
            output::print_diff(&diff);

            let print_fn = if params.explain {
                output::print_migration_explained
            } else {
                output::print_migration
            };

            match params.direction {
                MigrationDirection::Up => {
                    if !up_statements.is_empty() {
                        println!();
                        print_fn(&up_statements);
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
                        print_fn(&down_statements);
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
                        print_fn(&down_statements);
                    }
                }
            }

            println!();
            output::print_summary(&diff);
        }
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report).unwrap_or_default();
            println!("{json}");
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&report).unwrap_or_default();
            print!("{yaml}");
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

    // GitHub Actions annotations (auto-detect from env)
    if params.ci && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        report.emit_github_annotations();
    }

    // CI mode: determine exit code from report
    if params.ci {
        let code = report.exit_code(params.fail_on_blocking);
        if code != ci::EXIT_OK {
            return Err(ExitCode::from(code));
        }
    }

    Ok(())
}

fn check_protected(
    diff: &dbdiff::diff::SchemaDiff,
    protected: &config::ProtectedConfig,
    direction: MigrationDirection,
) -> Result<(), String> {
    // In UP direction, removed_tables/removed_columns are dropped.
    // In DOWN direction, added_tables/added_columns are dropped (rollback).
    // In BOTH, check both directions.
    let check_up = !matches!(direction, MigrationDirection::Down);
    let check_down = !matches!(direction, MigrationDirection::Up);

    // Check tables that would be dropped
    let dropped_tables: Vec<&dbdiff::model::Table> = std::iter::empty()
        .chain(if check_up {
            diff.removed_tables.iter().collect::<Vec<_>>()
        } else {
            vec![]
        })
        .chain(if check_down {
            diff.added_tables.iter().collect::<Vec<_>>()
        } else {
            vec![]
        })
        .collect();

    for table in &dropped_tables {
        if protected.tables.contains(&table.name) {
            return Err(format!(
                "Protected table '{}' would be dropped. Remove it from the protected list to allow this.",
                table.name
            ));
        }
        for col in table.columns.values() {
            if column_is_protected(&table.name, &col.name, &protected.columns) {
                return Err(format!(
                    "Protected column '{}.{}' would be dropped (table '{}' is being removed). \
                     Remove it from the protected list to allow this.",
                    table.name, col.name, table.name
                ));
            }
        }
    }

    // Check columns that would be dropped from modified tables
    for table_diff in &diff.modified_tables {
        let dropped_cols: Vec<&dbdiff::model::Column> = std::iter::empty()
            .chain(if check_up {
                table_diff.removed_columns.iter().collect::<Vec<_>>()
            } else {
                vec![]
            })
            .chain(if check_down {
                table_diff.added_columns.iter().collect::<Vec<_>>()
            } else {
                vec![]
            })
            .collect();

        for col in &dropped_cols {
            if column_is_protected(&table_diff.table_name, &col.name, &protected.columns) {
                return Err(format!(
                    "Protected column '{}.{}' would be dropped. Remove it from the protected list to allow this.",
                    table_diff.table_name, col.name
                ));
            }
        }
    }

    Ok(())
}

fn column_is_protected(table_name: &str, col_name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        if let Some(c) = pattern.strip_prefix("*.") {
            col_name == c
        } else if let Some((t, c)) = pattern.split_once('.') {
            table_name == t && col_name == c
        } else {
            false
        }
    })
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

async fn run_snapshot(args: SnapshotArgs) -> Result<(), ExitCode> {
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
            let json = serde_json::to_string_pretty(&snapshot).unwrap_or_default();

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

fn apply_color_mode(mode: ColorMode) {
    match mode {
        ColorMode::Auto => {
            if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                colored::control::set_override(false);
            }
        }
        ColorMode::Always => {
            colored::control::set_override(true);
        }
        ColorMode::Never => {
            colored::control::set_override(false);
        }
    }
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
