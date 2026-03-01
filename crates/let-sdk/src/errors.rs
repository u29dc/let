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
        matches!(self, Self::NoConfig | Self::NoSources | Self::SchemaMismatch)
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
        if self.code.is_blocking() {
            2
        } else {
            1
        }
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
        Self::new(
            ErrorCode::SchemaMismatch,
            format!("sqlite error: {err}"),
            "verify database schema and rebuild if needed",
        )
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
}
