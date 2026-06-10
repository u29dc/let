#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process;
use std::time::Instant;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use let_sdk::paths::PathOverrides;

mod clipboard;
mod commands;
mod env;
mod envelope;
mod registry;

use commands::{CommandError, CommandOutput, SharedArgs};
use envelope::{OutputFormat, emit_result};

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

    /// Emit Toon instead of the default JSON envelope.
    #[arg(long, global = true, default_value_t = false)]
    toon: bool,

    #[command(subcommand)]
    command: Option<Command>,
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
        command: Option<ConfigCommand>,
    },
    /// Start the terminal UI.
    Start,
    /// Score listing datasets.
    Score {
        #[command(subcommand)]
        command: Option<ScoreCommand>,
    },
    /// View listing lists or details.
    View {
        #[command(subcommand)]
        command: Option<ViewCommand>,
    },
    /// Assessment workflow commands.
    Assess {
        #[command(subcommand)]
        command: Option<AssessCommand>,
    },
    /// Search discovery command group.
    Search {
        #[command(subcommand)]
        command: Option<SearchCommand>,
    },
    /// Operational maintenance command group.
    Ops {
        #[command(subcommand)]
        command: Option<OpsCommand>,
    },
    /// Export commands.
    Export {
        #[command(subcommand)]
        command: Option<ExportCommand>,
    },
    /// Build source databases.
    Build {
        #[command(subcommand)]
        command: Option<BuildCommand>,
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
    /// Capture unknown config subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
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
    /// Capture unknown build subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Args)]
struct ViewCopyArgs {
    /// Copy rendered output to the clipboard.
    #[arg(long, short = 'c', default_value_t = false)]
    copy: bool,
}

#[derive(Debug, Subcommand)]
enum ViewCommand {
    /// Ranked listing list.
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
    /// Capture unknown view subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
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
    /// Capture unknown assess subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
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
        /// Skip confirmation prompt.
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
    /// Capture unknown score subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
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
    Help {
        path: &'static [&'static str],
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

    fn help(path: &'static [&'static str]) -> Self {
        Self::Help { path }
    }
}

