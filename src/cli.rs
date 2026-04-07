use clap::Parser;

/// Compare database schemas, detect drift, and generate migrations.
///
/// dbdiff compares two database schemas — either two live databases (DSN vs DSN)
/// or a live database against a .sql file — and generates the migration SQL
/// needed to bring the source schema in sync with the target.
#[derive(Parser, Debug)]
#[command(name = "dbdiff", version, about, long_about = None)]
pub struct Args {
    /// Source database DSN (e.g. postgres://user:pass@host/db)
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

    /// Show diff and migration without writing any files
    #[arg(long)]
    pub dry_run: bool,

    /// Output format (overrides config file setting)
    #[arg(long, value_enum)]
    pub format: Option<OutputFormat>,

    /// Path to config file
    #[arg(long, value_name = "FILE", default_value = ".dbdiff.yml")]
    pub config: String,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Pretty,
    Json,
    Sql,
}

impl Args {
    /// Resolve the target schema source — either from --schema flag or positional arg.
    pub fn target_source(&self) -> Result<&str, &'static str> {
        if let Some(ref schema) = self.schema_file {
            Ok(schema.as_str())
        } else if let Some(ref target) = self.target {
            Ok(target.as_str())
        } else {
            Err("Either a target DSN or --schema <file> is required")
        }
    }

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
