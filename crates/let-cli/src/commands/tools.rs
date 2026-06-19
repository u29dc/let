#![forbid(unsafe_code)]

use serde::Serialize;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult};
use crate::registry::{GlobalFlag, ToolMetadata, find_tool, global_flags, tool_registry};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsCatalog<'a> {
    pub version: &'static str,
    pub global_flags: &'a [GlobalFlag],
    pub output_formats: [&'static str; 2],
    pub default_output_format: &'static str,
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
        version: env!("CARGO_PKG_VERSION"),
        global_flags: global_flags(),
        output_formats: ["json", "toon"],
        default_output_format: "json",
        tools: tool_registry(),
    };
    let tool_count = payload.tools.len();
    let data = serde_json::to_value(payload).expect("tools catalog serialization failed");

    Ok(CommandOutput::new(data)
        .with_count(tool_count)
        .with_total(tool_count)
        .with_has_more(false))
}

fn detail(name: &str) -> CommandResult {
    let Some(tool) = find_tool(name) else {
        return Err(CommandError::runtime(
            "NOT_FOUND",
            format!("tool `{name}` not found"),
            "run `let tools` to list valid tool names",
        ));
    };

    let data = json!({ "tool": tool });
    Ok(CommandOutput::new(data))
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn catalog_output_has_required_shape() {
        let output = run(None).expect("catalog should succeed");
        let data = output.data;

        assert!(data.get("version").is_some());
        assert!(data.get("globalFlags").is_some());
        assert!(data.get("tools").is_some());
        assert_eq!(data["outputFormats"], serde_json::json!(["json", "toon"]));
        assert_eq!(data["defaultOutputFormat"], serde_json::json!("json"));
        assert_eq!(
            data["version"],
            serde_json::Value::String(env!("CARGO_PKG_VERSION").to_owned())
        );

        let global_flags = data["globalFlags"].as_array().expect("global flags array");
        assert!(
            global_flags
                .iter()
                .any(|flag| flag["name"] == serde_json::Value::String("--toon".to_owned())),
            "expected --toon global flag in catalog"
        );
        assert!(
            global_flags
                .iter()
                .any(|flag| flag["name"] == serde_json::Value::String("--profile".to_owned())),
            "expected --profile global flag in catalog"
        );
        assert!(
            !global_flags
                .iter()
                .any(|flag| flag["name"] == serde_json::Value::String("--text".to_owned())),
            "did not expect --text global flag in catalog"
        );

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
