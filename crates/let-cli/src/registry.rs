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
                    "assess.get",
                    "let assess get <id>",
                    "assess",
                    "Read an AI-authored assessment for a listing.",
                    vec![param("id", "string", true, "Rightmove id or entity id.")],
                    vec!["entityId", "assessment", "normalizedAssessment", "savedAt"],
                    ASSESS_GET_INPUT,
                    ASSESS_RECORD_OUTPUT,
                    true,
                    None,
                    "let assess get 170448131",
                ),
                tool(
                    "assess.list",
                    "let assess list",
                    "assess",
                    "List saved AI-authored assessments with optional listing summary filters.",
                    vec![
                        param("--recommendation", "string", false, "Saved assessment recommendation."),
                        param("--confidence", "string", false, "Saved assessment confidence."),
                        param("--area", "string", false, "Area, address, postcode, or assessment area text."),
                        param("--max-price", "number", false, "Maximum monthly rent."),
                        param("--postcode-prefix", "string", false, "Postcode prefix such as M1 or YO1."),
                    ],
                    vec!["assessments", "filters"],
                    LIST_FILTERS_INPUT,
                    ASSESS_LIST_OUTPUT,
                    true,
                    None,
                    "let assess list --recommendation view --max-price 1500",
                ),
                tool(
                    "assess.save",
                    "let assess save <id> <assessment>",
                    "assess",
                    "Persist an AI-authored assessment JSON object without applying a built-in score rubric.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("assessment", "json", true, "AI-authored assessment object."),
                    ],
                    vec!["entityId", "assessment", "normalizedAssessment", "savedAt"],
                    ASSESS_SAVE_INPUT,
                    ASSESS_RECORD_OUTPUT,
                    false,
                    None,
                    "let assess save 170448131 '{\"recommendation\":\"view\",\"reasoning\":\"...\"}'",
                ),
                tool(
                    "score.compute",
                    "let score compute <id>",
                    "score",
                    "Compute and persist a deterministic calibrated score for a stored evidence bundle; judgment calibration is read only from saved assessment JSON.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("--scorecard", "string", false, "Configured scorecard id; defaults to default."),
                    ],
                    vec!["score"],
                    SCORE_COMPUTE_INPUT,
                    SCORE_COMPUTE_OUTPUT,
                    false,
                    None,
                    "let score compute 170448131 --scorecard default",
                ),
                tool(
                    "score.get",
                    "let score get <id>",
                    "score",
                    "Read the latest persisted deterministic calibrated score for a listing and scorecard.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("--scorecard", "string", false, "Configured scorecard id; defaults to default."),
                    ],
                    vec![
                        "entityId",
                        "rightmoveId",
                        "scorecard",
                        "baseOverall",
                        "overall",
                        "judgment",
                        "band",
                        "confidence",
                        "domains",
                        "summary",
                    ],
                    SCORE_GET_INPUT,
                    SCORE_RESULT_OUTPUT,
                    true,
                    None,
                    "let score get 170448131",
                ),
                tool(
                    "score.list",
                    "let score list",
                    "score",
                    "List persisted score summaries, ordered by final calibrated score within each scorecard.",
                    vec![param("--scorecard", "string", false, "Optional scorecard id filter.")],
                    vec!["scores", "scorecardId"],
                    SCORE_LIST_INPUT,
                    SCORE_LIST_OUTPUT,
                    true,
                    None,
                    "let score list --scorecard default",
                ),
                tool(
                    "scorecards.list",
                    "let scorecards list",
                    "score",
                    "List resolved scorecard configurations after applying config overrides.",
                    vec![],
                    vec!["scorecards", "defaultScorecard"],
                    EMPTY_INPUT,
                    SCORECARDS_OUTPUT,
                    true,
                    None,
                    "let scorecards list",
                ),
                tool(
                    "scorecards.validate",
                    "let scorecards validate",
                    "score",
                    "Validate scorecard configuration and return the resolved scorecards.",
                    vec![],
                    vec!["status", "scorecards", "defaultScorecard"],
                    EMPTY_INPUT,
                    SCORECARDS_VALIDATE_OUTPUT,
                    true,
                    None,
                    "let scorecards validate",
                ),
                tool(
                    "config.show",
                    "let config show [--profile <name>]",
                    "infra",
                    "Load and print parsed configuration.",
                    vec![param(
                        "--profile",
                        "string",
                        false,
                        "Config profile name from profiles/<name>.toml.",
                    )],
                    vec!["path", "profile", "config"],
                    CONFIG_SHOW_INPUT,
                    CONFIG_OUTPUT,
                    true,
                    None,
                    "let config show --profile north",
                ),
                tool(
                    "config.profiles",
                    "let config profiles",
                    "infra",
                    "List available config profile files.",
                    vec![],
                    vec!["profileDir", "profiles"],
                    EMPTY_INPUT,
                    CONFIG_PROFILES_OUTPUT,
                    true,
                    None,
                    "let config profiles",
                ),
                tool(
                    "area.postcode",
                    "let area postcode <postcode>",
                    "area",
                    "Read local-source area facts for a postcode without requiring a listing.",
                    vec![
                        param("postcode", "string", true, "UK postcode to inspect."),
                        param("--radius-m", "number", false, "Nearby lookup radius in metres."),
                        param("--limit", "number", false, "Maximum nearby stop/UPRN candidates."),
                    ],
                    vec![
                        "query",
                        "joinKeys",
                        "sections",
                        "facts",
                        "broadband",
                        "nearby",
                        "missingSources",
                        "nextActions",
                    ],
                    AREA_POSTCODE_INPUT,
                    AREA_POSTCODE_OUTPUT,
                    true,
                    None,
                    "let area postcode 'CT21 5QR' --radius-m 800 --limit 8",
                ),
                tool(
                    "correct.address",
                    "let correct address <id>",
                    "correct",
                    "Record a manual address, postcode, or coordinate correction without mutating source snapshots.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("--address", "string", false, "Corrected address text."),
                        param("--postcode", "string", false, "Corrected postcode."),
                        param("--lat", "number", false, "Corrected latitude; must be paired with --lng."),
                        param("--lng", "number", false, "Corrected longitude; must be paired with --lat."),
                        param("--note", "string", false, "Correction provenance note."),
                    ],
                    vec!["correction", "affectedSections", "warnings", "nextCommands"],
                    CORRECT_ADDRESS_INPUT,
                    CORRECTION_OUTPUT,
                    false,
                    None,
                    "let correct address 170448131 --postcode 'YO1 7HH' --note 'verified from agent brochure'",
                ),
                tool(
                    "correct.clear",
                    "let correct clear <id>",
                    "correct",
                    "Disable an active correction without deleting its audit record.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("--kind", "string", true, "address|epc|media."),
                        param("--correction-id", "string", true, "Exact correction id to disable."),
                    ],
                    vec!["correction", "affectedSections", "warnings", "nextCommands"],
                    CORRECT_CLEAR_INPUT,
                    CORRECTION_OUTPUT,
                    false,
                    None,
                    "let correct clear 170448131 --kind address --correction-id <id>",
                ),
                tool(
                    "correct.epc",
                    "let correct epc <id>",
                    "correct",
                    "Record a manual EPC certificate correction and pin EPC-derived evidence when inspected.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("--certificate-url", "string", false, "Exact EPC certificate URL."),
                        param("--lmk-key", "string", false, "EPC LMK key."),
                        param("--uprn", "string", false, "EPC UPRN."),
                        param("--rating", "string", false, "Known EPC rating."),
                        param("--floor-area-sqm", "number", false, "Known floor area in square metres."),
                        param("--note", "string", false, "Correction provenance note."),
                    ],
                    vec!["correction", "affectedSections", "warnings", "nextCommands"],
                    CORRECT_EPC_INPUT,
                    CORRECTION_OUTPUT,
                    false,
                    None,
                    "let correct epc 170448131 --lmk-key 1234 --rating C --note 'matched gov EPC page'",
                ),
                tool(
                    "correct.media",
                    "let correct media <id>",
                    "correct",
                    "Record manual media inputs, currently exact map coordinates for map regeneration.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("--map-lat", "number", true, "Corrected map latitude."),
                        param("--map-lng", "number", true, "Corrected map longitude."),
                        param("--note", "string", false, "Correction provenance note."),
                    ],
                    vec!["correction", "affectedSections", "warnings", "nextCommands"],
                    CORRECT_MEDIA_INPUT,
                    CORRECTION_OUTPUT,
                    false,
                    None,
                    "let correct media 170448131 --map-lat 51.501 --map-lng -0.142 --note 'manual map pin'",
                ),
                tool(
                    "evidence",
                    "let evidence [id ...]",
                    "inspect",
                    "Read one or more stored evidence bundles; reads stdin when ids are omitted.",
                    vec![
                        param("id", "string[]", false, "One or more Rightmove ids or entity ids. If omitted, ids are read from stdin."),
                        param("--section", "string[]", false, "Optional comma-separated evidence sections."),
                    ],
                    vec!["bundle", "requestedSections", "items", "count", "okCount", "errorCount"],
                    EVIDENCE_INPUT,
                    EVIDENCE_OUTPUT,
                    true,
                    None,
                    "let evidence 170448131 170448132 --section broadband,verifications",
                ),
                tool(
                    "evidence.list",
                    "let evidence list",
                    "inspect",
                    "List stored evidence bundles with stable listing summary fields and optional filters.",
                    vec![
                        param("--recommendation", "string", false, "Saved assessment recommendation."),
                        param("--confidence", "string", false, "Saved assessment confidence."),
                        param("--area", "string", false, "Area, address, postcode, or assessment area text."),
                        param("--max-price", "number", false, "Maximum monthly rent."),
                        param("--postcode-prefix", "string", false, "Postcode prefix such as M1 or YO1."),
                    ],
                    vec!["listings", "filters"],
                    LIST_FILTERS_INPUT,
                    EVIDENCE_LIST_OUTPUT,
                    true,
                    None,
                    "let evidence list --postcode-prefix M1 --recommendation view",
                ),
                tool(
                    "health",
                    "let health",
                    "infra",
                    "Run readiness checks for config, Rightmove search API mode, intelligence DB, source DBs, credentials, and writable paths.",
                    vec![],
                    vec!["status", "paths", "checks", "summary"],
                    EMPTY_INPUT,
                    HEALTH_OUTPUT,
                    true,
                    None,
                    "let health",
                ),
                tool(
                    "inspect",
                    "let inspect [id-or-url ...]",
                    "inspect",
                    "Gather Rightmove, address, source, claim, verification, and media evidence for one or more listings; reads stdin when ids are omitted.",
                    vec![
                        param("id-or-url", "string[]", false, "One or more Rightmove ids or listing URLs. If omitted, ids or URLs are read from stdin."),
                        param("--depth", "string", false, "quick|standard|deep."),
                        param("--refresh", "string", false, "none|stale|all."),
                        param("--section", "string[]", false, "Optional comma-separated evidence sections."),
                    ],
                    vec![
                        "entityId",
                        "rightmoveId",
                        "sections",
                        "rightmove",
                        "address",
                        "facts",
                        "claims",
                        "verifications",
                        "media",
                        "flags",
                        "items",
                        "count",
                        "okCount",
                        "errorCount",
                    ],
                    INSPECT_INPUT,
                    EVIDENCE_BUNDLE_OUTPUT,
                    false,
                    Some("Rightmove, Mapbox, EPC, and source-dataset limits apply."),
                    "let inspect 170448131 170448132 --depth standard",
                ),
                tool(
                    "search.discover",
                    "let search discover",
                    "search",
                    "Discover Rightmove listing ids and available listing card summaries from configured or ad-hoc locations.",
                    vec![
                        param("--region", "string", false, "Configured region name."),
                        param("--location", "string", false, "Rightmove location identifier."),
                        param("--min-price", "number", false, "Minimum monthly rent."),
                        param("--max-price", "number", false, "Maximum monthly rent."),
                        param("--min-bedrooms", "number", false, "Minimum bedrooms."),
                        param("--max-bedrooms", "number", false, "Maximum bedrooms."),
                        param("--radius", "number", false, "Rightmove search radius in miles."),
                        param("--include-let-agreed", "boolean", false, "Include let-agreed listings."),
                        param("--property-types", "string", false, "Comma-separated Rightmove property type ids."),
                        param("--must-have", "string", false, "Rightmove must-have filters."),
                        param("--dont-show", "string", false, "Rightmove exclusion filters."),
                        param("--location-name", "string", false, "Display name for ad-hoc location."),
                        param("--limit", "number", false, "Maximum ids to return."),
                    ],
                    vec![
                        "ids",
                        "idsByLocation",
                        "listings",
                        "locations",
                        "locationMatchesById",
                        "duplicateIds",
                        "duplicateLocationMatches",
                        "total",
                    ],
                    SEARCH_DISCOVER_INPUT,
                    SEARCH_DISCOVER_OUTPUT,
                    false,
                    Some("Rightmove search limits apply."),
                    "let search discover --region Manchester --limit 25",
                ),
                tool(
                    "search.market",
                    "let search market",
                    "search",
                    "Summarize rent distribution, property types, and duplicate location matches from discovery results.",
                    vec![
                        param("--region", "string", false, "Configured region name."),
                        param("--location", "string", false, "Rightmove location identifier."),
                        param("--min-price", "number", false, "Minimum monthly rent."),
                        param("--max-price", "number", false, "Maximum monthly rent."),
                        param("--min-bedrooms", "number", false, "Minimum bedrooms."),
                        param("--max-bedrooms", "number", false, "Maximum bedrooms."),
                        param("--radius", "number", false, "Rightmove search radius in miles."),
                        param("--include-let-agreed", "boolean", false, "Include let-agreed listings."),
                        param("--property-types", "string", false, "Comma-separated Rightmove property type ids."),
                        param("--must-have", "string", false, "Rightmove must-have filters."),
                        param("--dont-show", "string", false, "Rightmove exclusion filters."),
                        param("--location-name", "string", false, "Display name for ad-hoc location."),
                        param("--limit", "number", false, "Maximum cards to summarize per location."),
                    ],
                    vec!["market", "ids", "listings", "locations", "total"],
                    SEARCH_DISCOVER_INPUT,
                    SEARCH_MARKET_OUTPUT,
                    false,
                    Some("Rightmove search limits apply."),
                    "let search market --region Manchester --limit 50",
                ),
                tool(
                    "search.resolve",
                    "let search resolve <location>",
                    "search",
                    "Resolve a place name to Rightmove location identifiers.",
                    vec![param("location", "string", true, "City, area, or postcode search text.")],
                    vec!["location", "matches"],
                    SEARCH_RESOLVE_INPUT,
                    SEARCH_RESOLVE_OUTPUT,
                    false,
                    Some("Rightmove location lookup limits apply."),
                    "let search resolve Manchester",
                ),
                tool(
                    "sources.build",
                    "let sources build <target>",
                    "sources",
                    "Build one or all local enrichment source databases.",
                    vec![
                        param("target", "string", true, "all|broadband|postcodes|deprivation|census|population|income|flood|naptan|uprn|crime."),
                        param("--jobs", "number", false, "Parallel jobs for all target."),
                        param("--progress", "string", false, "auto|plain|off."),
                    ],
                    vec!["target", "jobs", "durationMs", "status", "sources"],
                    SOURCES_BUILD_INPUT,
                    SOURCES_BUILD_OUTPUT,
                    false,
                    Some("Public dataset downloads and checksums may apply."),
                    "let sources build broadband",
                ),
                tool(
                    "sources.list",
                    "let sources list",
                    "sources",
                    "List supported local enrichment source databases.",
                    vec![],
                    vec!["sources", "defaultJobs"],
                    EMPTY_INPUT,
                    SOURCES_LIST_OUTPUT,
                    true,
                    None,
                    "let sources list",
                ),
                tool(
                    "sources.status",
                    "let sources status",
                    "sources",
                    "Report which local enrichment source databases exist.",
                    vec![],
                    vec!["sources", "present", "missing"],
                    EMPTY_INPUT,
                    SOURCES_STATUS_OUTPUT,
                    true,
                    None,
                    "let sources status",
                ),
                tool(
                    "start",
                    "let start [--id <id>]",
                    "browse",
                    "Launch the local TUI against the current intelligence DB and media cache.",
                    vec![
                        param("--id", "string", false, "Optional Rightmove id or entity id to focus when supported by the TUI."),
                        param("--section", "string[]", false, "Optional comma-separated evidence sections to focus when supported by the TUI."),
                    ],
                    vec!["status", "code", "binary", "id", "sections"],
                    START_INPUT,
                    START_OUTPUT,
                    false,
                    None,
                    "let start --id 170448131",
                ),
                tool(
                    "tools",
                    "let tools [name]",
                    "infra",
                    "List all available tools or return one tool metadata record.",
                    vec![param("name", "string", false, "Tool name for detail mode.")],
                    vec!["version", "globalFlags", "outputFormats", "defaultOutputFormat", "tools"],
                    TOOLS_INPUT,
                    TOOLS_OUTPUT,
                    true,
                    None,
                    "let tools inspect",
                ),
                tool(
                    "verify",
                    "let verify <id>",
                    "inspect",
                    "Verify extracted claims against available evidence, optionally refreshing the bundle first.",
                    vec![
                        param("id", "string", true, "Rightmove id or entity id."),
                        param("--claim", "string", false, "all|address|broadband|epc|media|description."),
                        param("--refresh", "string", false, "none|stale|all."),
                    ],
                    vec!["id", "claim", "verifications", "sections"],
                    VERIFY_INPUT,
                    VERIFY_OUTPUT,
                    false,
                    Some("May fetch Rightmove and external source data when refresh is not none."),
                    "let verify 170448131 --claim broadband --refresh stale",
                ),
            ];
            tools.sort_by(|left, right| {
                left.category
                    .cmp(right.category)
                    .then_with(|| left.name.cmp(right.name))
            });
            tools
        })
        .as_slice()
}

