use std::fmt;

/// All error types that can occur in dbdiff.
#[derive(Debug)]
pub enum DbDiffError {
    Postgres(tokio_postgres::Error),
    Io(std::io::Error),
    SqlParse(String),
    InvalidArg(String),
}

impl fmt::Display for DbDiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Postgres(e) => write!(f, "PostgreSQL error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::SqlParse(msg) => write!(f, "SQL parse error: {msg}"),
            Self::InvalidArg(msg) => write!(f, "Invalid argument: {msg}"),
        }
    }
}

impl std::error::Error for DbDiffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Postgres(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::SqlParse(_) | Self::InvalidArg(_) => None,
        }
    }
}

impl From<tokio_postgres::Error> for DbDiffError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::Postgres(e)
    }
}

impl From<std::io::Error> for DbDiffError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
