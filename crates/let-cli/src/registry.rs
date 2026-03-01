#![forbid(unsafe_code)]

use std::sync::OnceLock;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolParameter {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub param_type: &'static str,
    pub required: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMetadata {
    pub name: &'static str,
    pub command: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub parameters: Vec<ToolParameter>,
    pub output_fields: Vec<&'static str>,
    pub output_schema: Option<&'static str>,
    pub input_schema: Option<&'static str>,
    pub idempotent: bool,
    pub rate_limit: Option<&'static str>,
    pub example: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalFlag {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub flag_type: &'static str,
    pub description: &'static str,
}

static TOOL_REGISTRY: OnceLock<Vec<ToolMetadata>> = OnceLock::new();

pub fn tool_registry() -> &'static [ToolMetadata] {
    TOOL_REGISTRY
        .get_or_init(|| {
            let mut tools = vec![
                tool(
                    "assess.group",
                    "let assess",
                    "placeholder",
                    "Delegated command group for assessment actions.",
                    vec![],
                    vec![],
                    true,
                    "let assess",
                ),
                tool(
                    "build.group",
                    "let build",
                    "build",
                    "Build source database commands.",
                    vec![],
                    vec![],
                    true,
                    "let build",
                ),
                tool(
                    "build.sources",
                    "let build sources <target>",
                    "build",
                    "Build one or all source databases.",
                    vec![
                        ToolParameter {
                            name: "target",
                            param_type: "string",
                            required: true,
                            description:
                                "One of list|all|broadband|postcodes|deprivation|census|population|income|flood|naptan|uprn|crime.",
                        },
                        ToolParameter {
                            name: "--jobs",
                            param_type: "number",
                            required: false,
                            description: "Parallel jobs for `all` target (default: 3).",
                        },
                    ],
                    vec!["target", "jobs", "durationMs", "status"],
                    false,
                    "let build sources all --jobs 3 --json",
                ),
                tool(
                    "export.group",
                    "let export",
                    "placeholder",
                    "Delegated command group for export actions.",
                    vec![],
                    vec![],
                    true,
                    "let export",
                ),
                tool(
                    "fetch.group",
                    "let fetch",
                    "placeholder",
                    "Delegated command group for fetch actions.",
                    vec![],
                    vec![],
                    true,
                    "let fetch",
                ),
                tool(
                    "ops.group",
                    "let ops",
                    "placeholder",
                    "Delegated command group for operations actions.",
                    vec![],
                    vec![],
                    true,
                    "let ops",
                ),
                tool(
                    "score.group",
                    "let score",
                    "placeholder",
                    "Delegated command group for scoring actions.",
                    vec![],
                    vec![],
                    true,
                    "let score",
                ),
                tool(
                    "search.group",
                    "let search",
                    "placeholder",
                    "Delegated command group for search actions.",
                    vec![],
                    vec![],
                    true,
                    "let search",
                ),
                tool(
                    "view.group",
                    "let view",
                    "placeholder",
                    "Delegated command group for view actions.",
                    vec![],
                    vec![],
                    true,
                    "let view",
                ),
                tool(
                    "config.show",
                    "let config show",
                    "read",
                    "Load and print parsed config from let.config.toml.",
                    vec![],
                    vec!["path", "config"],
                    true,
                    "let config show --json",
                ),
                tool(
                    "health",
                    "let health",
                    "infra",
                    "Run readiness checks and report lifecycle status.",
                    vec![],
                    vec!["status", "checks", "paths"],
                    true,
                    "let health --json",
                ),
                tool(
                    "tools",
                    "let tools [name]",
                    "infra",
                    "List all available tools or return one tool metadata record.",
                    vec![ToolParameter {
                        name: "name",
                        param_type: "string",
                        required: false,
                        description: "Tool name for detail mode.",
                    }],
                    vec!["tools", "globalFlags"],
                    true,
                    "let tools --json",
                ),
                tool(
                    "start",
                    "let start",
                    "runtime",
                    "Launch the Ratatui terminal UI.",
                    vec![],
                    vec!["status", "code"],
                    false,
                    "let start --json",
                ),
                tool(
                    "config.validate",
                    "let config validate",
                    "validate",
                    "Validate config file and return structured result.",
                    vec![],
                    vec!["path", "valid"],
                    true,
                    "let config validate --json",
                ),
            ];
            tools.sort_by(|a, b| a.category.cmp(b.category).then_with(|| a.name.cmp(b.name)));
            tools
        })
        .as_slice()
}

pub fn find_tool(name: &str) -> Option<&'static ToolMetadata> {
    tool_registry().iter().find(|tool| tool.name == name)
}

pub fn global_flags() -> &'static [GlobalFlag] {
    static FLAGS: [GlobalFlag; 5] = [
        GlobalFlag {
            name: "--json",
            flag_type: "bool",
            description: "Emit exactly one JSON envelope object to stdout.",
        },
        GlobalFlag {
            name: "--data-dir",
            flag_type: "path",
            description: "Override data directory.",
        },
        GlobalFlag {
            name: "--config-dir",
            flag_type: "path",
            description: "Override config directory.",
        },
        GlobalFlag {
            name: "--cache-dir",
            flag_type: "path",
            description: "Override cache directory.",
        },
        GlobalFlag {
            name: "--sources-dir",
            flag_type: "path",
            description: "Override sources directory.",
        },
    ];
    &FLAGS
}

fn tool(
    name: &'static str,
    command: &'static str,
    category: &'static str,
    description: &'static str,
    parameters: Vec<ToolParameter>,
    output_fields: Vec<&'static str>,
    idempotent: bool,
    example: &'static str,
) -> ToolMetadata {
    ToolMetadata {
        name,
        command,
        category,
        description,
        parameters,
        output_fields,
        output_schema: None,
        input_schema: None,
        idempotent,
        rate_limit: None,
        example,
    }
}
