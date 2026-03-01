#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process;
use std::process::Command as ProcessCommand;
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
    /// Build source databases.
    Build {
        #[command(subcommand)]
        command: BuildCommand,
    },
    /// Delegate command groups not yet ported to the archive-compatible TS CLI.
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
    Delegated {
        code: i32,
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
        DispatchOutcome::Delegated { code } => {
            process::exit(code);
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
        Command::Build {
            command: BuildCommand::Sources { target, jobs },
        } => DispatchOutcome::Local {
            tool: "build.sources",
            result: commands::build::run_sources((*target).into(), *jobs, shared, json_mode),
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
                DispatchOutcome::Delegated {
                    code: delegate_to_legacy(args, shared, json_mode),
                }
            }
        }
    }
}

fn delegate_to_legacy(args: &[String], shared: &SharedArgs, json_mode: bool) -> i32 {
    let mut full_args: Vec<String> = vec![
        "run".to_owned(),
        "packages/cli/src/index.ts".to_owned(),
    ];

    if json_mode {
        full_args.push("--json".to_owned());
    }
    if let Some(path) = &shared.overrides.data_dir {
        full_args.push("--data-dir".to_owned());
        full_args.push(path.display().to_string());
    }
    if let Some(path) = &shared.overrides.config_dir {
        full_args.push("--config-dir".to_owned());
        full_args.push(path.display().to_string());
    }
    if let Some(path) = &shared.overrides.cache_dir {
        full_args.push("--cache-dir".to_owned());
        full_args.push(path.display().to_string());
    }
    if let Some(path) = &shared.overrides.sources_dir {
        full_args.push("--sources-dir".to_owned());
        full_args.push(path.display().to_string());
    }

    full_args.extend(args.iter().cloned());

    match ProcessCommand::new("bun").args(&full_args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("failed to delegate command to legacy bun cli: {error}");
            1
        }
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
