#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process;
use std::time::Instant;

use clap::{Parser, Subcommand};
use let_sdk::paths::PathOverrides;

mod commands;
mod envelope;
mod registry;

use commands::{CommandError, CommandOutput, CommandResult, SharedArgs};
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
    /// Start the CLI workflow runtime.
    Start,
    /// Placeholder command group.
    Search,
    /// Placeholder command group.
    Fetch,
    /// Placeholder command group.
    View,
    /// Placeholder command group.
    Score,
    /// Placeholder command group.
    Assess,
    /// Placeholder command group.
    Export,
    /// Placeholder command group.
    Ops,
    /// Placeholder command group.
    Build,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Show parsed config.
    Show,
    /// Validate config and report issues.
    Validate,
}

impl Command {
    fn tool_name(&self) -> &'static str {
        match self {
            Self::Tools { .. } => "tools",
            Self::Health => "health",
            Self::Config {
                command: ConfigCommand::Show,
            } => "config.show",
            Self::Config {
                command: ConfigCommand::Validate,
            } => "config.validate",
            Self::Start => "start",
            Self::Search => "search",
            Self::Fetch => "fetch",
            Self::View => "view",
            Self::Score => "score",
            Self::Assess => "assess",
            Self::Export => "export",
            Self::Ops => "ops",
            Self::Build => "build",
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let started = Instant::now();
    let tool_name = cli.command.tool_name();

    let shared = SharedArgs {
        overrides: PathOverrides {
            data_dir: cli.data_dir,
            config_dir: cli.config_dir,
            cache_dir: cli.cache_dir,
            sources_dir: cli.sources_dir,
        },
    };

    let result = dispatch(&cli.command, &shared);
    let elapsed = started.elapsed().as_millis() as u64;
    let exit_code = emit(&result, tool_name, elapsed, cli.json);
    process::exit(exit_code);
}

fn dispatch(command: &Command, shared: &SharedArgs) -> CommandResult {
    match command {
        Command::Tools { name } => commands::tools::run(name.as_deref()),
        Command::Health => commands::health::run(shared),
        Command::Config {
            command: ConfigCommand::Show,
        } => commands::config::show(shared),
        Command::Config {
            command: ConfigCommand::Validate,
        } => commands::config::validate(shared),
        Command::Start => commands::start::run(),
        Command::Search => commands::placeholder("search"),
        Command::Fetch => commands::placeholder("fetch"),
        Command::View => commands::placeholder("view"),
        Command::Score => commands::placeholder("score"),
        Command::Assess => commands::placeholder("assess"),
        Command::Export => commands::placeholder("export"),
        Command::Ops => commands::placeholder("ops"),
        Command::Build => commands::placeholder("build"),
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