fn main() {
    let cli = Cli::parse();

    let Some(command) = &cli.command else {
        process::exit(print_help(&[]));
    };

    let shared = SharedArgs {
        overrides: PathOverrides {
            data_dir: cli.data_dir,
            config_dir: cli.config_dir,
            cache_dir: cli.cache_dir,
            sources_dir: cli.sources_dir,
        },
    };

    let started = Instant::now();
    let outcome = dispatch(command, &shared);

    match outcome {
        DispatchOutcome::Local {
            tool,
            copy_requested,
            result,
        } => {
            let elapsed = started.elapsed().as_millis() as u64;
            let format = output_format(cli.toon);
            let result = apply_copy_request(result, copy_requested, format);
            let exit_code = emit_result(&result, tool, elapsed, format);
            process::exit(exit_code);
        }
        DispatchOutcome::Help { path } => {
            process::exit(print_help(path));
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
            command: Some(ConfigCommand::Show),
        } => DispatchOutcome::local("config.show", commands::config::show(shared)),
        Command::Config {
            command: Some(ConfigCommand::Validate),
        } => DispatchOutcome::local("config.validate", commands::config::validate(shared)),
        Command::Config {
            command: Some(ConfigCommand::External(args)),
        } => unsupported_external("config", args),
        Command::Config { command: None } => DispatchOutcome::help(&["config"]),
        Command::Start => DispatchOutcome::local("start", commands::start::run(shared)),
        Command::Score {
            command: Some(ScoreCommand::Compute),
        } => DispatchOutcome::local("score.compute", commands::score::compute(shared)),
        Command::Score {
            command: Some(ScoreCommand::Explain { id }),
        } => DispatchOutcome::local("score.explain", commands::score::explain(shared, id)),
        Command::Score {
            command: Some(ScoreCommand::External(args)),
        } => unsupported_external("score", args),
        Command::Score { command: None } => DispatchOutcome::help(&["score"]),
        Command::View {
            command:
                Some(ViewCommand::List {
                    top,
                    min_score,
                    sort,
                    asc,
                    region,
                    property_type,
                    copy,
                }),
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
            command: Some(ViewCommand::Detail { id, copy }),
        } => DispatchOutcome::local_with_copy(
            "view.detail",
            copy.copy,
            commands::view::detail(shared, id),
        ),
        Command::View {
            command: Some(ViewCommand::External(args)),
        } => unsupported_external("view", args),
        Command::View { command: None } => DispatchOutcome::help(&["view"]),
        Command::Assess {
            command:
                Some(AssessCommand::Candidates {
                    top,
                    region,
                    min_score,
                }),
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
            command: Some(AssessCommand::Context { id }),
        } => DispatchOutcome::local("assess.context", commands::assess::context(shared, id)),
        Command::Assess {
            command: Some(AssessCommand::Submit { id, assessment }),
        } => DispatchOutcome::local(
            "assess.submit",
            commands::assess::submit(shared, id, assessment),
        ),
        Command::Assess {
            command: Some(AssessCommand::External(args)),
        } => unsupported_external("assess", args),
        Command::Assess { command: None } => DispatchOutcome::help(&["assess"]),
        Command::Search {
            command: Some(SearchCommand::Resolve { location }),
        } => DispatchOutcome::local("search.resolve", commands::search::resolve(location)),
        Command::Search {
            command:
                Some(SearchCommand::Discover {
                    region,
                    location,
                    property_types,
                    must_have,
                    dont_show,
                    location_name,
                    limit,
                }),
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
            command: Some(SearchCommand::Diff { ids }),
        } => DispatchOutcome::local("search.diff", commands::search::diff(shared, ids)),
        Command::Search {
            command: Some(SearchCommand::External(args)),
        } => unsupported_external("search", args),
        Command::Search { command: None } => DispatchOutcome::help(&["search"]),
        Command::Ops {
            command:
                Some(OpsCommand::Patch {
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
                }),
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
                Some(OpsCommand::Prune {
                    min_score,
                    bottom,
                    region,
                    inactive,
                    dry_run,
                    force,
                }),
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
                Some(OpsCommand::Verify {
                    dry_run,
                    region,
                    limit,
                    delay,
                }),
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
            command: Some(OpsCommand::External(args)),
        } => unsupported_external("ops", args),
        Command::Ops { command: None } => DispatchOutcome::help(&["ops"]),
        Command::Export {
            command: Some(ExportCommand::Json { output }),
        } => DispatchOutcome::local(
            "export.json",
            commands::export::export_json(shared, output.clone()),
        ),
        Command::Export {
            command:
                Some(ExportCommand::Notion {
                    top,
                    min_score,
                    region,
                    dry_run,
                    force,
                }),
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
            command: Some(ExportCommand::External(args)),
        } => unsupported_external("export", args),
        Command::Export { command: None } => DispatchOutcome::help(&["export"]),
        Command::Build {
            command:
                Some(BuildCommand::Sources {
                    target,
                    jobs,
                    progress,
                }),
        } => DispatchOutcome::local(
            "build.sources",
            commands::build::run_sources((*target).into(), *jobs, shared, (*progress).into()),
        ),
        Command::Build {
            command: Some(BuildCommand::External(args)),
        } => unsupported_external("build", args),
        Command::Build { command: None } => DispatchOutcome::help(&["build"]),
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

fn output_format(toon_requested: bool) -> OutputFormat {
    if toon_requested {
        OutputFormat::Toon
    } else {
        OutputFormat::Json
    }
}

fn apply_copy_request(
    result: Result<CommandOutput, CommandError>,
    copy_requested: bool,
    format: OutputFormat,
) -> Result<CommandOutput, CommandError> {
    if !copy_requested {
        return result;
    }

    let payload = match &result {
        Ok(output) => render_copy_payload(output, format)?,
        Err(err) => return Err(err.clone()),
    };

    clipboard::copy_text(&payload)?;
    result
}

fn render_copy_payload(
    output: &CommandOutput,
    format: OutputFormat,
) -> Result<String, CommandError> {
    let payload = output.clipboard.json.as_ref().unwrap_or(&output.data);
    match format {
        OutputFormat::Json => {
            serde_json::to_string_pretty(payload).map_err(|error| error.to_string())
        }
        OutputFormat::Toon => {
            toon_format::encode_default(payload).map_err(|error| error.to_string())
        }
    }
    .map_err(|error| {
        CommandError::runtime(
            "INTERNAL_ERROR",
            format!("failed to serialize command payload for clipboard copy: {error}"),
            "report this bug",
        )
    })
}

fn print_help(path: &[&str]) -> i32 {
    let mut command = Cli::command();
    for name in path {
        let Some(subcommand) = command.find_subcommand_mut(name) else {
            eprintln!("failed to render help for `{}`", path.join(" "));
            return 1;
        };
        command = subcommand.clone();
    }
    if let Err(error) = command.print_help() {
        eprintln!("failed to render help: {error}");
        return 1;
    }
    println!();
    0
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
            super::DispatchOutcome::Help { .. } => panic!("expected local error outcome"),
        }
    }
}
