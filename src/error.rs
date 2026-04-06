use std::fmt;

/// All error types that can occur in dbdiff.
#[derive(Debug)]
pub enum DbDiffError {
    #[cfg(feature = "postgres")]
    Postgres(tokio_postgres::Error),
    #[cfg(feature = "mysql")]
    MySQL(mysql_async::Error),
    #[cfg(feature = "sqlite")]
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    SqlParse(String),
    InvalidArg(String),
}

impl fmt::Display for DbDiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(e) => write!(f, "PostgreSQL error: {e}"),
            #[cfg(feature = "mysql")]
            Self::MySQL(e) => write!(f, "MySQL error: {e}"),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(e) => write!(f, "SQLite error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::SqlParse(msg) => write!(f, "SQL parse error: {msg}"),
            Self::InvalidArg(msg) => write!(f, "Invalid argument: {msg}"),
        }
    }
}

impl std::error::Error for DbDiffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(e) => Some(e),
            #[cfg(feature = "mysql")]
            Self::MySQL(e) => Some(e),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::SqlParse(_) | Self::InvalidArg(_) => None,
        }
    }
}

#[cfg(feature = "postgres")]
impl From<tokio_postgres::Error> for DbDiffError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::Postgres(e)
    }
}

#[cfg(feature = "mysql")]
impl From<mysql_async::Error> for DbDiffError {
    fn from(e: mysql_async::Error) -> Self {
        Self::MySQL(e)
    }
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for DbDiffError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<std::io::Error> for DbDiffError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
