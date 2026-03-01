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
                    "assess",
                    "Assessment workflow command group.",
                    vec![],
                    vec![],
                    true,
                    "let assess",
                ),
                tool(
                    "assess.candidates",
                    "let assess candidates",
                    "assess",
                    "List unassessed active listings ranked by score.",
                    vec![
                        ToolParameter {
                            name: "--top",
                            param_type: "number",
                            required: false,
                            description: "Maximum candidates to return (default: 10).",
                        },
                        ToolParameter {
                            name: "--region",
                            param_type: "string",
                            required: false,
                            description: "Filter by region name.",
                        },
                        ToolParameter {
                            name: "--min-score",
                            param_type: "number",
                            required: false,
                            description: "Minimum algorithm score threshold.",
                        },
                    ],
                    vec!["candidates", "total", "assessed", "remaining"],
                    true,
                    "let assess candidates --top 10 --json",
                ),
                tool(
                    "assess.context",
                    "let assess context <id>",
                    "assess",
                    "Return context bundle for assessment work on a listing.",
                    vec![ToolParameter {
                        name: "id",
                        param_type: "string",
                        required: true,
                        description: "Listing UUID or portal identifier.",
                    }],
                    vec![
                        "listing",
                        "scoreBreakdown",
                        "assessmentSchema",
                        "media",
                        "links",
                        "description",
                        "notes",
                    ],
                    true,
                    "let assess context 170448131 --json",
                ),
                tool(
                    "assess.submit",
                    "let assess submit <id> <assessment>",
                    "assess",
                    "Validate and persist a listing assessment payload.",
                    vec![
                        ToolParameter {
                            name: "id",
                            param_type: "string",
                            required: true,
                            description: "Listing UUID or portal identifier.",
                        },
                        ToolParameter {
                            name: "assessment",
                            param_type: "json",
                            required: true,
                            description: "Assessment JSON payload.",
                        },
                    ],
                    vec!["id", "assessedScore", "algoScore", "scoreAdjustment"],
                    false,
                    "let assess submit 170448131 '{\"maintenance\":\"good\",...}' --json",
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
                    "export",
                    "Export command group.",
                    vec![],
                    vec![],
                    true,
                    "let export",
                ),
                tool(
                    "export.json",
                    "let export json",
                    "export",
                    "Export listings database to JSON file.",
                    vec![ToolParameter {
                        name: "--output",
                        param_type: "path",
                        required: false,
                        description: "Output path; defaults to data/let.db.json.",
                    }],
                    vec!["path", "count"],
                    true,
                    "let export json --output /tmp/let-export.json --json",
                ),
                tool(
                    "export.notion",
                    "let export notion",
                    "placeholder",
                    "Delegated export command for Notion sync.",
                    vec![],
                    vec![],
                    false,
                    "let export notion --json",
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
                    "Score command group.",
                    vec![],
                    vec![],
                    true,
                    "let score",
                ),
                tool(
                    "score.compute",
                    "let score compute",
                    "score",
                    "Recompute scores for all listings in database.",
                    vec![],
                    vec!["total", "scored", "avgScore", "avgConfidence"],
                    false,
                    "let score compute --json",
                ),
                tool(
                    "score.explain",
                    "let score explain <id>",
                    "score",
                    "Return factor-level score breakdown for one listing.",
                    vec![ToolParameter {
                        name: "id",
                        param_type: "string",
                        required: true,
                        description: "Listing UUID or portal identifier.",
                    }],
                    vec!["id", "overall", "assessedScore", "confidence", "composites", "penalties"],
                    true,
                    "let score explain 170448131 --json",
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
                    "view",
                    "Read-only listing view command group.",
                    vec![],
                    vec![],
                    true,
                    "let view",
                ),
                tool(
                    "view.list",
                    "let view list",
                    "view",
                    "Ranked listing table with filters and sorting.",
                    vec![
                        ToolParameter {
                            name: "--top",
                            param_type: "number",
                            required: false,
                            description: "Maximum rows to return (default: 20).",
                        },
                        ToolParameter {
                            name: "--min-score",
                            param_type: "number",
                            required: false,
                            description: "Minimum algorithm score threshold.",
                        },
                        ToolParameter {
                            name: "--sort",
                            param_type: "string",
                            required: false,
                            description: "One of score|price|bedrooms|date.",
                        },
                        ToolParameter {
                            name: "--asc",
                            param_type: "bool",
                            required: false,
                            description: "Sort ascending (default is descending).",
                        },
                        ToolParameter {
                            name: "--region",
                            param_type: "string",
                            required: false,
                            description: "Region filter.",
                        },
                        ToolParameter {
                            name: "--type",
                            param_type: "string",
                            required: false,
                            description: "Comma-separated property type filter.",
                        },
                    ],
                    vec!["listings", "total", "filtered"],
                    true,
                    "let view list --top 10 --region Sheffield --json",
                ),
                tool(
                    "view.detail",
                    "let view detail <id>",
                    "view",
                    "Full listing detail by UUID or portal id.",
                    vec![ToolParameter {
                        name: "id",
                        param_type: "string",
                        required: true,
                        description: "Listing UUID or portal identifier.",
                    }],
                    vec!["listing"],
                    true,
                    "let view detail 170448131 --json",
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
