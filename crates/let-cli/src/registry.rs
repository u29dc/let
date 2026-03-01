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
                        ToolParameter {
                            name: "--progress",
                            param_type: "string",
                            required: false,
                            description: "Progress mode: auto|plain|off (default: auto).",
                        },
                    ],
                    vec!["target", "jobs", "durationMs", "status"],
                    false,
                    "let build sources all --jobs 3 --progress auto",
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
                    "export",
                    "Export listings to Notion with optional filtering.",
                    vec![
                        ToolParameter {
                            name: "--top",
                            param_type: "number",
                            required: false,
                            description: "Export top N listings ranked by score.",
                        },
                        ToolParameter {
                            name: "--min-score",
                            param_type: "number",
                            required: false,
                            description: "Only export listings at or above score threshold.",
                        },
                        ToolParameter {
                            name: "--region",
                            param_type: "string",
                            required: false,
                            description: "Only export listings matching region text.",
                        },
                        ToolParameter {
                            name: "--dry-run",
                            param_type: "bool",
                            required: false,
                            description: "Preview export selection without writing to Notion.",
                        },
                        ToolParameter {
                            name: "--force",
                            param_type: "bool",
                            required: false,
                            description: "Update existing Notion pages instead of skipping them.",
                        },
                    ],
                    vec!["created", "updated", "skipped", "failed", "total", "dryRun"],
                    false,
                    "let export notion --json",
                ),
                tool(
                    "fetch",
                    "let fetch",
                    "fetch",
                    "Fetch listings by portal ids, parse, score, and persist them.",
                    vec![
                        ToolParameter {
                            name: "ids",
                            param_type: "string",
                            required: true,
                            description: "Comma-separated Rightmove portal ids.",
                        },
                        ToolParameter {
                            name: "--region",
                            param_type: "string",
                            required: false,
                            description: "Region override for fetched listings.",
                        },
                        ToolParameter {
                            name: "--skip-images",
                            param_type: "bool",
                            required: false,
                            description: "Skip image extraction output.",
                        },
                        ToolParameter {
                            name: "--skip-epc",
                            param_type: "bool",
                            required: false,
                            description: "Accepted for parity with legacy fetch options.",
                        },
                    ],
                    vec!["fetched", "failed", "total", "saveError"],
                    false,
                    "let fetch 170448131,170448132 --json",
                ),
                tool(
                    "ops.group",
                    "let ops",
                    "ops",
                    "Operational maintenance command group.",
                    vec![],
                    vec![],
                    true,
                    "let ops",
                ),
                tool(
                    "ops.prune",
                    "let ops prune",
                    "ops",
                    "Remove listings by score, region, or inactive status. Selector rules: default is score < 50; --region can combine with --min-score/--bottom; --inactive only with optional --region; --bottom and --min-score are mutually exclusive.",
                    vec![
                        ToolParameter {
                            name: "--min-score",
                            param_type: "number",
                            required: false,
                            description:
                                "Prune listings below score threshold. Used as default selector (50) when no selector flags are set.",
                        },
                        ToolParameter {
                            name: "--bottom",
                            param_type: "number",
                            required: false,
                            description:
                                "Prune bottom N percent by score (1-100). Cannot be combined with --min-score.",
                        },
                        ToolParameter {
                            name: "--region",
                            param_type: "string",
                            required: false,
                            description:
                                "Limit prune selection to region patterns, or prune full region when used alone.",
                        },
                        ToolParameter {
                            name: "--inactive",
                            param_type: "bool",
                            required: false,
                            description:
                                "Prune inactive listings only. Can be combined with --region but not score selectors.",
                        },
                        ToolParameter {
                            name: "--dry-run",
                            param_type: "bool",
                            required: false,
                            description: "Preview prune results without mutating data.",
                        },
                        ToolParameter {
                            name: "--force",
                            param_type: "bool",
                            required: false,
                            description: "Skip confirmation prompt in text mode.",
                        },
                    ],
                    vec!["removed", "remaining", "mode", "dryRun"],
                    false,
                    "let ops prune --inactive --dry-run --json",
                ),
                tool(
                    "ops.patch",
                    "let ops patch <id>",
                    "ops",
                    "Override listing fields and recompute scores.",
                    vec![
                        ToolParameter {
                            name: "id",
                            param_type: "string",
                            required: true,
                            description: "Listing UUID or portal id.",
                        },
                        ToolParameter {
                            name: "--address",
                            param_type: "string",
                            required: false,
                            description: "Override listing address.",
                        },
                        ToolParameter {
                            name: "--postcode",
                            param_type: "string",
                            required: false,
                            description: "Override listing postcode.",
                        },
                        ToolParameter {
                            name: "--lat",
                            param_type: "number",
                            required: false,
                            description: "Override latitude (requires --lng).",
                        },
                        ToolParameter {
                            name: "--lng",
                            param_type: "number",
                            required: false,
                            description: "Override longitude (requires --lat).",
                        },
                        ToolParameter {
                            name: "--epc-rating",
                            param_type: "string",
                            required: false,
                            description: "Override EPC rating (A-G).",
                        },
                        ToolParameter {
                            name: "--floor-area",
                            param_type: "number",
                            required: false,
                            description: "Override floor area (sqm).",
                        },
                        ToolParameter {
                            name: "--skip-re-enrich",
                            param_type: "bool",
                            required: false,
                            description: "Accepted for compatibility; enrichment reruns are deferred.",
                        },
                        ToolParameter {
                            name: "--skip-images",
                            param_type: "bool",
                            required: false,
                            description: "Accepted for compatibility.",
                        },
                    ],
                    vec!["id", "applied", "reEnriched", "rescored", "previousScore", "newScore"],
                    false,
                    "let ops patch 172223234 --postcode SY2\\ 5WP --json",
                ),
                tool(
                    "ops.verify",
                    "let ops verify",
                    "ops",
                    "Verify listing availability status via portal checks.",
                    vec![
                        ToolParameter {
                            name: "--dry-run",
                            param_type: "bool",
                            required: false,
                            description: "Preview without writing inactive statuses.",
                        },
                        ToolParameter {
                            name: "--region",
                            param_type: "string",
                            required: false,
                            description: "Only verify matching region names.",
                        },
                        ToolParameter {
                            name: "--limit",
                            param_type: "number",
                            required: false,
                            description: "Maximum listings to verify.",
                        },
                        ToolParameter {
                            name: "--delay",
                            param_type: "number",
                            required: false,
                            description: "Delay in milliseconds between requests.",
                        },
                    ],
                    vec!["checked", "active", "inactive", "errors", "dryRun", "results"],
                    false,
                    "let ops verify --dry-run --limit 10 --json",
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
                    "search",
                    "Search discovery command group.",
                    vec![],
                    vec![],
                    true,
                    "let search",
                ),
                tool(
                    "search.diff",
                    "let search diff <ids>",
                    "search",
                    "Partition ids into new versus already known listings.",
                    vec![ToolParameter {
                        name: "ids",
                        param_type: "string",
                        required: true,
                        description: "Comma-separated portal ids.",
                    }],
                    vec!["new", "known", "total"],
                    true,
                    "let search diff 170448131,170448132 --json",
                ),
                tool(
                    "search.resolve",
                    "let search resolve <location>",
                    "search",
                    "Resolve text location to Rightmove location identifiers.",
                    vec![ToolParameter {
                        name: "location",
                        param_type: "string",
                        required: true,
                        description: "City or area name.",
                    }],
                    vec!["query", "locations"],
                    true,
                    "let search resolve York --json",
                ),
                tool(
                    "search.discover",
                    "let search discover",
                    "search",
                    "Discover listing ids from configured search locations.",
                    vec![
                        ToolParameter {
                            name: "--region",
                            param_type: "string",
                            required: false,
                            description: "Filter configured locations by name.",
                        },
                        ToolParameter {
                            name: "--location",
                            param_type: "string",
                            required: false,
                            description: "Ad-hoc locationIdentifier value.",
                        },
                        ToolParameter {
                            name: "--property-types",
                            param_type: "string",
                            required: false,
                            description: "Comma-separated property types override.",
                        },
                        ToolParameter {
                            name: "--must-have",
                            param_type: "string",
                            required: false,
                            description: "Comma-separated must-have override or none.",
                        },
                        ToolParameter {
                            name: "--dont-show",
                            param_type: "string",
                            required: false,
                            description: "Comma-separated exclusions override or none.",
                        },
                        ToolParameter {
                            name: "--location-name",
                            param_type: "string",
                            required: false,
                            description: "Display label for ad-hoc location.",
                        },
                        ToolParameter {
                            name: "--limit",
                            param_type: "number",
                            required: false,
                            description: "Max listings per location.",
                        },
                    ],
                    vec!["ids", "idsByLocation", "total", "locations"],
                    true,
                    "let search discover --json",
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

#[allow(clippy::too_many_arguments)]
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
