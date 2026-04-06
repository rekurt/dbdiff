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

    /// Output format
    #[arg(long, value_enum, default_value = "pretty")]
    pub format: OutputFormat,
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
}
