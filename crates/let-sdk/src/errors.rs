#![forbid(unsafe_code)]

use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidInput,
    NotFound,
    Conflict,
    NoConfig,
    NoSources,
    SchemaMismatch,
    Network,
    Parse,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "INVALID_INPUT",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::NoConfig => "NO_CONFIG",
            Self::NoSources => "NO_SOURCES",
            Self::SchemaMismatch => "SCHEMA_MISMATCH",
            Self::Network => "NETWORK_ERROR",
            Self::Parse => "PARSE_ERROR",
            Self::Internal => "INTERNAL_ERROR",
        }
    }

    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            Self::NoConfig | Self::NoSources | Self::SchemaMismatch
        )
    }
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct LetError {
    pub code: ErrorCode,
    pub message: String,
    pub hint: String,
}

impl LetError {
    pub fn new(code: ErrorCode, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        if self.code.is_blocking() { 2 } else { 1 }
    }
}

impl From<std::io::Error> for LetError {
    fn from(err: std::io::Error) -> Self {
        Self::new(
            ErrorCode::Internal,
            format!("io error: {err}"),
            "check filesystem permissions and path availability",
        )
    }
}

impl From<rusqlite::Error> for LetError {
    fn from(err: rusqlite::Error) -> Self {
        use rusqlite::ffi::ErrorCode as SqliteErrorCode;

        let (code, hint) = match err.sqlite_error_code() {
            Some(SqliteErrorCode::SchemaChanged) => (
                ErrorCode::SchemaMismatch,
                "verify the intelligence database schema and recreate it if needed",
            ),
            Some(SqliteErrorCode::DatabaseBusy | SqliteErrorCode::DatabaseLocked) => (
                ErrorCode::Conflict,
                "close competing database users and retry once the lock clears",
            ),
            Some(
                SqliteErrorCode::PermissionDenied
                | SqliteErrorCode::ReadOnly
                | SqliteErrorCode::CannotOpen
                | SqliteErrorCode::SystemIoFailure
                | SqliteErrorCode::DatabaseCorrupt
                | SqliteErrorCode::DiskFull
                | SqliteErrorCode::NotADatabase,
            ) => (
                ErrorCode::Internal,
                "check the database path, permissions, locks, and disk state",
            ),
            _ => (
                ErrorCode::Internal,
                "inspect the database file and retry the operation",
            ),
        };

        Self::new(code, format!("sqlite error: {err}"), hint)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub type Result<T> = std::result::Result<T, LetError>;

#[cfg(test)]
mod tests {
    use rusqlite::Error;

    use super::{ErrorCode, LetError};

    #[test]
    fn blocking_code_maps_to_exit_2() {
        let err = LetError::new(ErrorCode::NoConfig, "missing config", "run config setup");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.code.as_str(), "NO_CONFIG");
    }

    #[test]
    fn runtime_code_maps_to_exit_1() {
        let err = LetError::new(ErrorCode::Internal, "boom", "retry");
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn sqlite_busy_maps_to_conflict() {
        let err = LetError::from(Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        ));
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn sqlite_schema_change_maps_to_schema_mismatch() {
        let err = LetError::from(Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_SCHEMA),
            None,
        ));
        assert_eq!(err.code, ErrorCode::SchemaMismatch);
        assert_eq!(err.exit_code(), 2);
    }
}
