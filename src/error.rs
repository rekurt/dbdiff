use std::fmt;

/// All error types that can occur in dbdiff.
#[derive(Debug)]
pub enum DbDiffError {
    #[cfg(feature = "postgres")]
    Postgres(tokio_postgres::Error),
    #[cfg(feature = "postgres")]
    PostgresConnect {
        host: String,
        source: tokio_postgres::Error,
    },
    #[cfg(feature = "mysql")]
    MySQL(mysql_async::Error),
    #[cfg(feature = "sqlite")]
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    SqlParse(String),
    InvalidArg(String),
}

/// Walk the error source chain and collect all messages.
fn format_error_chain(err: &dyn std::error::Error) -> String {
    let mut msg = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        msg.push_str(": ");
        msg.push_str(&cause.to_string());
        source = cause.source();
    }
    msg
}

/// Extract host from a DSN, stripping credentials.
/// "postgres://user:secret@myhost:5432/mydb" -> "myhost:5432/mydb"
pub(crate) fn sanitize_dsn(dsn: &str) -> String {
    if let Some(at_pos) = dsn.find('@') {
        dsn[at_pos + 1..].to_string()
    } else {
        dsn.strip_prefix("postgres://")
            .or_else(|| dsn.strip_prefix("postgresql://"))
            .unwrap_or(dsn)
            .to_string()
    }
}

impl fmt::Display for DbDiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(e) => write!(f, "PostgreSQL error: {}", format_error_chain(e)),
            #[cfg(feature = "postgres")]
            Self::PostgresConnect { host, source } => {
                write!(
                    f,
                    "PostgreSQL connection to '{}' failed: {}",
                    host,
                    format_error_chain(source)
                )
            }
            #[cfg(feature = "mysql")]
            Self::MySQL(e) => write!(f, "MySQL error: {}", format_error_chain(e)),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(e) => write!(f, "SQLite error: {}", format_error_chain(e)),
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
            #[cfg(feature = "postgres")]
            Self::PostgresConnect { source, .. } => Some(source),
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
