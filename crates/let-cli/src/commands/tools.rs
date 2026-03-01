#![forbid(unsafe_code)]

use serde::Serialize;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult};
use crate::registry::{GlobalFlag, ToolMetadata, find_tool, global_flags, tool_registry};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsCatalog<'a> {
    pub global_flags: &'a [GlobalFlag],
    pub tools: &'a [ToolMetadata],
}

pub fn run(name: Option<&str>) -> CommandResult {
    match name {
        Some(tool_name) => detail(tool_name),
        None => catalog(),
    }
}

fn catalog() -> CommandResult {
    let payload = ToolsCatalog {
        global_flags: global_flags(),
        tools: tool_registry(),
    };
    let tool_count = payload.tools.len();
    let data = serde_json::to_value(payload).expect("tools catalog serialization failed");

    Ok(CommandOutput::new(data)
        .with_count(tool_count)
        .with_total(tool_count)
        .with_has_more(false)
        .with_text(format!("{} tools available", tool_count)))
}

fn detail(name: &str) -> CommandResult {
    let Some(tool) = find_tool(name) else {
        return Err(CommandError::runtime(
            "NOT_FOUND",
            format!("tool `{name}` not found"),
            "run `let tools --json` to list valid tool names",
        ));
    };

    let data = json!({ "tool": tool });
    Ok(CommandOutput::new(data).with_text(format!("{} -> {}", tool.name, tool.command)))
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn catalog_output_has_required_shape() {
        let output = run(None).expect("catalog should succeed");
        let data = output.data;

        assert!(data.get("globalFlags").is_some());
        assert!(data.get("tools").is_some());

        let tools = data["tools"].as_array().expect("tools array");
        assert!(!tools.is_empty(), "tools catalog should not be empty");

        let first = &tools[0];
        assert_key(first, "name");
        assert_key(first, "command");
        assert_key(first, "category");
        assert_key(first, "description");
        assert_key(first, "parameters");
        assert_key(first, "outputFields");
        assert_key(first, "outputSchema");
        assert_key(first, "inputSchema");
        assert_key(first, "idempotent");
        assert_key(first, "rateLimit");
        assert_key(first, "example");
    }

    #[test]
    fn detail_output_wraps_tool_object() {
        let output = run(Some("tools")).expect("detail should succeed");
        let data = output.data;

        assert!(data.get("tool").is_some());
        assert_eq!(
            data["tool"]["name"],
            serde_json::Value::String("tools".to_string())
        );
        assert!(data["tool"]["command"].is_string());
    }

    fn assert_key(value: &serde_json::Value, key: &str) {
        assert!(
            value.get(key).is_some(),
            "missing key `{}` in value {}",
            key,
            value
        );
    }
}