pub fn find_tool(name: &str) -> Option<&'static ToolMetadata> {
    tool_registry().iter().find(|tool| tool.name == name)
}

pub fn global_flags() -> &'static [GlobalFlag] {
    static FLAGS: [GlobalFlag; 6] = [
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
        GlobalFlag {
            name: "--profile",
            flag_type: "string",
            description: "Select config profile from profiles/<name>.toml.",
        },
        GlobalFlag {
            name: "--toon",
            flag_type: "bool",
            description: "Emit Toon instead of the default JSON envelope.",
        },
    ];
    &FLAGS
}

fn param(
    name: &'static str,
    param_type: &'static str,
    required: bool,
    description: &'static str,
) -> ToolParameter {
    ToolParameter {
        name,
        param_type,
        required,
        description,
    }
}

#[allow(clippy::too_many_arguments)]
fn tool(
    name: &'static str,
    command: &'static str,
    category: &'static str,
    description: &'static str,
    parameters: Vec<ToolParameter>,
    output_fields: Vec<&'static str>,
    input_schema: &'static str,
    output_schema: &'static str,
    idempotent: bool,
    rate_limit: Option<&'static str>,
    example: &'static str,
) -> ToolMetadata {
    ToolMetadata {
        name,
        command,
        category,
        description,
        parameters,
        output_fields,
        output_schema: Some(output_schema),
        input_schema: Some(input_schema),
        idempotent,
        rate_limit,
        example,
    }
}

