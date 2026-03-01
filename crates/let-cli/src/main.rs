#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process;
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use let_sdk::paths::PathOverrides;

mod commands;
mod envelope;
mod registry;

use commands::{CommandError, CommandOutput, SharedArgs};
use envelope::{ErrorEnvelope, ErrorPayload, Meta, SuccessEnvelope};

#[derive(Debug, Parser)]
#[command(name = "let", version, about = "Agent-native rental CLI")]
struct Cli {
    /// Emit one JSON envelope object on stdout.
    #[arg(long, global = true)]
    json: bool,

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

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
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
        /// Skip image and map downloads.
        #[arg(long, default_value_t = false)]
        skip_images: bool,
        /// Skip EPC enrichment.
        #[arg(long, default_value_t = false)]
        skip_epc: bool,
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
    },
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
    },
    /// Full listing details by id.
    Detail {
        /// Listing UUID or portal id.
        id: String,
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
        #[arg(long = "epc-rating")]
        epc_rating: Option<String>,
        #[arg(long = "floor-area")]
        floor_area: Option<f64>,
        #[arg(long, default_value_t = false)]
        skip_re_enrich: bool,
        #[arg(long, default_value_t = false)]
        skip_images: bool,
    },
    /// Prune listings by score, region, or inactive status.
    Prune {
        #[arg(long, default_value_t = 50.0)]
        min_score: f64,
        #[arg(long)]
        bottom: Option<u8>,
        #[arg(long)]
        region: Option<String>,
        #[arg(long, default_value_t = false)]
        inactive: bool,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
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

enum DispatchOutcome {
    Local {
        tool: &'static str,
        result: Result<CommandOutput, CommandError>,
    },
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
    let outcome = dispatch(&cli.command, &shared, cli.json);

    match outcome {
        DispatchOutcome::Local { tool, result } => {
            let elapsed = started.elapsed().as_millis() as u64;
            let exit_code = emit(&result, tool, elapsed, cli.json);
            process::exit(exit_code);
        }
    }
}

fn dispatch(command: &Command, shared: &SharedArgs, json_mode: bool) -> DispatchOutcome {
    match command {
        Command::Tools { name } => DispatchOutcome::Local {
            tool: "tools",
            result: commands::tools::run(name.as_deref()),
        },
        Command::Health => DispatchOutcome::Local {
            tool: "health",
            result: commands::health::run(shared),
        },
        Command::Config {
            command: ConfigCommand::Show,
        } => DispatchOutcome::Local {
            tool: "config.show",
            result: commands::config::show(shared),
        },
        Command::Config {
            command: ConfigCommand::Validate,
        } => DispatchOutcome::Local {
            tool: "config.validate",
            result: commands::config::validate(shared),
        },
        Command::Start => DispatchOutcome::Local {
            tool: "start",
            result: commands::start::run(json_mode),
        },
        Command::Score {
            command: ScoreCommand::Compute,
        } => DispatchOutcome::Local {
            tool: "score.compute",
            result: commands::score::compute(shared),
        },
        Command::Score {
            command: ScoreCommand::Explain { id },
        } => DispatchOutcome::Local {
            tool: "score.explain",
            result: commands::score::explain(shared, id),
        },
        Command::View {
            command:
                ViewCommand::List {
                    top,
                    min_score,
                    sort,
                    asc,
                    region,
                    property_type,
                },
        } => DispatchOutcome::Local {
            tool: "view.list",
            result: commands::view::list(
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
        },
        Command::View {
            command: ViewCommand::Detail { id },
        } => DispatchOutcome::Local {
            tool: "view.detail",
            result: commands::view::detail(shared, id),
        },
        Command::Assess {
            command:
                AssessCommand::Candidates {
                    top,
                    region,
                    min_score,
                },
        } => DispatchOutcome::Local {
            tool: "assess.candidates",
            result: commands::assess::candidates(
                shared,
                &commands::assess::CandidatesParams {
                    top: if *top == 0 { 10 } else { *top },
                    region: region.clone(),
                    min_score: *min_score,
                },
            ),
        },
        Command::Assess {
            command: AssessCommand::Context { id },
        } => DispatchOutcome::Local {
            tool: "assess.context",
            result: commands::assess::context(shared, id),
        },
        Command::Assess {
            command: AssessCommand::Submit { id, assessment },
        } => DispatchOutcome::Local {
            tool: "assess.submit",
            result: commands::assess::submit(shared, id, assessment),
        },
        Command::Search {
            command: SearchCommand::Resolve { location },
        } => DispatchOutcome::Local {
            tool: "search.resolve",
            result: commands::search::resolve(location),
        },
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
        } => DispatchOutcome::Local {
            tool: "search.discover",
            result: commands::search::discover(
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
        },
        Command::Search {
            command: SearchCommand::Diff { ids },
        } => DispatchOutcome::Local {
            tool: "search.diff",
            result: commands::search::diff(shared, ids),
        },
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
                    epc_rating,
                    floor_area,
                    skip_re_enrich,
                    skip_images,
                },
        } => DispatchOutcome::Local {
            tool: "ops.patch",
            result: commands::ops::patch(
                shared,
                &commands::ops::PatchParams {
                    id: id.clone(),
                    address: address.clone(),
                    postcode: postcode.clone(),
                    lat: *lat,
                    lng: *lng,
                    epc_rating: epc_rating.clone(),
                    floor_area: *floor_area,
                    skip_re_enrich: *skip_re_enrich,
                    skip_images: *skip_images,
                },
            ),
        },
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
        } => DispatchOutcome::Local {
            tool: "ops.prune",
            result: commands::ops::prune(
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
        },
        Command::Ops {
            command:
                OpsCommand::Verify {
                    dry_run,
                    region,
                    limit,
                    delay,
                },
        } => DispatchOutcome::Local {
            tool: "ops.verify",
            result: commands::ops::verify(
                shared,
                &commands::ops::VerifyParams {
                    dry_run: *dry_run,
                    region: region.clone(),
                    limit: *limit,
                    delay_ms: *delay,
                },
            ),
        },
        Command::Ops {
            command: OpsCommand::External(args),
        } => unsupported_external("ops", args),
        Command::Export {
            command: ExportCommand::Json { output },
        } => DispatchOutcome::Local {
            tool: "export.json",
            result: commands::export::export_json(shared, output.clone()),
        },
        Command::Export {
            command:
                ExportCommand::Notion {
                    top,
                    min_score,
                    region,
                    dry_run,
                    force,
                },
        } => DispatchOutcome::Local {
            tool: "export.notion",
            result: commands::export::export_notion(
                shared,
                &commands::export::NotionParams {
                    top: *top,
                    min_score: *min_score,
                    region: region.clone(),
                    dry_run: *dry_run,
                    force: *force,
                },
            ),
        },
        Command::Export {
            command: ExportCommand::External(args),
        } => unsupported_external("export", args),
        Command::Build {
            command: BuildCommand::Sources { target, jobs },
        } => DispatchOutcome::Local {
            tool: "build.sources",
            result: commands::build::run_sources((*target).into(), *jobs, shared, json_mode),
        },
        Command::Fetch {
            ids,
            region,
            skip_images,
            skip_epc,
        } => DispatchOutcome::Local {
            tool: "fetch",
            result: commands::fetch::run(
                shared,
                &commands::fetch::FetchParams {
                    ids: ids.clone(),
                    region: region.clone(),
                    skip_images: *skip_images,
                    skip_epc: *skip_epc,
                },
            ),
        },
        Command::External(args) => {
            if args.is_empty() {
                DispatchOutcome::Local {
                    tool: "external",
                    result: Err(CommandError::runtime(
                        "VALIDATION_ERROR",
                        "missing command",
                        "run `let tools --json` to list available commands",
                    )),
                }
            } else {
                DispatchOutcome::Local {
                    tool: "external",
                    result: Err(CommandError::runtime(
                        "UNSUPPORTED_COMMAND",
                        format!("unsupported command: {}", args.join(" ")),
                        "run `let tools --json` to list available commands",
                    )),
                }
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

    DispatchOutcome::Local {
        tool: "external",
        result: Err(CommandError::runtime(
            "UNSUPPORTED_COMMAND",
            detail,
            "run `let tools --json` to list available commands",
        )),
    }
}

fn emit(result: &Result<CommandOutput, CommandError>, tool: &str, elapsed: u64, json: bool) -> i32 {
    if json {
        emit_json(result, tool, elapsed)
    } else {
        emit_text(result)
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
                ErrorPayload::new(err.code.clone(), err.message.clone(), err.hint.clone()),
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
        Ok(output) => {
            if let Some(text) = &output.text {
                println!("{text}");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output.data)
                        .expect("text payload pretty serialization failed")
                );
            }
            0
        }
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
