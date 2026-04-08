use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

/// Compare database schemas, detect drift, and generate migrations.
///
/// dbdiff compares two database schemas — either two live databases (DSN vs DSN)
/// or a live database against a .sql file — and generates the migration SQL
/// needed to bring the source schema in sync with the target.
#[derive(Parser, Debug)]
#[command(
    name = "dbdiff",
    version,
    about,
    long_about = None,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Source database DSN (e.g. postgres://user:pass@host/db) or .sql file
    #[arg(value_name = "SOURCE", required = true)]
    pub source: Option<String>,

    /// Target database DSN to compare against
    #[arg(value_name = "TARGET")]
    pub target: Option<String>,

    /// Path to a .sql schema file to compare against (instead of a second DSN)
    #[arg(long = "schema", value_name = "FILE")]
    pub schema_file: Option<String>,

    /// Write generated migration SQL to a file
    #[arg(long, value_name = "FILE")]
    pub out: Option<String>,

    /// CI mode: exit with code 1 if schemas differ
    #[arg(long)]
    pub ci: bool,

    /// Preview mode: show diff and migration without writing any files (default when --out is set)
    #[arg(long)]
    pub dry_run: bool,

    /// Actually write the migration file (overrides safe-by-default dry-run)
    #[arg(long)]
    pub write: bool,

    /// Output format (overrides config file setting)
    #[arg(long, value_enum)]
    pub format: Option<OutputFormat>,

    /// Path to config file
    #[arg(long, value_name = "FILE", default_value = ".dbdiff.yml")]
    pub config: String,

    /// Connection timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value = "10")]
    pub timeout: u64,

    /// SSL mode for database connections (disable, prefer, require)
    #[arg(long, value_enum, default_value = "prefer")]
    pub ssl_mode: SslMode,

    /// Migration direction: up (forward), down (rollback), or both
    #[arg(long, value_enum, default_value = "up")]
    pub direction: MigrationDirection,

    /// Color output mode
    #[arg(long, value_enum, default_value = "auto")]
    pub color: ColorMode,

    /// Show explanation for each migration statement
    #[arg(long)]
    pub explain: bool,
}

/// Color output mode.
#[derive(Debug, Clone, Copy, clap::ValueEnum, Default)]
pub enum ColorMode {
    /// Auto-detect based on terminal
    #[default]
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

/// SSL mode for database connections.
#[derive(Debug, Clone, Copy, clap::ValueEnum, Default)]
pub enum SslMode {
    /// Do not use SSL
    Disable,
    /// Use SSL if available, fall back to plaintext
    #[default]
    Prefer,
    /// Require SSL, fail if unavailable
    Require,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Compare two database schemas and generate migration SQL (default command)
    Diff(DiffArgs),
    /// Test database connectivity and display server info
    Validate(ValidateArgs),
    /// List tables in a database
    Tables(TablesArgs),
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Create a default .dbdiff.yml configuration file
    Init,
    /// Export schema as JSON snapshot for offline comparison
    Snapshot(SnapshotArgs),
}

/// Arguments for the snapshot subcommand.
#[derive(Parser, Debug)]
pub struct SnapshotArgs {
    /// Database DSN to snapshot
    #[arg(value_name = "DSN")]
    pub dsn: String,

    /// Output file (defaults to stdout)
    #[arg(long, value_name = "FILE")]
    pub out: Option<String>,

    /// Connection timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value = "10")]
    pub timeout: u64,

    /// SSL mode for database connections
    #[arg(long, value_enum, default_value = "prefer")]
    pub ssl_mode: SslMode,
}

/// Arguments for the diff subcommand (mirrors top-level args).
#[derive(Parser, Debug)]
pub struct DiffArgs {
    /// Source database DSN (e.g. postgres://user:pass@host/db) or .sql file
    #[arg(value_name = "SOURCE")]
    pub source: String,

    /// Target database DSN to compare against
    #[arg(value_name = "TARGET")]
    pub target: Option<String>,

    /// Path to a .sql schema file to compare against (instead of a second DSN)
    #[arg(long = "schema", value_name = "FILE")]
    pub schema_file: Option<String>,

    /// Write generated migration SQL to a file
    #[arg(long, value_name = "FILE")]
    pub out: Option<String>,

    /// CI mode: exit with code 1 if schemas differ
    #[arg(long)]
    pub ci: bool,

    /// Preview mode: show diff and migration without writing any files (default when --out is set)
    #[arg(long)]
    pub dry_run: bool,

    /// Actually write the migration file (overrides safe-by-default dry-run)
    #[arg(long)]
    pub write: bool,

    /// Output format (overrides config file setting)
    #[arg(long, value_enum)]
    pub format: Option<OutputFormat>,

    /// Path to config file
    #[arg(long, value_name = "FILE", default_value = ".dbdiff.yml")]
    pub config: String,

    /// Connection timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value = "10")]
    pub timeout: u64,

    /// SSL mode for database connections (disable, prefer, require)
    #[arg(long, value_enum, default_value = "prefer")]
    pub ssl_mode: SslMode,

    /// Migration direction: up (forward), down (rollback), or both
    #[arg(long, value_enum, default_value = "up")]
    pub direction: MigrationDirection,

    /// Color output mode
    #[arg(long, value_enum, default_value = "auto")]
    pub color: ColorMode,

