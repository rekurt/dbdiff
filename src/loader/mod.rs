#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod sqlfile;
#[cfg(feature = "sqlite")]
pub mod sqlite;

use crate::error::DbDiffError;
use crate::model::Schema;

/// Load a schema from a source string.
///
/// Dispatches to the appropriate loader based on the source format:
/// - `postgres://...` or `postgresql://...` → live PostgreSQL connection
/// - `mysql://...` or `mariadb://...` → live MySQL/MariaDB connection
/// - `.db`, `.sqlite`, `.sqlite3`, or `sqlite://...` → SQLite database file
/// - `.sql` file path → SQL file parser
pub async fn load_schema(source: &str) -> Result<Schema, DbDiffError> {
    #[cfg(feature = "postgres")]
    if source.starts_with("postgres://") || source.starts_with("postgresql://") {
        return postgres::load(source).await;
    }

    #[cfg(feature = "mysql")]
    if source.starts_with("mysql://") || source.starts_with("mariadb://") {
        return mysql::load(source).await;
    }

    #[cfg(feature = "sqlite")]
    if is_sqlite_source(source) {
        let source = source.to_string();
        return tokio::task::spawn_blocking(move || sqlite::load(&source))
            .await
            .map_err(|e| DbDiffError::InvalidArg(e.to_string()))?;
    }

    if source.ends_with(".sql") || std::path::Path::new(source).exists() {
        return sqlfile::load_file(source);
    }

    Err(DbDiffError::InvalidArg(format!(
        "Cannot determine source type for '{source}'. \
         Expected a database DSN (postgres://, mysql://, sqlite://) or a .sql file path."
    )))
}

#[cfg(feature = "sqlite")]
fn is_sqlite_source(source: &str) -> bool {
    source.starts_with("sqlite://")
        || source.ends_with(".db")
        || source.ends_with(".sqlite")
        || source.ends_with(".sqlite3")
}
