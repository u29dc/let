#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process;
use std::time::Instant;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use let_sdk::intelligence::{
    CorrectionKind, EvidenceSection, InspectDepth, ListingListFilters, RefreshPolicy,
};
use let_sdk::paths::PathOverrides;

mod commands;
mod env;
mod envelope;
mod registry;

use commands::{CommandError, CommandOutput, SharedArgs};
use envelope::{OutputFormat, emit_result};

#[derive(Debug, Parser)]
#[command(
    name = "let",
    version,
    about = "Agent-native UK rental intelligence CLI"
)]
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

    /// Select config profile from profiles/<name>.toml.
    #[arg(long, value_name = "NAME", global = true)]
    profile: Option<String>,

    /// Emit Toon instead of the default JSON envelope.
    #[arg(long, global = true, default_value_t = false)]
    toon: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List tools metadata or show one tool by name.
    Tools { name: Option<String> },
    /// Report runtime health checks.
    Health,
    /// Inspect configuration.
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
    /// Inspect local area facts by postcode.
    Area {
        #[command(subcommand)]
        command: Option<AreaCommand>,
    },
    /// Resolve locations or discover Rightmove listing ids.
    Search {
        #[command(subcommand)]
        command: Option<SearchCommand>,
    },
    /// Gather and persist property evidence for one Rightmove listing.
    Inspect {
        /// Rightmove portal id or listing URL.
        id_or_url: String,
        /// Evidence depth.
        #[arg(long, value_enum, default_value_t = CliInspectDepth::Standard)]
        depth: CliInspectDepth,
        /// Refresh policy for network/source reads.
        #[arg(long, value_enum, default_value_t = CliRefreshPolicy::Stale)]
        refresh: CliRefreshPolicy,
        /// Restrict sections; repeat or comma-separate values.
        #[arg(long, value_enum, value_delimiter = ',')]
        section: Vec<CliEvidenceSection>,
    },
    /// Read a stored evidence bundle.
    Evidence {
        /// Rightmove id or entity id.
        id: String,
        /// Restrict sections; repeat or comma-separate values.
        #[arg(long, value_enum, value_delimiter = ',')]
        section: Vec<CliEvidenceSection>,
        #[command(flatten)]
        filters: CliListFilters,
    },
    /// Verify extracted claims against available sources.
    Verify {
        /// Rightmove id or entity id.
        id: String,
        /// Claim type to verify: all, address, broadband, epc, media, or description.
        #[arg(long, default_value = "all")]
        claim: String,
        /// Refresh policy before verification.
        #[arg(long, value_enum, default_value_t = CliRefreshPolicy::None)]
        refresh: CliRefreshPolicy,
    },
    /// Save or read AI-authored assessments.
    Assess {
        #[command(subcommand)]
        command: Option<AssessCommand>,
    },
    /// Record manual evidence corrections without mutating source snapshots.
    Correct {
        #[command(subcommand)]
        command: Option<CorrectCommand>,
    },
    /// Manage local enrichment source databases.
    Sources {
        #[command(subcommand)]
        command: Option<SourcesCommand>,
    },
    /// Launch the local TUI browser.
    Start {
        /// Optional Rightmove id or entity id to focus when supported by the TUI.
        #[arg(long)]
        id: Option<String>,
        /// Restrict starting sections; repeat or comma-separate values.
        #[arg(long, value_enum, value_delimiter = ',')]
        section: Vec<CliEvidenceSection>,
    },
    /// Capture unknown top-level commands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Show parsed config.
    Show,
    /// List available config profiles.
    Profiles,
    /// Capture unknown config subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum AreaCommand {
    /// Read local-source area facts for a postcode.
    Postcode {
        /// UK postcode to inspect.
        postcode: String,
        /// Nearby lookup radius in metres for local stop/UPRN sources.
        #[arg(long = "radius-m", default_value_t = 800.0)]
        radius_m: f64,
        /// Maximum nearby stop/UPRN candidates to return.
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Capture unknown area subcommands.
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
    Discover(SearchDiscoveryArgs),
    /// Summarize discovered listing card rents and duplicate location matches.
    Market(SearchDiscoveryArgs),
    /// Capture unknown search subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Args)]
struct SearchDiscoveryArgs {
    #[arg(long)]
    region: Option<String>,
    #[arg(long)]
    location: Option<String>,
    #[arg(long = "min-price")]
    min_price: Option<i64>,
    #[arg(long = "max-price")]
    max_price: Option<i64>,
    #[arg(long = "min-bedrooms")]
    min_bedrooms: Option<i64>,
    #[arg(long = "max-bedrooms")]
    max_bedrooms: Option<i64>,
    #[arg(long)]
    radius: Option<f64>,
    #[arg(long = "include-let-agreed", num_args = 0..=1, default_missing_value = "true")]
    include_let_agreed: Option<bool>,
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
}

#[derive(Debug, Subcommand)]
enum AssessCommand {
    /// Save an AI-authored assessment JSON object.
    Save {
        /// Rightmove id or entity id.
        id: String,
        /// Assessment JSON object.
        assessment: String,
    },
    /// Read a saved assessment.
    Get {
        /// Rightmove id or entity id.
        id: String,
    },
    /// List saved assessments with optional summary filters.
    List {
        #[command(flatten)]
        filters: CliListFilters,
    },
    /// Capture unknown assess subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Default, Args)]
struct CliListFilters {
    /// Match a saved assessment recommendation.
    #[arg(long)]
    recommendation: Option<String>,
    /// Match saved area, address, postcode, or assessment area text.
    #[arg(long)]
    area: Option<String>,
    /// Maximum monthly rent in pounds.
    #[arg(long = "max-price")]
    max_price: Option<i64>,
    /// Match a postcode prefix such as M1 or YO1.
    #[arg(long = "postcode-prefix")]
    postcode_prefix: Option<String>,
}

impl CliListFilters {
    fn has_values(&self) -> bool {
        self.recommendation.is_some()
            || self.area.is_some()
            || self.max_price.is_some()
            || self.postcode_prefix.is_some()
    }
}

impl From<&CliListFilters> for ListingListFilters {
    fn from(value: &CliListFilters) -> Self {
        Self {
            recommendation: value.recommendation.clone(),
            area: value.area.clone(),
            max_price: value.max_price,
            postcode_prefix: value.postcode_prefix.clone(),
        }
    }
}

#[derive(Debug, Subcommand)]
enum CorrectCommand {
    /// Record a manual address, postcode, or coordinate correction.
    Address {
        /// Rightmove id or entity id.
        id: String,
        /// Corrected address text.
        #[arg(long)]
        address: Option<String>,
        /// Corrected postcode.
        #[arg(long)]
        postcode: Option<String>,
        /// Corrected latitude.
        #[arg(long, allow_hyphen_values = true)]
        lat: Option<f64>,
        /// Corrected longitude.
        #[arg(long, allow_hyphen_values = true)]
        lng: Option<f64>,
        /// Correction provenance note.
        #[arg(long)]
        note: Option<String>,
    },
    /// Record a manual EPC certificate correction.
    Epc {
        /// Rightmove id or entity id.
        id: String,
        /// Exact EPC certificate URL.
        #[arg(long = "certificate-url")]
        certificate_url: Option<String>,
        /// EPC LMK key.
        #[arg(long = "lmk-key")]
        lmk_key: Option<String>,
        /// EPC UPRN.
        #[arg(long)]
        uprn: Option<String>,
        /// Corrected EPC rating; requires --certificate-url, --lmk-key, or --uprn.
        #[arg(long)]
        rating: Option<String>,
        /// Corrected floor area in square metres; requires --certificate-url, --lmk-key, or --uprn.
        #[arg(long = "floor-area-sqm")]
        floor_area_sqm: Option<f64>,
        /// Correction provenance note.
        #[arg(long)]
        note: Option<String>,
    },
    /// Record manual media correction inputs, currently map coordinates.
    Media {
        /// Rightmove id or entity id.
        id: String,
        /// Corrected map latitude.
        #[arg(long = "map-lat", allow_hyphen_values = true)]
        map_lat: Option<f64>,
        /// Corrected map longitude.
        #[arg(long = "map-lng", allow_hyphen_values = true)]
        map_lng: Option<f64>,
        /// Correction provenance note.
        #[arg(long)]
        note: Option<String>,
    },
    /// Disable an active correction without deleting its audit record.
    Clear {
        /// Rightmove id or entity id.
        id: String,
        /// Correction kind to clear.
        #[arg(long, value_enum)]
        kind: CliCorrectionKind,
        /// Exact correction id to clear.
        #[arg(long = "correction-id")]
        correction_id: String,
    },
    /// Capture unknown correct subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum SourcesCommand {
    /// List supported source databases.
    List,
    /// Report which source databases are present locally.
    Status,
    /// Build one or all source databases.
    Build {
        /// Source target to build.
        #[arg(value_enum)]
        target: CliSourceTarget,
        /// Parallel jobs for `all`.
        #[arg(long, default_value_t = 3)]
        jobs: usize,
        /// Progress output mode.
        #[arg(long, value_enum, default_value_t = CliProgressMode::Auto)]
        progress: CliProgressMode,
    },
    /// Capture unknown source subcommands.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliInspectDepth {
    Quick,
    Standard,
    Deep,
}

impl From<CliInspectDepth> for InspectDepth {
    fn from(value: CliInspectDepth) -> Self {
        match value {
            CliInspectDepth::Quick => Self::Quick,
            CliInspectDepth::Standard => Self::Standard,
            CliInspectDepth::Deep => Self::Deep,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliRefreshPolicy {
    None,
    Stale,
    All,
}

impl From<CliRefreshPolicy> for RefreshPolicy {
    fn from(value: CliRefreshPolicy) -> Self {
        match value {
            CliRefreshPolicy::None => Self::None,
            CliRefreshPolicy::Stale => Self::Stale,
            CliRefreshPolicy::All => Self::All,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliEvidenceSection {
    Rightmove,
    Description,
    Address,
    Facts,
    Claims,
    Broadband,
    Epc,
    Media,
    Verifications,
    Assessment,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliCorrectionKind {
    Address,
    Epc,
    Media,
}

impl From<CliCorrectionKind> for CorrectionKind {
    fn from(value: CliCorrectionKind) -> Self {
        match value {
            CliCorrectionKind::Address => Self::Address,
            CliCorrectionKind::Epc => Self::Epc,
            CliCorrectionKind::Media => Self::Media,
        }
    }
}

impl From<CliEvidenceSection> for EvidenceSection {
    fn from(value: CliEvidenceSection) -> Self {
        match value {
            CliEvidenceSection::Rightmove => Self::Rightmove,
            CliEvidenceSection::Description => Self::Description,
            CliEvidenceSection::Address => Self::Address,
            CliEvidenceSection::Facts => Self::Facts,
            CliEvidenceSection::Claims => Self::Claims,
            CliEvidenceSection::Broadband => Self::Broadband,
            CliEvidenceSection::Epc => Self::Epc,
            CliEvidenceSection::Media => Self::Media,
            CliEvidenceSection::Verifications => Self::Verifications,
            CliEvidenceSection::Assessment => Self::Assessment,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliSourceTarget {
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

impl From<CliSourceTarget> for commands::sources::SourceTarget {
    fn from(value: CliSourceTarget) -> Self {
        match value {
            CliSourceTarget::All => Self::All,
            CliSourceTarget::Broadband => Self::Broadband,
            CliSourceTarget::Postcodes => Self::Postcodes,
            CliSourceTarget::Deprivation => Self::Deprivation,
            CliSourceTarget::Census => Self::Census,
            CliSourceTarget::Population => Self::Population,
            CliSourceTarget::Income => Self::Income,
            CliSourceTarget::Flood => Self::Flood,
            CliSourceTarget::Naptan => Self::Naptan,
            CliSourceTarget::Uprn => Self::Uprn,
            CliSourceTarget::Crime => Self::Crime,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliProgressMode {
    Auto,
    Plain,
    Off,
}

impl From<CliProgressMode> for commands::sources::ProgressMode {
    fn from(value: CliProgressMode) -> Self {
        match value {
            CliProgressMode::Auto => Self::Auto,
            CliProgressMode::Plain => Self::Plain,
            CliProgressMode::Off => Self::Off,
        }
    }
}

enum DispatchOutcome {
    Local {
        tool: &'static str,
        result: Result<CommandOutput, CommandError>,
    },
    Help {
        path: &'static [&'static str],
    },
}

impl DispatchOutcome {
    fn local(tool: &'static str, result: Result<CommandOutput, CommandError>) -> Self {
        Self::Local { tool, result }
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
        profile: cli.profile,
    };

    let started = Instant::now();
    let outcome = dispatch(command, &shared);

    match outcome {
        DispatchOutcome::Local { tool, result } => {
            let elapsed = started.elapsed().as_millis() as u64;
            let format = output_format(cli.toon);
            let exit_code = emit_result(&result, tool, elapsed, format);
            process::exit(exit_code);
        }
        DispatchOutcome::Help { path } => {
            process::exit(print_help(path));
        }
    }
}

fn discover_params_from_args(args: &SearchDiscoveryArgs) -> commands::search::DiscoverParams {
    commands::search::DiscoverParams {
        region: args.region.clone(),
        location: args.location.clone(),
        min_price: args.min_price,
        max_price: args.max_price,
        min_bedrooms: args.min_bedrooms,
        max_bedrooms: args.max_bedrooms,
        radius: args.radius,
        include_let_agreed: args.include_let_agreed,
        property_types: args.property_types.clone(),
        must_have: args.must_have.clone(),
        dont_show: args.dont_show.clone(),
        location_name: args.location_name.clone(),
        limit: args.limit,
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
            command: Some(ConfigCommand::Profiles),
        } => DispatchOutcome::local("config.profiles", commands::config::profiles(shared)),
        Command::Config {
            command: Some(ConfigCommand::External(args)),
        } => unsupported_external_group("config", args),
        Command::Config { command: None } => DispatchOutcome::help(&["config"]),
        Command::Area {
            command:
                Some(AreaCommand::Postcode {
                    postcode,
                    radius_m,
                    limit,
                }),
        } => DispatchOutcome::local(
            "area.postcode",
            commands::area::postcode(
                shared,
                commands::area::AreaPostcodeCommandParams {
                    postcode: postcode.clone(),
                    radius_m: *radius_m,
                    limit: *limit,
                },
            ),
        ),
        Command::Area {
            command: Some(AreaCommand::External(args)),
        } => unsupported_external_group("area", args),
        Command::Area { command: None } => DispatchOutcome::help(&["area"]),
        Command::Search {
            command: Some(SearchCommand::Resolve { location }),
        } => DispatchOutcome::local("search.resolve", commands::search::resolve(location)),
        Command::Search {
            command: Some(SearchCommand::Discover(args)),
        } => DispatchOutcome::local(
            "search.discover",
            commands::search::discover(shared, &discover_params_from_args(args)),
        ),
        Command::Search {
            command: Some(SearchCommand::Market(args)),
        } => DispatchOutcome::local(
            "search.market",
            commands::search::market(shared, &discover_params_from_args(args)),
        ),
        Command::Search { command: None } => DispatchOutcome::help(&["search"]),
        Command::Search {
            command: Some(SearchCommand::External(args)),
        } => unsupported_external_group("search", args),
        Command::Inspect {
            id_or_url,
            depth,
            refresh,
            section,
        } => DispatchOutcome::local(
            "inspect",
            commands::inspect::run(
                shared,
                commands::inspect::InspectCommandParams {
                    id_or_url: id_or_url.clone(),
                    depth: (*depth).into(),
                    refresh: (*refresh).into(),
                    sections: map_sections(section),
                },
            ),
        ),
        Command::Evidence {
            id,
            section,
            filters,
        } if id == "list" => {
            if !section.is_empty() {
                DispatchOutcome::local(
                    "evidence.list",
                    Err(CommandError::runtime(
                        "VALIDATION_ERROR",
                        "`--section` is only supported for `let evidence <id>`",
                        "remove `--section` or pass a listing id",
                    )),
                )
            } else {
                DispatchOutcome::local(
                    "evidence.list",
                    commands::evidence::list(shared, filters.into()),
                )
            }
        }
        Command::Evidence {
            id,
            section,
            filters,
        } => {
            if filters.has_values() {
                DispatchOutcome::local(
                    "evidence",
                    Err(CommandError::runtime(
                        "VALIDATION_ERROR",
                        "list filters require `let evidence list`",
                        "use `let evidence list --recommendation <value>` or remove the filters",
                    )),
                )
            } else {
                DispatchOutcome::local(
                    "evidence",
                    commands::evidence::run(
                        shared,
                        commands::evidence::EvidenceCommandParams {
                            id: id.clone(),
                            sections: map_sections(section),
                        },
                    ),
                )
            }
        }
        Command::Verify { id, claim, refresh } => DispatchOutcome::local(
            "verify",
            commands::verify::run(
                shared,
                commands::verify::VerifyCommandParams {
                    id: id.clone(),
                    claim: claim.clone(),
                    refresh: (*refresh).into(),
                },
            ),
        ),
        Command::Assess {
            command: Some(AssessCommand::Save { id, assessment }),
        } => DispatchOutcome::local(
            "assess.save",
            commands::agent_assess::save(shared, id, assessment),
        ),
        Command::Assess {
            command: Some(AssessCommand::Get { id }),
        } => DispatchOutcome::local("assess.get", commands::agent_assess::get(shared, id)),
        Command::Assess {
            command: Some(AssessCommand::List { filters }),
        } => DispatchOutcome::local(
            "assess.list",
            commands::agent_assess::list(shared, filters.into()),
        ),
        Command::Assess { command: None } => DispatchOutcome::help(&["assess"]),
        Command::Assess {
            command: Some(AssessCommand::External(args)),
        } => unsupported_external_group("assess", args),
        Command::Correct {
            command:
                Some(CorrectCommand::Address {
                    id,
                    address,
                    postcode,
                    lat,
                    lng,
                    note,
                }),
        } => DispatchOutcome::local(
            "correct.address",
            commands::correct::address(
                shared,
                commands::correct::AddressCorrectionParams {
                    id: id.clone(),
                    address: address.clone(),
                    postcode: postcode.clone(),
                    lat: *lat,
                    lng: *lng,
                    note: note.clone(),
                },
            ),
        ),
        Command::Correct {
            command:
                Some(CorrectCommand::Epc {
                    id,
                    certificate_url,
                    lmk_key,
                    uprn,
                    rating,
                    floor_area_sqm,
                    note,
                }),
        } => DispatchOutcome::local(
            "correct.epc",
            commands::correct::epc(
                shared,
                commands::correct::EpcCorrectionParams {
                    id: id.clone(),
                    certificate_url: certificate_url.clone(),
                    lmk_key: lmk_key.clone(),
                    uprn: uprn.clone(),
                    rating: rating.clone(),
                    floor_area_sqm: *floor_area_sqm,
                    note: note.clone(),
                },
            ),
        ),
        Command::Correct {
            command:
                Some(CorrectCommand::Media {
                    id,
                    map_lat,
                    map_lng,
                    note,
                }),
        } => DispatchOutcome::local(
            "correct.media",
            commands::correct::media(
                shared,
                commands::correct::MediaCorrectionParams {
                    id: id.clone(),
                    map_lat: *map_lat,
                    map_lng: *map_lng,
                    note: note.clone(),
                },
            ),
        ),
        Command::Correct {
            command:
                Some(CorrectCommand::Clear {
                    id,
                    kind,
                    correction_id,
                }),
        } => DispatchOutcome::local(
            "correct.clear",
            commands::correct::clear(
                shared,
                commands::correct::ClearCorrectionParams {
                    id: id.clone(),
                    kind: (*kind).into(),
                    correction_id: correction_id.clone(),
                },
            ),
        ),
        Command::Correct { command: None } => DispatchOutcome::help(&["correct"]),
        Command::Correct {
            command: Some(CorrectCommand::External(args)),
        } => unsupported_external_group("correct", args),
        Command::Sources {
            command: Some(SourcesCommand::List),
        } => DispatchOutcome::local("sources.list", commands::sources::list()),
        Command::Sources {
            command: Some(SourcesCommand::Status),
        } => DispatchOutcome::local("sources.status", commands::sources::status(shared)),
        Command::Sources {
            command:
                Some(SourcesCommand::Build {
                    target,
                    jobs,
                    progress,
                }),
        } => DispatchOutcome::local(
            "sources.build",
            commands::sources::build(shared, (*target).into(), *jobs, (*progress).into()),
        ),
        Command::Sources { command: None } => DispatchOutcome::help(&["sources"]),
        Command::Sources {
            command: Some(SourcesCommand::External(args)),
        } => unsupported_external_group("sources", args),
        Command::Start { id, section } => DispatchOutcome::local(
            "start",
            commands::start::run(
                shared,
                commands::start::StartParams {
                    id: id.clone(),
                    sections: map_sections(section),
                },
            ),
        ),
        Command::External(args) => unsupported_external(args),
    }
}

fn map_sections(values: &[CliEvidenceSection]) -> Vec<EvidenceSection> {
    values.iter().copied().map(Into::into).collect()
}

fn unsupported_external(args: &[String]) -> DispatchOutcome {
    if args.is_empty() {
        return DispatchOutcome::local(
            "external",
            Err(CommandError::runtime(
                "VALIDATION_ERROR",
                "missing command",
                "run `let tools` to list available commands",
            )),
        );
    }

    DispatchOutcome::local(
        "external",
        Err(CommandError::runtime(
            "UNSUPPORTED_COMMAND",
            format!("unsupported command: {}", args.join(" ")),
            "run `let tools` to list available commands",
        )),
    )
}

fn unsupported_external_group(group: &str, args: &[String]) -> DispatchOutcome {
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
        let result = unsupported_external(&[String::from("legacy-subcommand")]);
        match result {
            super::DispatchOutcome::Local { result, .. } => {
                let error = result.expect_err("expected unsupported command error");
                assert_eq!(error.code, "UNSUPPORTED_COMMAND");
            }
            super::DispatchOutcome::Help { .. } => panic!("expected local error outcome"),
        }
    }
}