    /// Show explanation for each migration statement
    #[arg(long)]
    pub explain: bool,
}

/// Migration direction for output.
#[derive(Debug, Clone, Copy, clap::ValueEnum, Default)]
pub enum MigrationDirection {
    /// Generate forward migration only (default)
    #[default]
    Up,
    /// Generate rollback migration only
    Down,
    /// Generate both forward and rollback migrations
    Both,
}

/// Arguments for the tables subcommand.
#[derive(Parser, Debug)]
pub struct TablesArgs {
    /// Database DSN to list tables from
    #[arg(value_name = "DSN")]
    pub dsn: String,

    /// Connection timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value = "10")]
    pub timeout: u64,

    /// SSL mode for database connections (disable, prefer, require)
    #[arg(long, value_enum, default_value = "prefer")]
    pub ssl_mode: SslMode,
}

/// Arguments for the validate subcommand.
#[derive(Parser, Debug)]
pub struct ValidateArgs {
    /// Database DSN to validate (e.g. postgres://user:pass@host/db)
    #[arg(value_name = "DSN")]
    pub dsn: String,

    /// Connection timeout in seconds
    #[arg(long, value_name = "SECONDS", default_value = "10")]
    pub timeout: u64,

    /// SSL mode for database connections (disable, prefer, require)
    #[arg(long, value_enum, default_value = "prefer")]
    pub ssl_mode: SslMode,
}

/// Arguments for the completions subcommand.
#[derive(Parser, Debug)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: Shell,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Pretty,
    Json,
    Sql,
}

/// Resolved diff parameters, normalized from either top-level or subcommand args.
pub struct DiffParams {
    pub source: String,
    pub target_source: String,
    pub out: Option<String>,
    pub ci: bool,
    pub should_write: bool,
    pub format: Option<OutputFormat>,
    pub config: String,
    pub timeout: u64,
    pub ssl_mode: SslMode,
    pub direction: MigrationDirection,
    pub color: ColorMode,
    pub explain: bool,
}

impl Cli {
    /// Resolve the CLI into a concrete command to execute.
    pub fn resolve(self) -> ResolvedCommand {
        match self.command {
            Some(Commands::Diff(args)) => ResolvedCommand::Diff(args.into_params()),
            Some(Commands::Validate(args)) => ResolvedCommand::Validate(args),
            Some(Commands::Tables(args)) => ResolvedCommand::Tables(args),
            Some(Commands::Completions(args)) => ResolvedCommand::Completions(args),
            Some(Commands::Init) => ResolvedCommand::Init,
            Some(Commands::Snapshot(args)) => ResolvedCommand::Snapshot(args),
            None => {
                // Backward compat: top-level args are treated as diff
                let source = self.source.unwrap_or_default();
                let target_source = if let Some(ref schema) = self.schema_file {
                    schema.clone()
                } else if let Some(ref target) = self.target {
                    target.clone()
                } else {
                    String::new() // Will be caught by validation
                };
                // Safe-by-default: when --out is specified, default to dry-run unless --write is given
                let should_write = if self.out.is_some() {
                    self.write && !self.dry_run
                } else {
                    false
                };
                ResolvedCommand::Diff(DiffParams {
                    source,
                    target_source,
                    out: self.out,
                    ci: self.ci,
                    should_write,
                    format: self.format,
                    config: self.config,
                    timeout: self.timeout,
                    ssl_mode: self.ssl_mode,
                    direction: self.direction,
                    color: self.color,
                    explain: self.explain,
                })
            }
        }
    }

    /// Generate shell completions and write to stdout.
    pub fn generate_completions(shell: Shell) {
        let mut cmd = Cli::command();
        clap_complete::generate(shell, &mut cmd, "dbdiff", &mut std::io::stdout());
    }
}

impl DiffArgs {
    fn into_params(self) -> DiffParams {
        let target_source = if let Some(schema) = self.schema_file {
            schema
        } else {
            self.target.unwrap_or_default()
        };

        // Safe-by-default: when --out is specified, default to dry-run unless --write is given
        let should_write = if self.out.is_some() {
            self.write && !self.dry_run
        } else {
            false
        };

        DiffParams {
            source: self.source,
            target_source,
            out: self.out,
            ci: self.ci,
            should_write,
            format: self.format,
            config: self.config,
            timeout: self.timeout,
            ssl_mode: self.ssl_mode,
            direction: self.direction,
            color: self.color,
            explain: self.explain,
        }
    }
}

pub enum ResolvedCommand {
    Diff(DiffParams),
    Validate(ValidateArgs),
    Tables(TablesArgs),
    Completions(CompletionsArgs),
    Init,
    Snapshot(SnapshotArgs),
}

impl DiffParams {
    /// Resolve the output format: CLI flag > config file > default (pretty).
    pub fn resolve_format(&self, config_format: &Option<String>) -> OutputFormat {
        if let Some(ref fmt) = self.format {
            return fmt.clone();
        }
        if let Some(ref fmt) = config_format {
            match fmt.to_lowercase().as_str() {
                "json" => OutputFormat::Json,
                "sql" => OutputFormat::Sql,
                _ => OutputFormat::Pretty,
            }
        } else {
            OutputFormat::Pretty
        }
    }
}
