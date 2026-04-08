use std::fmt;

/// Structured error codes for common failure scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// E001: Database connection failed (host unreachable, refused, DNS).
    Connection,
    /// E002: Authentication failed (bad credentials).
    Auth,
    /// E003: Permission denied (insufficient privileges).
    Permission,
    /// E004: Connection timed out.
    Timeout,
    /// E005: I/O error (file not found, permission denied).
    Io,
    /// E006: SQL parsing error.
    SqlParse,
    /// E007: Invalid argument or configuration.
    InvalidArg,
}

impl ErrorCode {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Connection => "E001",
            Self::Auth => "E002",
            Self::Permission => "E003",
            Self::Timeout => "E004",
            Self::Io => "E005",
            Self::SqlParse => "E006",
            Self::InvalidArg => "E007",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// All error types that can occur in dbdiff.
#[derive(Debug)]
pub struct DbDiffError {
    pub code: ErrorCode,
    pub message: String,
    pub hint: Option<String>,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl DbDiffError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            source: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    // -- Convenience constructors --

    pub fn connection(host: &str, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        let msg = format!("Connection to '{}' failed", sanitize_dsn(host));
        let cause = format_error_chain(&source);
        let hint = if cause.contains("refused") {
            format!(
                "Check that the database server is running and accepting connections.\n\
                 Try: pg_isready -h {host} or mysql --host={host} --execute='SELECT 1'",
            )
        } else if cause.contains("not found") || cause.contains("resolve") {
            format!("Host '{host}' could not be resolved. Check the hostname and DNS settings.")
        } else {
            "Check the connection string and ensure the database server is reachable.".to_string()
        };
        Self {
            code: ErrorCode::Connection,
            message: format!("{msg}: {cause}"),
            hint: Some(hint),
            source: Some(Box::new(source)),
        }
    }

    pub fn auth(host: &str, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        let msg = format!("Authentication to '{}' failed", sanitize_dsn(host));
        let cause = format_error_chain(&source);
        Self {
            code: ErrorCode::Auth,
            message: format!("{msg}: {cause}"),
            hint: Some(
                "Check your username and password. For PostgreSQL, also verify pg_hba.conf rules."
                    .to_string(),
            ),
            source: Some(Box::new(source)),
        }
    }

    pub fn timeout(host: &str, timeout_secs: u64) -> Self {
        Self::new(
            ErrorCode::Timeout,
            format!(
                "Connection to '{}' timed out after {}s",
                sanitize_dsn(host),
                timeout_secs
            ),
        )
        .with_hint(
            "The database server may be unreachable or overloaded. \
             Try increasing --timeout or check network connectivity.",
        )
    }

    pub fn io(message: impl Into<String>, source: std::io::Error) -> Self {
        let msg = message.into();
        let hint = if source.kind() == std::io::ErrorKind::NotFound {
            Some("Check that the file path is correct and the file exists.".to_string())
        } else if source.kind() == std::io::ErrorKind::PermissionDenied {
            Some("Check file permissions.".to_string())
        } else {
            None
        };
        Self {
            code: ErrorCode::Io,
            message: format!("{msg}: {source}"),
            hint,
            source: Some(Box::new(source)),
        }
    }

    pub fn sql_parse(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::SqlParse, message)
    }

    pub fn invalid_arg(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArg, message)
    }
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
            .or_else(|| dsn.strip_prefix("mysql://"))
            .or_else(|| dsn.strip_prefix("mariadb://"))
            .unwrap_or(dsn)
            .to_string()
    }
}

impl fmt::Display for DbDiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(ref hint) = self.hint {
            write!(f, "\n  Hint: {hint}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DbDiffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn std::error::Error + 'static))
    }
}

#[cfg(feature = "postgres")]
impl From<tokio_postgres::Error> for DbDiffError {
    fn from(e: tokio_postgres::Error) -> Self {
        let msg = format_error_chain(&e);
        if msg.contains("authentication") || msg.contains("password") {
            Self {
                code: ErrorCode::Auth,
                message: format!("PostgreSQL authentication failed: {msg}"),
                hint: Some(
                    "Check your username and password. For PostgreSQL, also verify pg_hba.conf rules."
                        .to_string(),
                ),
                source: Some(Box::new(e)),
            }
        } else if msg.contains("permission denied") {
            Self {
                code: ErrorCode::Permission,
                message: format!("PostgreSQL permission denied: {msg}"),
                hint: Some(
                    "The database user may lack required privileges. \
                     Grant SELECT on information_schema and pg_catalog."
                        .to_string(),
                ),
                source: Some(Box::new(e)),
            }
        } else {
            Self {
                code: ErrorCode::Connection,
                message: format!("PostgreSQL error: {msg}"),
                hint: None,
                source: Some(Box::new(e)),
            }
        }
    }
}

#[cfg(feature = "mysql")]
impl From<mysql_async::Error> for DbDiffError {
    fn from(e: mysql_async::Error) -> Self {
        let msg = format_error_chain(&e);
        if msg.contains("Access denied") || msg.contains("authentication") {
            Self {
                code: ErrorCode::Auth,
                message: format!("MySQL authentication failed: {msg}"),
                hint: Some("Check your username and password.".to_string()),
                source: Some(Box::new(e)),
            }
        } else {
            Self {
                code: ErrorCode::Connection,
                message: format!("MySQL error: {msg}"),
                hint: None,
                source: Some(Box::new(e)),
            }
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for DbDiffError {
    fn from(e: rusqlite::Error) -> Self {
        let msg = format_error_chain(&e);
        if msg.contains("unable to open") {
            Self {
                code: ErrorCode::Io,
                message: format!("SQLite error: {msg}"),
                hint: Some("Check that the database file exists and is readable.".to_string()),
                source: Some(Box::new(e)),
            }
        } else {
            Self {
                code: ErrorCode::Connection,
                message: format!("SQLite error: {msg}"),
                hint: None,
                source: Some(Box::new(e)),
            }
        }
    }
}

impl From<std::io::Error> for DbDiffError {
    fn from(e: std::io::Error) -> Self {
        Self::io("I/O error", e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_display() {
        assert_eq!(ErrorCode::Connection.code(), "E001");
        assert_eq!(ErrorCode::Auth.code(), "E002");
        assert_eq!(ErrorCode::Timeout.code(), "E004");
    }

    #[test]
    fn sanitize_dsn_strips_credentials() {
        assert_eq!(
            sanitize_dsn("postgres://user:secret@myhost:5432/mydb"),
            "myhost:5432/mydb"
        );
        assert_eq!(
            sanitize_dsn("mysql://root:pass@localhost/db"),
            "localhost/db"
        );
        assert_eq!(sanitize_dsn("postgres://localhost/db"), "localhost/db");
    }

    #[test]
    fn error_display_with_hint() {
        let err = DbDiffError::timeout("myhost:5432", 10);
        let display = format!("{err}");
        assert!(display.contains("[E004]"));
        assert!(display.contains("timed out"));
        assert!(display.contains("Hint:"));
    }

    #[test]
    fn error_display_without_hint() {
        let err = DbDiffError::new(ErrorCode::Connection, "test error");
        let display = format!("{err}");
        assert!(display.contains("[E001]"));
        assert!(display.contains("test error"));
        assert!(!display.contains("Hint:"));
    }

    #[test]
    fn io_error_has_hint_for_not_found() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = DbDiffError::io("reading schema", io_err);
        assert_eq!(err.code, ErrorCode::Io);
        assert!(err.hint.is_some());
        assert!(err.hint.unwrap().contains("file path"));
    }
}
