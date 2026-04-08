use std::process::ExitCode;

use dbdiff::ci::{self, CiReport};
use dbdiff::cli::{DiffParams, MigrationDirection, OutputFormat};
use dbdiff::config;
use dbdiff::loader;
use dbdiff::loader::SqlDialect;
use dbdiff::migration;
use dbdiff::migration::MigrationStatement;
use dbdiff::output;

use super::helpers::{apply_color_mode, create_spinner, resolve_ssl_mode};

pub async fn run_diff(params: DiffParams) -> Result<(), ExitCode> {
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

    let diff = dbdiff::diff::diff_schemas_with_options(
        &left.schema,
        &right.schema,
        params.detect_renames,
    );

    if let Err(msg) = check_protected(&diff, &cfg.protected, params.direction) {
        eprintln!("Error: {msg}");
        return Err(ExitCode::from(2));
    }

    let migration_dialect = match (left.dialect, right.dialect) {
        (
            SqlDialect::SqlFile | SqlDialect::Snapshot,
            SqlDialect::SqlFile | SqlDialect::Snapshot,
        ) => SqlDialect::Postgres,
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

    if params.concurrently && !matches!(migration_dialect, SqlDialect::Postgres) {
        eprintln!(
            "Warning: --concurrently only affects PostgreSQL. \
             Ignoring for {:?} dialect. Transaction wrapping is also disabled.",
            migration_dialect
        );
    }

    let mut up_statements = migration::generate_migration(&diff, migration_dialect, params.concurrently);
    let down_statements = migration::generate_rollback(&diff, migration_dialect, params.concurrently);

    // Check for duplicate data that would prevent unique index creation
    let is_pg_source = params.source.starts_with("postgres://")
        || params.source.starts_with("postgresql://");

    if is_pg_source {
        let unique_indexes: Vec<(&str, &str, &[String])> = diff
            .modified_tables
            .iter()
            .flat_map(|td| {
                td.added_indexes.iter().filter(|idx| idx.is_unique).map(
                    move |idx| {
                        (
                            td.table_name.as_str(),
                            idx.name.as_str(),
                            idx.columns.as_slice(),
                        )
                    },
                )
            })
            .collect();

        if !unique_indexes.is_empty() {
            let pg_ssl = resolve_ssl_mode(params.ssl_mode);
            let pg_ssl_mode = match pg_ssl {
                loader::SslMode::Disable => loader::postgres::PgSslMode::Disable,
                loader::SslMode::Prefer => loader::postgres::PgSslMode::Prefer,
                loader::SslMode::Require => loader::postgres::PgSslMode::Require,
            };

            let mut duplicates = Vec::new();
            for (table, idx_name, columns) in &unique_indexes {
                match loader::postgres::check_duplicates(
                    &params.source,
                    pg_ssl_mode,
                    table,
                    idx_name,
                    columns,
                )
                .await
                {
                    Ok(Some(dup)) => duplicates.push(dup),
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("Warning: failed to check duplicates for {table}: {e}");
                    }
                }
            }

            if !duplicates.is_empty() {
                if !params.force {
                    eprintln!();
                    eprintln!("Error: Duplicate data found that would prevent unique index creation:");
                    eprintln!();
                    for dup in &duplicates {
                        eprintln!(
                            "  Table '{}', index '{}' on columns ({}): {} duplicate group(s)",
                            dup.table,
                            dup.index_name,
                            dup.columns.join(", "),
                            dup.duplicate_count
                        );
                        for sample in &dup.sample_values {
                            eprintln!("    {sample}");
                        }
                    }
                    eprintln!();
                    eprintln!("Use --force to add TRUNCATE TABLE statements before creating unique indexes.");
                    return Err(ExitCode::from(2));
                }

                // With --force: inject TRUNCATE TABLE before each unique index on tables with duplicates
                let dup_tables: std::collections::HashSet<&str> =
                    duplicates.iter().map(|d| d.table.as_str()).collect();

                let mut patched = Vec::with_capacity(up_statements.len() + duplicates.len());
                let mut truncated_tables: std::collections::HashSet<String> =
                    std::collections::HashSet::new();

                for stmt in up_statements {
                    // Detect CREATE UNIQUE INDEX on a table with duplicates.
                    // Match both quoted (`ON "table"`) and unquoted (`ON table`) forms.
                    let sql_matches_table = |sql: &str, table: &str| -> bool {
                        sql.contains(&format!("ON \"{table}\""))
                            || sql.contains(&format!("ON {table}("))
                    };

                    let needs_truncate = dup_tables.iter().any(|table| {
                        stmt.sql.contains("CREATE UNIQUE INDEX")
                            && sql_matches_table(&stmt.sql, table)
                            && !truncated_tables.contains(*table)
                    });

                    if needs_truncate {
                        if let Some(table) = dup_tables.iter().find(|t| {
                            sql_matches_table(&stmt.sql, t)
                        }) {
                            patched.push(MigrationStatement {
                                sql: format!(
                                    "TRUNCATE TABLE \"{table}\";",
                                ),
                                warnings: vec![format!(
                                    "TRUNCATE TABLE will delete ALL data from '{table}'. This is required because duplicate rows prevent creating a unique index."
                                )],
                                is_blocking: true,
                            });
                            truncated_tables.insert(table.to_string());
                        }
                    }
                    patched.push(stmt);
                }
                up_statements = patched;
            }
        }
    }

    let format = params.resolve_format(&cfg.output.format);

    let report = match params.direction {
        MigrationDirection::Down => CiReport::from_diff_reversed(&diff, &down_statements),
        _ => CiReport::from_diff(&diff, &up_statements),
    };

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
            let json = serde_json::to_string_pretty(&report).map_err(|e| {
                eprintln!("Error: failed to serialize report as JSON: {e}");
                ExitCode::from(2)
            })?;
            println!("{json}");
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&report).map_err(|e| {
                eprintln!("Error: failed to serialize report as YAML: {e}");
                ExitCode::from(2)
            })?;
            print!("{yaml}");
        }
        OutputFormat::Sql => match params.direction {
            MigrationDirection::Up => {
                print!("{}", output::migration_to_sql(&up_statements, params.transaction));
            }
            MigrationDirection::Down => {
                print!("{}", output::migration_to_sql(&down_statements, params.transaction));
            }
            MigrationDirection::Both => {
                println!("-- === UP ===");
                print!("{}", output::migration_to_sql(&up_statements, params.transaction));
                println!("-- === DOWN ===");
                print!("{}", output::migration_to_sql(&down_statements, params.transaction));
            }
        },
    }

    if let Some(ref path) = params.out {
        if params.should_write {
            let sql = match params.direction {
                MigrationDirection::Up => output::migration_to_sql(&up_statements, params.transaction),
                MigrationDirection::Down => output::migration_to_sql(&down_statements, params.transaction),
                MigrationDirection::Both => {
                    format!(
                        "-- === UP ===\n{}\n-- === DOWN ===\n{}",
                        output::migration_to_sql(&up_statements, params.transaction),
                        output::migration_to_sql(&down_statements, params.transaction)
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

    if params.ci && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        report.emit_github_annotations();
    }

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
    let check_up = !matches!(direction, MigrationDirection::Down);
    let check_down = !matches!(direction, MigrationDirection::Up);

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
            if c == "*" {
                table_name == t
            } else {
                table_name == t && col_name == c
            }
        } else {
            false
        }
    })
}
