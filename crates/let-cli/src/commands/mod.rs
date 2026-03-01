#![forbid(unsafe_code)]

use let_sdk::paths::PathOverrides;
use serde::Serialize;
use serde_json::Value;

pub mod assess;
pub mod build;
pub mod config;
pub mod export;
pub mod fetch;
pub mod health;
pub mod ops;
pub mod score;
pub mod search;
pub mod start;
pub mod tools;
pub mod view;

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

pub fn to_camel_json<T: Serialize>(value: &T) -> Value {
    let raw = serde_json::to_value(value).expect("serialization should succeed");
    camelize_value(raw)
}

fn camelize_value(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(camelize_value).collect()),
        Value::Object(map) => {
            let mut next = serde_json::Map::with_capacity(map.len());
            for (key, item) in map {
                next.insert(snake_to_camel(&key), camelize_value(item));
            }
            Value::Object(next)
        }
        other => other,
    }
}

fn snake_to_camel(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut uppercase = false;
    for ch in key.chars() {
        if ch == '_' {
            uppercase = true;
            continue;
        }
        if uppercase {
            out.extend(ch.to_uppercase());
            uppercase = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde_json::json;

    use super::to_camel_json;

    #[derive(Debug, Serialize)]
    struct Demo {
        snake_case: bool,
        nested_value: DemoNested,
    }

    #[derive(Debug, Serialize)]
    struct DemoNested {
        deep_key: String,
    }

    #[test]
    fn camel_json_converts_nested_keys() {
        let payload = Demo {
            snake_case: true,
            nested_value: DemoNested {
                deep_key: "ok".to_owned(),
            },
        };

        let value = to_camel_json(&payload);
        assert_eq!(
            value,
            json!({
                "snakeCase": true,
                "nestedValue": {
                    "deepKey": "ok",
                },
            })
        );
    }
}