const EMPTY_INPUT: &str = r#"{"type":"object","additionalProperties":false,"properties":{}}"#;
const TOOLS_INPUT: &str =
    r#"{"type":"object","additionalProperties":false,"properties":{"name":{"type":"string"}}}"#;
const CONFIG_SHOW_INPUT: &str =
    r#"{"type":"object","additionalProperties":false,"properties":{"profile":{"type":"string"}}}"#;
const INSPECT_INPUT: &str = r#"{"type":"object","properties":{"idOrUrl":{"type":["string","array"],"items":{"type":"string"},"description":"one or more ids/URLs; stdin is read when omitted"},"depth":{"enum":["quick","standard","deep"]},"refresh":{"enum":["none","stale","all"]},"section":{"type":"array","items":{"enum":["rightmove","description","address","facts","claims","broadband","epc","media","verifications","assessment"]}}}}"#;
const EVIDENCE_INPUT: &str = r#"{"type":"object","properties":{"id":{"type":["string","array"],"items":{"type":"string"},"description":"one or more ids; stdin is read when omitted"},"section":{"type":"array","items":{"type":"string"}}}}"#;
const VERIFY_INPUT: &str = r#"{"type":"object","required":["id"],"properties":{"id":{"type":"string"},"claim":{"enum":["all","address","broadband","epc","media","description"],"default":"all"},"refresh":{"enum":["none","stale","all"]}}}"#;
const CORRECT_ADDRESS_INPUT: &str = r#"{"type":"object","required":["id"],"properties":{"id":{"type":"string"},"address":{"type":"string"},"postcode":{"type":"string"},"lat":{"type":"number"},"lng":{"type":"number"},"note":{"type":"string"}}}"#;
const CORRECT_EPC_INPUT: &str = r#"{"type":"object","required":["id"],"anyOf":[{"required":["certificateUrl"]},{"required":["lmkKey"]},{"required":["uprn"]}],"properties":{"id":{"type":"string"},"certificateUrl":{"type":"string"},"lmkKey":{"type":"string"},"uprn":{"type":"string"},"rating":{"type":"string"},"floorAreaSqm":{"type":"number"},"note":{"type":"string"}}}"#;
const CORRECT_MEDIA_INPUT: &str = r#"{"type":"object","required":["id","mapLat","mapLng"],"properties":{"id":{"type":"string"},"mapLat":{"type":"number"},"mapLng":{"type":"number"},"note":{"type":"string"}}}"#;
const CORRECT_CLEAR_INPUT: &str = r#"{"type":"object","required":["id","kind","correctionId"],"properties":{"id":{"type":"string"},"kind":{"enum":["address","epc","media"]},"correctionId":{"type":"string"}}}"#;
const ASSESS_SAVE_INPUT: &str = r#"{"type":"object","required":["id","assessment"],"properties":{"id":{"type":"string"},"assessment":{"type":"object"}}}"#;
const ASSESS_GET_INPUT: &str =
    r#"{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}"#;
