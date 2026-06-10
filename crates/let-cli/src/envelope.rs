#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use crate::commands::{CommandError, CommandOutput, ErrorDetail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Toon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub tool: String,
    pub elapsed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

impl Meta {
    pub fn new(tool: impl Into<String>, elapsed: u64) -> Self {
        Self {
            tool: tool.into(),
            elapsed,
            count: None,
            total: None,
            has_more: None,
        }
    }

    pub fn with_count(mut self, count: Option<usize>) -> Self {
        self.count = count;
        self
    }

    pub fn with_total(mut self, total: Option<usize>) -> Self {
        self.total = total;
        self
    }

    pub fn with_has_more(mut self, has_more: Option<bool>) -> Self {
        self.has_more = has_more;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuccessEnvelope<T> {
    pub ok: bool,
    pub data: T,
    pub meta: Meta,
}

impl<T> SuccessEnvelope<T> {
    pub fn new(data: T, meta: Meta) -> Self {
        Self {
            ok: true,
            data,
            meta,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub ok: bool,
    pub error: ErrorPayload,
    pub meta: Meta,
}

impl ErrorEnvelope {
    pub fn new(error: ErrorPayload, meta: Meta) -> Self {
        Self {
            ok: false,
            error,
            meta,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    pub hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<ErrorDetail>>,
}

impl ErrorPayload {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
        details: Option<Vec<ErrorDetail>>,
    ) -> Self {
        Self {
            code: normalize_error_code(&code.into()),
            message: message.into(),
            hint: hint.into(),
            details,
        }
    }
}

pub fn emit_result(
    result: &Result<CommandOutput, CommandError>,
    tool: &str,
    elapsed: u64,
    format: OutputFormat,
) -> i32 {
    match result {
        Ok(output) => {
            let meta = Meta::new(tool, elapsed)
                .with_count(output.meta.count)
                .with_total(output.meta.total)
                .with_has_more(output.meta.has_more);
            let envelope = SuccessEnvelope::new(output.data.clone(), meta);
            match render(&envelope, format) {
                Ok(text) => {
                    println!("{text}");
                    0
                }
                Err(error) => emit_serialization_error(tool, elapsed, format, error),
            }
        }
        Err(err) => {
            let envelope = ErrorEnvelope::new(
                ErrorPayload::new(
                    err.code.clone(),
                    err.message.clone(),
                    err.hint.clone(),
                    err.details.clone(),
                ),
                Meta::new(tool, elapsed),
            );
            match render(&envelope, format) {
                Ok(text) => println!("{text}"),
                Err(error) => {
                    return emit_serialization_error(tool, elapsed, format, error);
                }
            }
            err.exit_code
        }
    }
}

fn render<T: Serialize>(value: &T, format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Json => serde_json::to_string(value).map_err(|error| error.to_string()),
        OutputFormat::Toon => toon_format::encode_default(value).map_err(|error| error.to_string()),
    }
}

fn emit_serialization_error(tool: &str, elapsed: u64, format: OutputFormat, error: String) -> i32 {
    let envelope = ErrorEnvelope::new(
        ErrorPayload::new(
            "serialization_error",
            format!("failed to serialize CLI envelope: {error}"),
            "report this bug",
            None,
        ),
        Meta::new(tool, elapsed),
    );
    match render(&envelope, format) {
        Ok(text) => println!("{text}"),
        Err(_) => println!(
            "{}",
            serde_json::to_string(&envelope)
                .expect("serialization error envelope should serialize")
        ),
    }
    1
}

fn normalize_error_code(code: &str) -> String {
    match code {
        "INTERNAL" | "INTERNAL_ERROR" => return "internal_error".to_owned(),
        "RUNTIME_ERROR" => return "runtime_error".to_owned(),
        "SERIALIZATION_ERROR" => return "serialization_error".to_owned(),
        _ => {}
    }

    let mut normalized = String::with_capacity(code.len());
    let mut previous_was_underscore = false;
    for ch in code.chars() {
        if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !previous_was_underscore && !normalized.is_empty() {
                normalized.push('_');
                previous_was_underscore = true;
            }
        } else {
            normalized.extend(ch.to_lowercase());
            previous_was_underscore = false;
        }
    }
    normalized.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ErrorEnvelope, ErrorPayload, Meta, SuccessEnvelope};

    #[test]
    fn success_envelope_serializes_contract_keys() {
        let envelope = SuccessEnvelope::new(
            json!({ "status": "ready" }),
            Meta::new("health", 13)
                .with_count(Some(3))
                .with_total(Some(3))
                .with_has_more(Some(false)),
        );

        let value = serde_json::to_value(envelope).expect("serialize success envelope");
        assert_eq!(value["ok"], json!(true));
        assert_eq!(value["data"]["status"], json!("ready"));
        assert_eq!(value["meta"]["tool"], json!("health"));
        assert_eq!(value["meta"]["elapsed"], json!(13));
        assert_eq!(value["meta"]["count"], json!(3));
        assert_eq!(value["meta"]["total"], json!(3));
        assert_eq!(value["meta"]["hasMore"], json!(false));
    }

    #[test]
    fn error_envelope_serializes_contract_keys() {
        let envelope = ErrorEnvelope::new(
            ErrorPayload::new(
                "NO_CONFIG",
                "missing config",
                "create let.config.toml",
                None,
            ),
            Meta::new("config.validate", 8),
        );
        let value = serde_json::to_value(envelope).expect("serialize error envelope");

        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["error"]["code"], json!("no_config"));
        assert_eq!(value["error"]["message"], json!("missing config"));
        assert_eq!(value["error"]["hint"], json!("create let.config.toml"));
        assert_eq!(value["meta"]["tool"], json!("config.validate"));
        assert_eq!(value["meta"]["elapsed"], json!(8));
        assert!(value["meta"].get("count").is_none());
    }
}
