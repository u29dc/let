#![forbid(unsafe_code)]

use let_sdk::paths::PathOverrides;
use serde_json::Value;

pub mod config;
pub mod health;
pub mod start;
pub mod tools;

#[derive(Debug, Clone)]
pub struct SharedArgs {
    pub overrides: PathOverrides,
}

#[derive(Debug, Clone, Default)]
pub struct MetaOptions {
    pub count: Option<usize>,
    pub total: Option<usize>,
    pub has_more: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub data: Value,
    pub text: Option<String>,
    pub meta: MetaOptions,
}

impl CommandOutput {
    pub fn new(data: Value) -> Self {
        Self {
            data,
            text: None,
            meta: MetaOptions::default(),
        }
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn with_count(mut self, count: usize) -> Self {
        self.meta.count = Some(count);
        self
    }

    pub fn with_total(mut self, total: usize) -> Self {
        self.meta.total = Some(total);
        self
    }

    pub fn with_has_more(mut self, has_more: bool) -> Self {
        self.meta.has_more = Some(has_more);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub hint: String,
    pub exit_code: i32,
}

impl CommandError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
        exit_code: i32,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            hint: hint.into(),
            exit_code,
        }
    }

    pub fn runtime(
        code: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(code, message, hint, 1)
    }
}

impl From<let_sdk::LetError> for CommandError {
    fn from(err: let_sdk::LetError) -> Self {
        let exit_code = err.exit_code();
        Self {
            code: err.code.as_str().to_string(),
            message: err.message,
            hint: err.hint,
            exit_code,
        }
    }
}

pub type CommandResult = Result<CommandOutput, CommandError>;

pub fn placeholder(group: &str) -> CommandResult {
    Err(CommandError::runtime(
        "NOT_IMPLEMENTED",
        format!("command group `{group}` is not wired yet"),
        "run `let tools --json` to inspect available command metadata",
    ))
}
