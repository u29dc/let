#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process;
use std::time::Instant;

use clap::{Args, Parser, Subcommand, ValueEnum};
use let_sdk::paths::PathOverrides;

mod clipboard;
mod commands;
mod env;
mod envelope;
mod registry;

use commands::{CommandError, CommandOutput, SharedArgs};
use envelope::{ErrorEnvelope, ErrorPayload, Meta, SuccessEnvelope};

#[derive(Debug, Parser)]
#[command(name = "let", version, about = "Agent-native rental CLI")]
struct Cli {
    /// Override data directory path.
    #[arg(long, value_name = "DIR", global = true)]
    data_dir: Option<PathBuf>,

    /// Override config directory path.
    #[arg(long, value_name = "DIR", global = true)]
    config_dir: Option<PathBuf>,

    /// Override cache directory path.
    #[arg(long, value_name = "DIR", global = true)]
    cache_dir: Option<PathBuf>,

    /// Override sources directory path.
    #[arg(long, value_name = "DIR", global = true)]
    sources_dir: Option<PathBuf>,

    /// Emit human-readable output instead of the default JSON envelope.
    #[arg(long, global = true, default_value_t = false)]
    text: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// List tools metadata or show one tool by name.
    Tools { name: Option<String> },
    /// Report runtime health checks.
    Health,
    /// Inspect or validate configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Start the terminal UI.
    Start,
    /// Score listing datasets.
    Score {
        #[command(subcommand)]
        command: ScoreCommand,
    },
    /// View listing tables or details.
    View {
        #[command(subcommand)]
        command: ViewCommand,
    },
    /// Assessment workflow commands.
    Assess {
        #[command(subcommand)]
        command: AssessCommand,
    },
    /// Search discovery command group.
    Search {
        #[command(subcommand)]
        command: SearchCommand,
    },
    /// Operational maintenance command group.
    Ops {
        #[command(subcommand)]
        command: OpsCommand,
    },
    /// Export commands.
    Export {
        #[command(subcommand)]
        command: ExportCommand,
    },
    /// Build source databases.
    Build {
        #[command(subcommand)]
        command: BuildCommand,
    },
    /// Fetch listings by portal ids.
    Fetch {
        /// Comma-separated portal ids.
        ids: String,
        /// Region name override for fetched listings.
        #[arg(long)]
        region: Option<String>,
        /// Optional postcode override used instead of scraped postcode.
        #[arg(long = "override-postcode")]
        override_postcode: Option<String>,
        /// Optional full address override used instead of scraped address.
        #[arg(long = "override-address")]
        override_address: Option<String>,
        /// Skip image and map downloads.
        #[arg(long, default_value_t = false)]
        skip_images: bool,
        /// Skip EPC asset download during media stage.
        #[arg(long, default_value_t = false)]
        skip_epc: bool,
        /// Override min-score threshold used before heavy media stage.
        #[arg(long)]
        min_score: Option<f64>,
        /// Keep new below-threshold listings instead of dropping them.
        #[arg(long, default_value_t = false)]
        keep_below_min: bool,
    },
    /// Capture unknown top-level commands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Show parsed config.
    Show,
    /// Validate config and report issues.
    Validate,
}

#[derive(Debug, Subcommand)]
enum BuildCommand {
    /// Build source databases.
    Sources {
        /// Source target to build.
        #[arg(value_enum)]
        target: BuildSourceTarget,
        /// Parallel jobs for `all` target.
        #[arg(long, default_value_t = 3)]
        jobs: usize,
        /// Progress output mode.
        #[arg(long, value_enum, default_value_t = BuildProgressMode::Auto)]
        progress: BuildProgressMode,
    },
}

#[derive(Debug, Clone, Args)]
struct ViewCopyArgs {
    /// Copy rendered output to the clipboard.
    #[arg(long, short = 'c', default_value_t = false)]
    copy: bool,
}

#[derive(Debug, Subcommand)]
enum ViewCommand {
    /// Ranked listing table.
    List {
        #[arg(long, default_value_t = 20)]
        top: usize,
        #[arg(long)]
        min_score: Option<f64>,
        #[arg(long, default_value = "score")]
        sort: String,
        #[arg(long, default_value_t = false)]
        asc: bool,
        #[arg(long)]
        region: Option<String>,
        #[arg(long = "type")]
        property_type: Option<String>,
        #[command(flatten)]
        copy: ViewCopyArgs,
    },
    /// Full listing details by id.
    Detail {
        /// Listing UUID or portal id.
        id: String,
        #[command(flatten)]
        copy: ViewCopyArgs,
    },
}