const SCORE_COMPUTE_INPUT: &str = r#"{"type":"object","required":["id"],"properties":{"id":{"type":"string"},"scorecard":{"type":"string","default":"default"}}}"#;
const SCORE_GET_INPUT: &str = SCORE_COMPUTE_INPUT;
const SCORE_LIST_INPUT: &str = r#"{"type":"object","properties":{"scorecard":{"type":"string"}}}"#;
const AREA_POSTCODE_INPUT: &str = r#"{"type":"object","required":["postcode"],"properties":{"postcode":{"type":"string"},"radiusM":{"type":"number","default":800},"limit":{"type":"integer","default":8}}}"#;
const LIST_FILTERS_INPUT: &str = r#"{"type":"object","properties":{"recommendation":{"type":"string"},"confidence":{"type":"string"},"area":{"type":"string"},"maxPrice":{"type":"integer"},"postcodePrefix":{"type":"string"}}}"#;
const SEARCH_RESOLVE_INPUT: &str =
    r#"{"type":"object","required":["location"],"properties":{"location":{"type":"string"}}}"#;
const SEARCH_DISCOVER_INPUT: &str = r#"{"type":"object","properties":{"region":{"type":"string"},"location":{"type":"string"},"minPrice":{"type":"integer"},"maxPrice":{"type":"integer"},"minBedrooms":{"type":"integer"},"maxBedrooms":{"type":"integer"},"radius":{"type":"number"},"includeLetAgreed":{"type":"boolean"},"propertyTypes":{"type":"string"},"mustHave":{"type":"string"},"dontShow":{"type":"string"},"locationName":{"type":"string"},"limit":{"type":"integer"}}}"#;
const SOURCES_BUILD_INPUT: &str = r#"{"type":"object","required":["target"],"properties":{"target":{"type":"string"},"jobs":{"type":"integer","default":3},"progress":{"enum":["auto","plain","off"]}}}"#;
const START_INPUT: &str = r#"{"type":"object","additionalProperties":false,"properties":{"id":{"type":"string"},"section":{"type":"array","items":{"enum":["rightmove","description","address","facts","claims","broadband","epc","media","verifications","assessment"]}}}}"#;

