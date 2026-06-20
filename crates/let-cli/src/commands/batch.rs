#![forbid(unsafe_code)]

use std::io::{IsTerminal, Read};
use std::time::Duration;

use let_sdk::intelligence::EvidenceBundle;
use serde::Serialize;
use serde_json::Value;

use crate::commands::CommandError;
use crate::envelope::ErrorPayload;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResponse {
    pub items: Vec<BatchItem>,
    pub count: usize,
    pub ok_count: usize,
    pub error_count: usize,
}

impl BatchResponse {
    pub fn new(items: Vec<BatchItem>) -> Self {
        let count = items.len();
        let ok_count = items.iter().filter(|item| item.ok).count();
        let error_count = count.saturating_sub(ok_count);
        Self {
            items,
            count,
            ok_count,
            error_count,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub input: String,
    pub id: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
    pub elapsed: String,
    pub warnings: Vec<String>,
}

impl BatchItem {
    pub fn success(
        input: String,
        id: String,
        bundle: Value,
        elapsed: Duration,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            input,
            id: Some(id),
            ok: true,
            bundle: Some(bundle),
            error: None,
            elapsed: format_elapsed(elapsed),
            warnings,
        }
    }

    pub fn failure(input: String, error: CommandError, elapsed: Duration) -> Self {
        Self {
            input,
            id: None,
            ok: false,
            bundle: None,
            error: Some(ErrorPayload::new(
                error.code,
                error.message,
                error.hint,
                error.details,
            )),
            elapsed: format_elapsed(elapsed),
            warnings: Vec::new(),
        }
    }
}

pub fn resolve_inputs(
    positionals: &[String],
    command: &str,
    value_name: &str,
) -> Result<Vec<String>, CommandError> {
    if !positionals.is_empty() {
        return Ok(split_inputs(&positionals.join("\n")));
    }

    let mut input = String::new();
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        stdin.lock().read_to_string(&mut input).map_err(|error| {
            CommandError::runtime(
                "RUNTIME_ERROR",
                format!("failed to read stdin: {error}"),
                "retry with explicit positional inputs",
            )
        })?;
    }

    let values = split_inputs(&input);
    if values.is_empty() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("missing input for `let {command}`"),
            format!("pass one or more {value_name} values or pipe them on stdin"),
        ));
    }
    Ok(values)
}

pub fn bundle_warnings(bundle: &EvidenceBundle) -> Vec<String> {
    let mut warnings = Vec::new();
    for flag in &bundle.flags {
        if !warnings.contains(&flag.summary) {
            warnings.push(flag.summary.clone());
        }
    }
    for section in bundle.sections.values() {
        for warning in &section.warnings {
            if !warnings.contains(warning) {
                warnings.push(warning.clone());
            }
        }
    }
    warnings
}

fn split_inputs(input: &str) -> Vec<String> {
    input
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter_map(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        })
        .collect()
}

fn format_elapsed(elapsed: Duration) -> String {
    format!("{:.2}s", elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::{resolve_inputs, split_inputs};

    #[test]
    fn split_inputs_accepts_whitespace_and_commas() {
        assert_eq!(
            split_inputs("170448131, 170448132\n170448133\t"),
            vec!["170448131", "170448132", "170448133"]
        );
    }

    #[test]
    fn resolve_inputs_splits_collapsed_positionals() {
        assert_eq!(
            resolve_inputs(
                &["170448131 170448132,170448133".to_owned()],
                "inspect",
                "listing ids"
            )
            .expect("positionals resolve"),
            vec!["170448131", "170448132", "170448133"]
        );
    }
}
