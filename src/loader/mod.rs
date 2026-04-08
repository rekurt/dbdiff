#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod sqlfile;
#[cfg(feature = "sqlite")]
pub mod sqlite;

use crate::error::DbDiffError;
use crate::model::Schema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDialect {
    Postgres,
    MySql,
    Sqlite,
    SqlFile,
    /// JSON snapshot — carries all object types (views, enums, sequences)
    Snapshot,
}

#[derive(Debug, Clone)]
pub struct LoadedSchema {
    pub schema: Schema,
    pub dialect: SqlDialect,
}

/// SSL mode for database connections.
#[derive(Debug, Clone, Copy, Default)]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

/// Load a schema from a source string.
///
/// Dispatches to the appropriate loader based on the source format:
/// - `postgres://...` or `postgresql://...` -> live PostgreSQL connection
/// - `mysql://...` or `mariadb://...` -> live MySQL/MariaDB connection
/// - `.db`, `.sqlite`, `.sqlite3`, or `sqlite://...` -> SQLite database file
/// - `.sql` file path -> SQL file parser
pub async fn load_schema(source: &str) -> Result<LoadedSchema, DbDiffError> {
    load_schema_with_ssl(source, SslMode::Prefer).await
}

/// Load a schema from a source string with specified SSL mode.
pub async fn load_schema_with_ssl(
    source: &str,
    ssl_mode: SslMode,
) -> Result<LoadedSchema, DbDiffError> {
    #[cfg(feature = "postgres")]
    if source.starts_with("postgres://") || source.starts_with("postgresql://") {
        let pg_ssl = match ssl_mode {
            SslMode::Disable => postgres::PgSslMode::Disable,
            SslMode::Prefer => postgres::PgSslMode::Prefer,
            SslMode::Require => postgres::PgSslMode::Require,
        };
        return postgres::load_with_ssl(source, pg_ssl)
            .await
            .map(|schema| LoadedSchema {
                schema,
                dialect: SqlDialect::Postgres,
            });
    }

    #[cfg(feature = "mysql")]
    if source.starts_with("mysql://") || source.starts_with("mariadb://") {
        return mysql::load(source).await.map(|schema| LoadedSchema {
            schema,
            dialect: SqlDialect::MySql,
        });
    }

    #[cfg(feature = "sqlite")]
    if is_sqlite_source(source) {
        let source = source.to_string();
        let schema = tokio::task::spawn_blocking(move || sqlite::load(&source))
            .await
            .map_err(|e| DbDiffError::invalid_arg(e.to_string()))?;
        return Ok(LoadedSchema {
            schema: schema?,
            dialect: SqlDialect::Sqlite,
        });
    }

    // JSON snapshot files
    if source.ends_with(".json") {
        let content = std::fs::read_to_string(source)?;
        let snapshot: crate::model::SchemaSnapshot = serde_json::from_str(&content)
            .map_err(|e| DbDiffError::invalid_arg(format!("Failed to parse JSON snapshot: {e}")))?;
        return Ok(LoadedSchema {
            schema: snapshot.into(),
            dialect: SqlDialect::Snapshot,
        });
    }

    if source.ends_with(".sql") || std::path::Path::new(source).exists() {
        return sqlfile::load_file(source).map(|schema| LoadedSchema {
            schema,
            dialect: SqlDialect::SqlFile,
        });
    }

    Err(DbDiffError::invalid_arg(format!(
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