const TOOLS_OUTPUT: &str = r#"{"type":"object","required":["version","globalFlags","outputFormats","defaultOutputFormat","tools"],"properties":{"globalFlags":{"type":"array"},"outputFormats":{"type":"array","items":{"enum":["json","toon"]}},"defaultOutputFormat":{"enum":["json"]},"tools":{"type":"array"}}}"#;
const HEALTH_OUTPUT: &str = r#"{"type":"object","required":["status","paths","checks","summary"],"properties":{"status":{"enum":["ready","degraded","blocked"]},"checks":{"type":"array"}}}"#;
const CONFIG_OUTPUT: &str = r#"{"type":"object","required":["path","profile","config"],"properties":{"path":{"type":"string"},"profile":{"type":["string","null"]},"config":{"type":"object"}}}"#;
const CONFIG_PROFILES_OUTPUT: &str = r#"{"type":"object","required":["profileDir","profiles"],"properties":{"profileDir":{"type":"string"},"profiles":{"type":"array","items":{"type":"object","required":["name","path"],"properties":{"name":{"type":"string"},"path":{"type":"string"}}}}}}"#;
const EVIDENCE_BUNDLE_OUTPUT: &str = r#"{"type":"object","required":["entityId","rightmoveId","sections","rightmove","address","facts","claims","verifications","media"],"properties":{"sections":{"type":"object"},"facts":{"type":"array"},"claims":{"type":"array"},"verifications":{"type":"array"},"flags":{"type":"array","items":{"type":"object","required":["severity","category","code","summary","sources","recommendedAction"]}},"assessment":{"type":["object","null"],"properties":{"assessment":{"type":"object"},"normalizedAssessment":{"type":"object"}}},"media":{"type":"object","properties":{"contactSheet":{"type":"object","properties":{"status":{"type":"string"},"localPath":{"type":"string"},"photoCount":{"type":"integer"},"generatedAt":{"type":"string"},"width":{"type":"integer"},"height":{"type":"integer"},"contentHash":{"type":"string"}}}}}}}"#;
const EVIDENCE_OUTPUT: &str = r#"{"oneOf":[{"type":"object","required":["bundle","requestedSections"],"properties":{"bundle":{"type":"object"},"requestedSections":{"type":"array"}}},{"type":"object","required":["items","count","okCount","errorCount"],"properties":{"items":{"type":"array","items":{"type":"object","required":["input","id","ok","elapsed","warnings"],"properties":{"bundle":{"type":"object"},"error":{"type":"object"},"warnings":{"type":"array"}}}},"count":{"type":"integer"},"okCount":{"type":"integer"},"errorCount":{"type":"integer"}}}]}"#;
const VERIFY_OUTPUT: &str = r#"{"type":"object","required":["id","claim","verifications","sections"],"properties":{"verifications":{"type":"array"},"sections":{"type":"object"}}}"#;
const CORRECTION_OUTPUT: &str = r#"{"type":"object","required":["correction","affectedSections","warnings","nextCommands"],"properties":{"correction":{"type":"object"},"affectedSections":{"type":"array"},"warnings":{"type":"array"},"nextCommands":{"type":"array"}}}"#;
const ASSESS_RECORD_OUTPUT: &str = r#"{"type":"object","required":["entityId","assessment","normalizedAssessment","savedAt"],"properties":{"assessment":{"type":"object"},"normalizedAssessment":{"type":"object","properties":{"recommendation":{"type":["string","null"],"enum":["view","consider","hold","watch","pass","benchmark",null]},"confidence":{"type":["string","null"]},"summary":{"type":["string","null"]},"scoreAdjustment":{"type":["number","null"]},"judgmentScore":{"type":["number","null"]},"judgmentRationale":{"type":["string","null"]},"positives":{"type":"array"},"risks":{"type":"array"},"nextActions":{"type":"array"},"tradeoffs":{"type":"array"},"areaNotes":{"type":["string","null"]},"commuteNotes":{"type":["string","null"]},"familyFit":{"type":["string","null"]},"evidenceGaps":{"type":"array"},"source":{"type":["string","null"]},"warnings":{"type":"array"}}}}}"#;
const SCORE_RESULT_OUTPUT: &str = r#"{"type":"object","required":["entityId","rightmoveId","scorecard","computedAt","baseOverall","overall","judgment","band","confidence","domains","summary"],"properties":{"scorecard":{"type":"object"},"baseOverall":{"type":"number"},"overall":{"type":"number"},"judgment":{"type":"object","required":["source","appliedAdjustment","warnings"],"properties":{"source":{"enum":["none","scoreAdjustment","judgmentScore"]},"judgmentScore":{"type":["number","null"]},"requestedAdjustment":{"type":["number","null"]},"appliedAdjustment":{"type":"number"},"rationale":{"type":["string","null"]},"warnings":{"type":"array"}}},"band":{"type":"string"},"confidence":{"type":"string"},"domains":{"type":"array"},"caps":{"type":"array"},"blockers":{"type":"array"},"nextActions":{"type":"array"}}}"#;
const SCORE_COMPUTE_OUTPUT: &str =
    r#"{"type":"object","required":["score"],"properties":{"score":{"type":"object"}}}"#;
