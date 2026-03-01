#![forbid(unsafe_code)]

use std::fs;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceRunResult {
    source: String,
    exit_code: i32,
    duration_ms: u64,
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
    let job_count = jobs.max(1);
    let source_results = match target {
        SourceTarget::All => run_all_sources(job_count, shared, json_mode)?,
        _ => vec![run_one_source(target.as_str(), shared, json_mode)?],
    };

    let failed = source_results
        .iter()
        .filter(|result| result.exit_code != 0)
        .map(|result| result.source.clone())
        .collect::<Vec<_>>();

    if !failed.is_empty() {
        return Err(CommandError::new(
            "SOURCE_BUILD_FAILED",
            format!(
                "source build failed for target `{}` (failed: {})",
                target.as_str(),
                failed.join(", ")
            ),
            "inspect command output for download or parsing errors",
            1,
        ));
    }

    let elapsed = started.elapsed().as_millis() as u64;
    let payload = BuildResult {
        target: target.as_str().to_owned(),
        jobs: job_count,
        duration_ms: elapsed,
        status: "ok".to_owned(),
    };

    Ok(CommandOutput::new(json!({
        "target": payload.target,
        "jobs": payload.jobs,
        "durationMs": payload.duration_ms,
        "status": payload.status,
        "sources": source_results,
    }))
    .with_text(format!(
        "build sources {} completed in {}ms",
        target.as_str(),
        elapsed
    )))
}

fn run_all_sources(
    jobs: usize,
    shared: &SharedArgs,
    json_mode: bool,
) -> Result<Vec<SourceRunResult>, CommandError> {
    let sources = SourceTarget::all_sources()
        .iter()
        .map(|source| source.to_string())
        .collect::<Vec<_>>();

    let (task_tx, task_rx) = mpsc::channel::<String>();
    let (result_tx, result_rx) = mpsc::channel::<Result<SourceRunResult, CommandError>>();
    let task_rx = Arc::new(Mutex::new(task_rx));
    let worker_count = jobs.min(sources.len().max(1));

    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let task_rx = Arc::clone(&task_rx);
        let result_tx = result_tx.clone();
        let shared = shared.clone();
        workers.push(thread::spawn(move || {
            loop {
                let next = {
                    let receiver = task_rx.lock().expect("worker receiver lock poisoned");
                    receiver.recv()
                };
                let Ok(source) = next else {
                    break;
                };
                let result = run_one_source(&source, &shared, json_mode);
                let _ = result_tx.send(result);
            }
        }));
    }
    drop(result_tx);

    for source in sources {
        task_tx.send(source).map_err(|error| {
            CommandError::runtime(
                "PROCESS_ERROR",
                format!("failed to queue source build: {error}"),
                "retry source build command",
            )
        })?;
    }
    drop(task_tx);

    let mut results = Vec::new();
    for result in result_rx {
        results.push(result?);
    }

    for worker in workers {
        let _ = worker.join();
    }

    results.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(results)
}

fn run_one_source(
    source: &str,
    shared: &SharedArgs,
    json_mode: bool,
) -> Result<SourceRunResult, CommandError> {
    let started = Instant::now();
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.source_db(&paths.resolved.sources, source);
    if db_path.exists() {
        let _ = fs::remove_file(&db_path);
    }

    let script = format!("scripts/sources/{source}.ts");
    let exit_code = run_child(&["run", &script], json_mode)?;
    Ok(SourceRunResult {
        source: source.to_owned(),
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
    })
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
