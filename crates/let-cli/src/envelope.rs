#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

use crate::commands::ErrorDetail;

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
            code: code.into(),
            message: message.into(),
            hint: hint.into(),
            details,
        }
    }
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
        assert_eq!(value["error"]["code"], json!("NO_CONFIG"));
        assert_eq!(value["error"]["message"], json!("missing config"));
        assert_eq!(value["error"]["hint"], json!("create let.config.toml"));
        assert_eq!(value["meta"]["tool"], json!("config.validate"));
        assert_eq!(value["meta"]["elapsed"], json!(8));
        assert!(value["meta"].get("count").is_none());
    }
}