const SCORE_LIST_OUTPUT: &str = r#"{"type":"object","required":["scores"],"properties":{"scorecardId":{"type":["string","null"]},"scores":{"type":"array","items":{"type":"object","required":["id","entityId","rightmoveId","scorecardId","scorecardVersion","baseOverall","overall","judgmentAdjustment","band","confidence","computedAt"],"properties":{"judgmentScore":{"type":["number","null"]},"judgmentRationale":{"type":["string","null"]}}}}}}"#;
const SCORECARDS_OUTPUT: &str = r#"{"type":"object","required":["scorecards","defaultScorecard"],"properties":{"scorecards":{"type":"array"},"defaultScorecard":{"type":"object"}}}"#;
const SCORECARDS_VALIDATE_OUTPUT: &str = r#"{"type":"object","required":["status","scorecards","defaultScorecard"],"properties":{"status":{"enum":["ok"]},"scorecards":{"type":"array"},"defaultScorecard":{"type":"object"}}}"#;
const AREA_POSTCODE_OUTPUT: &str = r#"{"type":"object","required":["query","joinKeys","sections","facts","nearby","missingSources","nextActions"],"properties":{"query":{"type":"object"},"joinKeys":{"type":"object"},"sections":{"type":"object"},"facts":{"type":"array"},"broadband":{"type":["object","null"]},"nearby":{"type":"object"},"missingSources":{"type":"array"},"nextActions":{"type":"array"}}}"#;
const ASSESS_LIST_OUTPUT: &str = r#"{"type":"object","required":["assessments","filters"],"properties":{"assessments":{"type":"array","items":{"type":"object","required":["id","entityId","recommendation","confidence","assessment","normalizedAssessment"],"properties":{"assessment":{"type":"object"},"normalizedAssessment":{"type":"object"},"summary":{"type":["string","null"]},"positives":{"type":"array"},"risks":{"type":"array"},"nextActions":{"type":"array"},"tradeoffs":{"type":"array"},"areaNotes":{"type":["string","null"]},"commuteNotes":{"type":["string","null"]},"familyFit":{"type":["string","null"]},"evidenceGaps":{"type":"array"},"source":{"type":["string","null"]}}}},"filters":{"type":"object"}}}"#;
const EVIDENCE_LIST_OUTPUT: &str = r#"{"type":"object","required":["listings","filters"],"properties":{"listings":{"type":"array","items":{"type":"object","required":["id","entityId","url","address","postcode","area","price","pricePcm","recommendation","confidence","savedAt","inspectedAt","updatedAt"]}},"filters":{"type":"object"}}}"#;
const SEARCH_RESOLVE_OUTPUT: &str = r#"{"type":"object","required":["location","matches"],"properties":{"matches":{"type":"array"}}}"#;
const SEARCH_DISCOVER_OUTPUT: &str = r#"{"type":"object","required":["ids","listings","total","locations"],"properties":{"ids":{"type":"array","items":{"type":"string"}},"idsByLocation":{"type":"object"},"listings":{"type":"array"},"locationMatchesById":{"type":"object"},"duplicateIds":{"type":"array"},"duplicateLocationMatches":{"type":"object"},"locations":{"type":"array"},"total":{"type":"integer"}}}"#;
const SEARCH_MARKET_OUTPUT: &str = r#"{"type":"object","required":["market","ids","listings","locations","total"],"properties":{"market":{"type":"object","required":["count","pricedCount","byType","duplicateIds","duplicateLocationMatches"]},"ids":{"type":"array","items":{"type":"string"}},"listings":{"type":"array"},"locations":{"type":"array"},"total":{"type":"integer"}}}"#;
const SOURCES_LIST_OUTPUT: &str = r#"{"type":"object","required":["sources","defaultJobs"],"properties":{"sources":{"type":"array"}}}"#;
const SOURCES_STATUS_OUTPUT: &str = r#"{"type":"object","required":["sources","present","missing"],"properties":{"sources":{"type":"array"}}}"#;
const SOURCES_BUILD_OUTPUT: &str = r#"{"type":"object","required":["target","jobs","durationMs","status","sources"],"properties":{"sources":{"type":"array"}}}"#;
const START_OUTPUT: &str = r#"{"type":"object","required":["status","code","binary"],"properties":{"status":{"enum":["exited"]},"code":{"type":["integer","null"]},"binary":{"type":"string"},"id":{"type":["string","null"]},"sections":{"type":"string"}}}"#;
