pub mod filter;

use std::path::Path;

use serde::Deserialize;

use crate::error::DbDiffError;

/// Top-level configuration loaded from `.dbdiff.yml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ignore: IgnoreConfig,
    pub output: OutputConfig,
    pub protected: ProtectedConfig,
}

/// Objects that cannot be dropped or altered.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProtectedConfig {
    /// Table names that cannot be dropped.
    pub tables: Vec<String>,
    /// Column patterns that cannot be dropped (same format as ignore.columns).
    pub columns: Vec<String>,
}

/// Default config template for `dbdiff init`.
pub const DEFAULT_CONFIG_TEMPLATE: &str = r#"# dbdiff configuration
# See https://github.com/rekurt/dbdiff for documentation

ignore:
  tables: []
    # - _migrations
    # - schema_version
  columns: []
    # - "*.created_at"
    # - "*.updated_at"

protected:
  tables: []
    # - users
    # - payments
  columns: []
    # - "*.id"

output:
  format: pretty
  # color: true
"#;

/// Rules for ignoring tables and columns during comparison.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct IgnoreConfig {
    /// Exact table names to ignore (e.g. `_migrations`, `schema_version`).
    pub tables: Vec<String>,
    /// Column patterns to ignore:
    /// - `*.column_name` — ignore column in all tables
    /// - `table_name.*` — ignore all columns in a specific table
    /// - `table_name.column_name` — ignore exact table.column pair
    pub columns: Vec<String>,
}

/// Default output settings (CLI args override these).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Output format: `pretty`, `json`, or `sql`.
    pub format: Option<String>,
    /// Whether to use colored output.
    pub color: Option<bool>,
}

/// Load configuration from a YAML file.
///
/// Returns `Config::default()` if the file does not exist.
/// Returns an error only if the file exists but cannot be parsed.
pub fn load_config(path: &str) -> Result<Config, DbDiffError> {
    let path = Path::new(path);

    if !path.exists() {
        return Ok(Config::default());
    }

    let contents = std::fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&contents)
        .map_err(|e| DbDiffError::invalid_arg(format!("Failed to parse config file: {e}")))?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let yaml = r#"
ignore:
  tables:
    - _migrations
    - schema_version
  columns:
    - "*.created_at"
    - "sessions.*"
output:
  format: json
  color: false
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.ignore.tables, vec!["_migrations", "schema_version"]);
        assert_eq!(config.ignore.columns, vec!["*.created_at", "sessions.*"]);
        assert_eq!(config.output.format.as_deref(), Some("json"));
        assert_eq!(config.output.color, Some(false));
    }

    #[test]
    fn parse_empty_config() {
        let yaml = "";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.ignore.tables.is_empty());
        assert!(config.ignore.columns.is_empty());
        assert_eq!(config.output.format, None);
        assert_eq!(config.output.color, None);
    }

    #[test]
    fn parse_partial_config() {
        let yaml = r#"
ignore:
  tables:
    - _migrations
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.ignore.tables, vec!["_migrations"]);
        assert!(config.ignore.columns.is_empty());
        assert_eq!(config.output.format, None);
    }

    #[test]
    fn parse_protected_config() {
        let yaml = r#"
protected:
  tables:
    - users
    - payments
  columns:
    - "*.id"
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.protected.tables, vec!["users", "payments"]);
        assert_eq!(config.protected.columns, vec!["*.id"]);
    }

    #[test]
    fn missing_file_returns_default() {
        let config = load_config("/nonexistent/path/.dbdiff.yml").unwrap();
        assert!(config.ignore.tables.is_empty());
    }
}