#[derive(Debug, Subcommand)]
enum AssessCommand {
    /// Unassessed listings ranked by score.
    Candidates {
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        region: Option<String>,
        #[arg(long)]
        min_score: Option<f64>,
    },
    /// Assessment context bundle for listing id.
    Context {
        /// Listing UUID or portal id.
        id: String,
    },
    /// Submit assessment payload for listing id.
    Submit {
        /// Listing UUID or portal id.
        id: String,
        /// Assessment JSON payload string.
        assessment: String,
    },
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export database snapshot to JSON.
    Json {
        /// Output path (defaults to data/let.db.json).
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Export listings to Notion.
    Notion {
        #[arg(long)]
        top: Option<usize>,
        #[arg(long)]
        min_score: Option<f64>,
        #[arg(long)]
        region: Option<String>,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Capture unknown export subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum SearchCommand {
    /// Resolve location names to Rightmove identifiers.
    Resolve {
        /// City or area name.
        location: String,
    },
    /// Discover listing ids from configured or ad-hoc locations.
    Discover {
        #[arg(long)]
        region: Option<String>,
        #[arg(long)]
        location: Option<String>,
        #[arg(long = "property-types")]
        property_types: Option<String>,
        #[arg(long = "must-have")]
        must_have: Option<String>,
        #[arg(long = "dont-show")]
        dont_show: Option<String>,
        #[arg(long = "location-name")]
        location_name: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Compare ids with known listings.
    Diff {
        /// Comma-separated portal ids.
        ids: String,
    },
    /// Capture unknown search subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum OpsCommand {
    /// Patch listing fields and rescore.
    Patch {
        id: String,
        #[arg(long)]
        address: Option<String>,
        #[arg(long)]
        postcode: Option<String>,
        #[arg(long)]
        lat: Option<f64>,
        #[arg(long)]
        lng: Option<f64>,
        #[arg(long)]
        region: Option<String>,
        #[arg(long = "epc-rating")]
        epc_rating: Option<String>,
        #[arg(long = "floor-area")]
        floor_area: Option<f64>,
        #[arg(long = "gigabit-availability")]
        gigabit_availability: Option<f64>,
        #[arg(long = "crime-rate-per-1k")]
        crime_rate_per_1k: Option<f64>,
        #[arg(long = "crime-count-12m")]
        crime_count_12m: Option<i64>,
        #[arg(long = "crime-violent-12m")]
        crime_violent_12m: Option<i64>,
        #[arg(long = "crime-burglary-12m")]
        crime_burglary_12m: Option<i64>,
        #[arg(long = "crime-robbery-12m")]
        crime_robbery_12m: Option<i64>,
        #[arg(long = "imd-decile")]
        imd_decile: Option<i64>,
        #[arg(long = "imd-rank")]
        imd_rank: Option<i64>,
        #[arg(long = "imd-score")]
        imd_score: Option<f64>,
        #[arg(long = "lsoa-code")]
        lsoa_code: Option<String>,
        #[arg(long = "lsoa-name")]
        lsoa_name: Option<String>,
        #[arg(long = "msoa-code")]
        msoa_code: Option<String>,
        #[arg(long = "msoa-name")]
        msoa_name: Option<String>,
        #[arg(long = "income-bhc")]
        income_bhc: Option<f64>,
        #[arg(long = "income-ahc")]
        income_ahc: Option<f64>,
        #[arg(long = "social-housing-pct")]
        social_housing_pct: Option<f64>,
        #[arg(long)]
        population: Option<i64>,
        #[arg(long = "flood-risk-level")]
        flood_risk_level: Option<String>,
        #[arg(long = "flood-risk-source")]
        flood_risk_source: Option<String>,
        #[arg(long = "crime-band")]
        crime_band: Option<String>,
        #[arg(long = "crime-trend")]
        crime_trend: Option<String>,
        #[arg(long = "crime-updated-at")]
        crime_updated_at: Option<String>,
        #[arg(long = "patch-json")]
        patch_json: Option<String>,
        #[arg(long, default_value_t = false)]
        skip_re_enrich: bool,
        #[arg(long, default_value_t = false)]
        skip_images: bool,
    },
    /// Prune listings by score, region, or inactive status.
    #[command(long_about = "Prune selector rules:\n\
                      - No selector defaults to score < 50.\n\
                      - --region alone prunes all listings matching the region filter.\n\
                      - --region can be combined with --min-score or --bottom.\n\
                      - --inactive can be combined only with optional --region.\n\
                      - --bottom and --min-score are mutually exclusive.")]
    Prune {
        /// Prune listings with score lower than this threshold.
        /// When no selector flags are set, default behavior is `score < 50`.
        #[arg(long)]
        min_score: Option<f64>,
        /// Prune bottom N percent by score (1-100). Cannot be combined with `--min-score`.
        #[arg(long)]
        bottom: Option<u8>,
        /// Limit selection to region patterns (comma-separated).
        /// With no other selectors, all matched regions are pruned.
        #[arg(long)]
        region: Option<String>,
        /// Prune inactive listings only.
        /// Can be combined with `--region` but not with score selectors.
        #[arg(long, default_value_t = false)]
        inactive: bool,
        /// Preview selected rows without deleting.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        /// Skip confirmation prompt in text mode.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Verify listing activity status against portal pages.
    Verify {
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(long)]
        region: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = 3000)]
        delay: u64,
    },
    /// Capture unknown ops subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum ScoreCommand {
    /// Recompute scores for all listings.
    Compute,
    /// Explain score breakdown for a listing id.
    Explain {
        /// Listing UUID or portal id.
        id: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BuildSourceTarget {
    List,
    All,
    Broadband,
    Postcodes,
    Deprivation,
    Census,
    Population,
    Income,
    Flood,
    Naptan,
    Uprn,
    Crime,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BuildProgressMode {
    Auto,
    Plain,
    Off,
}

impl From<BuildSourceTarget> for commands::build::SourceTarget {
    fn from(value: BuildSourceTarget) -> Self {
        use commands::build::SourceTarget;
        match value {
            BuildSourceTarget::List => SourceTarget::List,
            BuildSourceTarget::All => SourceTarget::All,
            BuildSourceTarget::Broadband => SourceTarget::Broadband,
            BuildSourceTarget::Postcodes => SourceTarget::Postcodes,
            BuildSourceTarget::Deprivation => SourceTarget::Deprivation,
            BuildSourceTarget::Census => SourceTarget::Census,
            BuildSourceTarget::Population => SourceTarget::Population,
            BuildSourceTarget::Income => SourceTarget::Income,
            BuildSourceTarget::Flood => SourceTarget::Flood,
            BuildSourceTarget::Naptan => SourceTarget::Naptan,
            BuildSourceTarget::Uprn => SourceTarget::Uprn,
            BuildSourceTarget::Crime => SourceTarget::Crime,
        }
    }
}

impl From<BuildProgressMode> for commands::build::ProgressMode {
    fn from(value: BuildProgressMode) -> Self {
        use commands::build::ProgressMode;
        match value {
            BuildProgressMode::Auto => ProgressMode::Auto,
            BuildProgressMode::Plain => ProgressMode::Plain,
            BuildProgressMode::Off => ProgressMode::Off,
        }
    }
}

enum DispatchOutcome {
    Local {
        tool: &'static str,
        copy_requested: bool,
        result: Result<CommandOutput, CommandError>,
    },
}

impl DispatchOutcome {
    fn local(tool: &'static str, result: Result<CommandOutput, CommandError>) -> Self {
        Self::Local {
            tool,
            copy_requested: false,
            result,
        }
    }

    fn local_with_copy(
        tool: &'static str,
        copy_requested: bool,
        result: Result<CommandOutput, CommandError>,
    ) -> Self {
        Self::Local {
            tool,
            copy_requested,
            result,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputMode {
    EnvelopeJson,
    Text,
}

fn main() {
    let cli = Cli::parse();

    let shared = SharedArgs {
        overrides: PathOverrides {
            data_dir: cli.data_dir,
            config_dir: cli.config_dir,
            cache_dir: cli.cache_dir,
            sources_dir: cli.sources_dir,
        },
    };

    let started = Instant::now();
    let outcome = dispatch(&cli.command, &shared);

    match outcome {
        DispatchOutcome::Local {
            tool,
            copy_requested,
            result,
        } => {
            let elapsed = started.elapsed().as_millis() as u64;
            let mode = output_mode(cli.text);
            let result = apply_copy_request(result, copy_requested, mode);
            let exit_code = emit(&result, tool, elapsed, mode);
            process::exit(exit_code);
        }
    }
}

fn dispatch(command: &Command, shared: &SharedArgs) -> DispatchOutcome {
    match command {
        Command::Tools { name } => {
            DispatchOutcome::local("tools", commands::tools::run(name.as_deref()))
        }
        Command::Health => DispatchOutcome::local("health", commands::health::run(shared)),
        Command::Config {
            command: ConfigCommand::Show,
        } => DispatchOutcome::local("config.show", commands::config::show(shared)),
        Command::Config {
            command: ConfigCommand::Validate,
        } => DispatchOutcome::local("config.validate", commands::config::validate(shared)),
        Command::Start => DispatchOutcome::local("start", commands::start::run(shared)),
        Command::Score {
            command: ScoreCommand::Compute,
        } => DispatchOutcome::local("score.compute", commands::score::compute(shared)),
        Command::Score {
            command: ScoreCommand::Explain { id },
        } => DispatchOutcome::local("score.explain", commands::score::explain(shared, id)),
        Command::View {
            command:
                ViewCommand::List {
                    top,
                    min_score,
                    sort,
                    asc,
                    region,
                    property_type,
                    copy,
                },
        } => DispatchOutcome::local_with_copy(
            "view.list",
            copy.copy,
            commands::view::list(
                shared,
                &commands::view::ViewListParams {
                    top: if *top == 0 { 20 } else { *top },
                    min_score: *min_score,
                    sort: commands::view::SortField::parse(sort.as_str()),
                    asc: *asc,
                    region: region.clone(),
                    property_type: property_type.clone(),
                },
            ),
        ),
        Command::View {
            command: ViewCommand::Detail { id, copy },
        } => DispatchOutcome::local_with_copy(
            "view.detail",
            copy.copy,
            commands::view::detail(shared, id),
        ),
        Command::Assess {
            command:
                AssessCommand::Candidates {
                    top,
                    region,
                    min_score,
                },
        } => DispatchOutcome::local(
            "assess.candidates",
            commands::assess::candidates(
                shared,
                &commands::assess::CandidatesParams {
                    top: if *top == 0 { 10 } else { *top },
                    region: region.clone(),
                    min_score: *min_score,
                },
            ),
        ),
        Command::Assess {
            command: AssessCommand::Context { id },
        } => DispatchOutcome::local("assess.context", commands::assess::context(shared, id)),
        Command::Assess {
            command: AssessCommand::Submit { id, assessment },
        } => DispatchOutcome::local(
            "assess.submit",
            commands::assess::submit(shared, id, assessment),
        ),
        Command::Search {
            command: SearchCommand::Resolve { location },
        } => DispatchOutcome::local("search.resolve", commands::search::resolve(location)),
        Command::Search {
            command:
                SearchCommand::Discover {
                    region,
                    location,
                    property_types,
                    must_have,
                    dont_show,
                    location_name,
                    limit,
                },
        } => DispatchOutcome::local(
            "search.discover",
            commands::search::discover(
                shared,
                &commands::search::DiscoverParams {
                    region: region.clone(),
                    location: location.clone(),
                    property_types: property_types.clone(),
                    must_have: must_have.clone(),
                    dont_show: dont_show.clone(),
                    location_name: location_name.clone(),
                    limit: *limit,
                },
            ),
        ),
        Command::Search {
            command: SearchCommand::Diff { ids },
        } => DispatchOutcome::local("search.diff", commands::search::diff(shared, ids)),
        Command::Search {
            command: SearchCommand::External(args),
        } => unsupported_external("search", args),
        Command::Ops {
            command:
                OpsCommand::Patch {
                    id,
                    address,
                    postcode,
                    lat,
                    lng,
                    region,
                    epc_rating,
                    floor_area,
                    gigabit_availability,
                    crime_rate_per_1k,
                    crime_count_12m,
                    crime_violent_12m,
                    crime_burglary_12m,
                    crime_robbery_12m,
                    imd_decile,
                    imd_rank,
                    imd_score,
                    lsoa_code,
                    lsoa_name,
                    msoa_code,
                    msoa_name,
                    income_bhc,
                    income_ahc,
                    social_housing_pct,
                    population,
                    flood_risk_level,
                    flood_risk_source,
                    crime_band,
                    crime_trend,
                    crime_updated_at,
                    patch_json,
                    skip_re_enrich,
                    skip_images,
                },
        } => DispatchOutcome::local(
            "ops.patch",
            commands::ops::patch(
                shared,
                &commands::ops::PatchParams {
                    id: id.clone(),
                    address: address.clone(),
                    postcode: postcode.clone(),
                    lat: *lat,
                    lng: *lng,
                    region: region.clone(),
                    epc_rating: epc_rating.clone(),
                    floor_area: *floor_area,
                    gigabit_availability: *gigabit_availability,
                    crime_rate_per_1k: *crime_rate_per_1k,
                    crime_count_12m: *crime_count_12m,
                    crime_violent_12m: *crime_violent_12m,
                    crime_burglary_12m: *crime_burglary_12m,
                    crime_robbery_12m: *crime_robbery_12m,
                    imd_decile: *imd_decile,
                    imd_rank: *imd_rank,
                    imd_score: *imd_score,
                    lsoa_code: lsoa_code.clone(),
                    lsoa_name: lsoa_name.clone(),
                    msoa_code: msoa_code.clone(),
                    msoa_name: msoa_name.clone(),
                    income_bhc: *income_bhc,
                    income_ahc: *income_ahc,
                    social_housing_pct: *social_housing_pct,
                    population: *population,
                    flood_risk_level: flood_risk_level.clone(),
                    flood_risk_source: flood_risk_source.clone(),
                    crime_band: crime_band.clone(),
                    crime_trend: crime_trend.clone(),
                    crime_updated_at: crime_updated_at.clone(),
                    patch_json: patch_json.clone(),
                    skip_re_enrich: *skip_re_enrich,
                    skip_images: *skip_images,
                },
            ),
        ),
        Command::Ops {
            command:
                OpsCommand::Prune {
                    min_score,
                    bottom,
                    region,
                    inactive,
                    dry_run,
                    force,
                },
        } => DispatchOutcome::local(
            "ops.prune",
            commands::ops::prune(
                shared,
                &commands::ops::PruneParams {
                    min_score: *min_score,
                    bottom_percent: *bottom,
                    region: region.clone(),
                    inactive_only: *inactive,
                    dry_run: *dry_run,
                    force: *force,
                },
            ),
        ),
        Command::Ops {
            command:
                OpsCommand::Verify {
                    dry_run,
                    region,
                    limit,
                    delay,
                },
        } => DispatchOutcome::local(
            "ops.verify",
            commands::ops::verify(
                shared,
                &commands::ops::VerifyParams {
                    dry_run: *dry_run,
                    region: region.clone(),
                    limit: *limit,
                    delay_ms: *delay,
                },
            ),
        ),
        Command::Ops {
            command: OpsCommand::External(args),
        } => unsupported_external("ops", args),
        Command::Export {
            command: ExportCommand::Json { output },
        } => DispatchOutcome::local(
            "export.json",
            commands::export::export_json(shared, output.clone()),
        ),
        Command::Export {
            command:
                ExportCommand::Notion {
                    top,
                    min_score,
                    region,
                    dry_run,
                    force,
                },
        } => DispatchOutcome::local(
            "export.notion",
            commands::export::export_notion(
                shared,
                &commands::export::NotionParams {
                    top: *top,
                    min_score: *min_score,
                    region: region.clone(),
                    dry_run: *dry_run,
                    force: *force,
                },
            ),
        ),
        Command::Export {
            command: ExportCommand::External(args),
        } => unsupported_external("export", args),
        Command::Build {
            command:
                BuildCommand::Sources {
                    target,
                    jobs,
                    progress,
                },
        } => DispatchOutcome::local(
            "build.sources",
            commands::build::run_sources((*target).into(), *jobs, shared, (*progress).into()),
        ),
        Command::Fetch {
            ids,
            region,
            override_postcode,
            override_address,
            skip_images,
            skip_epc,
            min_score,
            keep_below_min,
        } => DispatchOutcome::local(
            "fetch",
            commands::fetch::run(
                shared,
                &commands::fetch::FetchParams {
                    ids: ids.clone(),
                    region: region.clone(),
                    override_postcode: override_postcode.clone(),
                    override_address: override_address.clone(),
                    skip_images: *skip_images,
                    skip_epc: *skip_epc,
                    min_score: *min_score,
                    keep_below_min: *keep_below_min,
                },
            ),
        ),
        Command::External(args) => {
            if args.is_empty() {
                DispatchOutcome::local(
                    "external",
                    Err(CommandError::runtime(
                        "VALIDATION_ERROR",
                        "missing command",
                        "run `let tools` to list available commands",
                    )),
                )
            } else {
                DispatchOutcome::local(
                    "external",
                    Err(CommandError::runtime(
                        "UNSUPPORTED_COMMAND",
                        format!("unsupported command: {}", args.join(" ")),
                        "run `let tools` to list available commands",
                    )),
                )
            }
        }
    }
}

fn unsupported_external(group: &str, args: &[String]) -> DispatchOutcome {
    let detail = if args.is_empty() {
        format!("{group} command is not supported")
    } else {
        format!("{group} command is not supported: {}", args.join(" "))
    };

    DispatchOutcome::local(
        "external",
        Err(CommandError::runtime(
            "UNSUPPORTED_COMMAND",
            detail,
            "run `let tools` to list available commands",
        )),
    )
}

fn output_mode(text_requested: bool) -> OutputMode {
    if text_requested {
        OutputMode::Text
    } else {
        OutputMode::EnvelopeJson
    }
}

fn apply_copy_request(
    result: Result<CommandOutput, CommandError>,
    copy_requested: bool,
    mode: OutputMode,
) -> Result<CommandOutput, CommandError> {
    if !copy_requested {
        return result;
    }

    let payload = match &result {
        Ok(output) => render_copy_payload(output, mode)?,
        Err(err) => return Err(err.clone()),
    };

    clipboard::copy_text(&payload)?;
    result
}

fn render_copy_payload(output: &CommandOutput, mode: OutputMode) -> Result<String, CommandError> {
    match mode {
        OutputMode::EnvelopeJson => render_json_copy_payload(output),
        OutputMode::Text => render_text_copy_payload(output),
    }
}

fn render_json_copy_payload(output: &CommandOutput) -> Result<String, CommandError> {
    let payload = output.clipboard.json.as_ref().unwrap_or(&output.data);
    serde_json::to_string_pretty(payload).map_err(|error| {
        CommandError::runtime(
            "INTERNAL_ERROR",
            format!("failed to serialize command payload for clipboard copy: {error}"),
            "report this bug",
        )
    })
}

fn render_text_copy_payload(output: &CommandOutput) -> Result<String, CommandError> {
    if let Some(text) = &output.clipboard.text {
        return Ok(text.clone());
    }

    render_text_payload(output)
}

fn render_text_payload(output: &CommandOutput) -> Result<String, CommandError> {
    if let Some(text) = &output.text {
        return Ok(text.clone());
    }

    serde_json::to_string_pretty(&output.data).map_err(|error| {
        CommandError::runtime(
            "INTERNAL_ERROR",
            format!("failed to render text output: {error}"),
            "report this bug",
        )
    })
}

fn emit(
    result: &Result<CommandOutput, CommandError>,
    tool: &str,
    elapsed: u64,
    mode: OutputMode,
) -> i32 {
    match mode {
        OutputMode::EnvelopeJson => emit_json(result, tool, elapsed),
        OutputMode::Text => emit_text(result),
    }
}

fn emit_json(result: &Result<CommandOutput, CommandError>, tool: &str, elapsed: u64) -> i32 {
    match result {
        Ok(output) => {
            let meta = Meta::new(tool, elapsed)
                .with_count(output.meta.count)
                .with_total(output.meta.total)
                .with_has_more(output.meta.has_more);
            let envelope = SuccessEnvelope::new(output.data.clone(), meta);
            println!(
                "{}",
                serde_json::to_string(&envelope).expect("success envelope serialization failed")
            );
            0
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
            println!(
                "{}",
                serde_json::to_string(&envelope).expect("error envelope serialization failed")
            );
            err.exit_code
        }
    }
}

fn emit_text(result: &Result<CommandOutput, CommandError>) -> i32 {
    match result {
        Ok(output) => match render_text_payload(output) {
            Ok(text) => {
                println!("{text}");
                0
            }
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                eprintln!("hint: {}", err.hint);
                err.exit_code
            }
        },
        Err(err) => {
            eprintln!("{}: {}", err.code, err.message);
            eprintln!("hint: {}", err.hint);
            err.exit_code
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unsupported_external;

    #[test]
    fn unsupported_external_returns_error() {
        let result = unsupported_external("search", &[String::from("legacy-subcommand")]);
        match result {
            super::DispatchOutcome::Local { result, .. } => {
                let error = result.expect_err("expected unsupported command error");
                assert_eq!(error.code, "UNSUPPORTED_COMMAND");
            }
        }
    }
}
