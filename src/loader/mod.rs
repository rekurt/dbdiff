pub mod postgres;
pub mod sqlfile;

use crate::error::DbDiffError;
use crate::model::Schema;

/// Load a schema from a source string.
///
/// Dispatches to the appropriate loader based on the source format:
/// - `postgres://...` or `postgresql://...` → live PostgreSQL connection
/// - File path ending in `.sql` or existing file → SQL file parser
pub async fn load_schema(source: &str) -> Result<Schema, DbDiffError> {
    if source.starts_with("postgres://") || source.starts_with("postgresql://") {
        postgres::load(source).await
    } else if source.ends_with(".sql") || std::path::Path::new(source).exists() {
        sqlfile::load_file(source)
    } else {
        Err(DbDiffError::InvalidArg(format!(
            "Cannot determine source type for '{source}'. \
             Expected a PostgreSQL DSN (postgres://...) or a .sql file path."
        )))
    }
}
