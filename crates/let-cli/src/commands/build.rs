#![forbid(unsafe_code)]

use std::fs;
use std::process::{Command, Stdio};
use std::time::Instant;

use let_sdk::paths::resolve_paths;
use serde::Serialize;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTarget {
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

impl SourceTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::All => "all",
            Self::Broadband => "broadband",
            Self::Postcodes => "postcodes",
            Self::Deprivation => "deprivation",
            Self::Census => "census",
            Self::Population => "population",
            Self::Income => "income",
            Self::Flood => "flood",
            Self::Naptan => "naptan",
            Self::Uprn => "uprn",
            Self::Crime => "crime",
        }
    }

    pub fn all_sources() -> &'static [&'static str] {
        &[
            "broadband",
            "postcodes",
            "deprivation",
            "census",
            "population",
            "income",
            "flood",
            "naptan",
            "uprn",
            "crime",
        ]
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildResult {
    target: String,
    jobs: usize,
    duration_ms: u64,
    status: String,
}

pub fn run_sources(
    target: SourceTarget,
    jobs: usize,
    shared: &SharedArgs,
    json_mode: bool,
) -> CommandResult {
    if target == SourceTarget::List {
        return Ok(CommandOutput::new(json!({
            "sources": SourceTarget::all_sources(),
            "defaultJobs": 3,
        }))
        .with_count(SourceTarget::all_sources().len())
        .with_total(SourceTarget::all_sources().len())
        .with_has_more(false)
        .with_text("available sources listed"));
    }

    let started = Instant::now();
    let status_code = match target {
        SourceTarget::All => run_child(
            &[
                "run",
                "scripts/build-sources.ts",
                "--concurrency",
                &jobs.to_string(),
            ],
            json_mode,
        )?,
        _ => {
            let source = target.as_str();
            let paths = resolve_paths(Some(shared.overrides.clone()));
            let db_path = paths.derived.source_db(&paths.resolved.sources, source);
            if db_path.exists() {
                let _ = fs::remove_file(&db_path);
            }
            let script = format!("scripts/sources/{source}.ts");
            run_child(&["run", &script], json_mode)?
        }
    };

    if status_code != 0 {
        return Err(CommandError::new(
            "SOURCE_BUILD_FAILED",
            format!(
                "source build failed for target `{}` with exit code {}",
                target.as_str(),
                status_code
            ),
            "inspect command output for download or parsing errors",
            status_code,
        ));
    }

    let elapsed = started.elapsed().as_millis() as u64;
    let payload = BuildResult {
        target: target.as_str().to_owned(),
        jobs,
        duration_ms: elapsed,
        status: "ok".to_owned(),
    };

    Ok(CommandOutput::new(
        serde_json::to_value(payload).expect("build payload serialization failed"),
    )
    .with_text(format!(
        "build sources {} completed in {}ms",
        target.as_str(),
        elapsed
    )))
}

fn run_child(args: &[&str], json_mode: bool) -> Result<i32, CommandError> {
    if json_mode {
        let output = Command::new("bun")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| {
                CommandError::runtime(
                    "PROCESS_ERROR",
                    format!("failed to run bun command: {error}"),
                    "ensure bun is installed and available on PATH",
                )
            })?;

        if !output.stdout.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(output.status.code().unwrap_or(1))
    } else {
        let status = Command::new("bun").args(args).status().map_err(|error| {
            CommandError::runtime(
                "PROCESS_ERROR",
                format!("failed to run bun command: {error}"),
                "ensure bun is installed and available on PATH",
            )
        })?;
        Ok(status.code().unwrap_or(1))
    }
}
