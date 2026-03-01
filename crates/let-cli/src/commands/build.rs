#![forbid(unsafe_code)]

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
        let_sdk::sources::list_sources()
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
    rows: usize,
    duration_ms: u64,
    db_path: String,
    status: String,
}

pub fn run_sources(
    target: SourceTarget,
    jobs: usize,
    shared: &SharedArgs,
    _json_mode: bool,
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
        SourceTarget::All => run_all_sources(job_count, shared)?,
        _ => vec![run_one_source(target.as_str(), shared)?],
    };

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

fn run_all_sources(jobs: usize, shared: &SharedArgs) -> Result<Vec<SourceRunResult>, CommandError> {
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

                let result = run_one_source(&source, &shared);
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

fn run_one_source(source: &str, shared: &SharedArgs) -> Result<SourceRunResult, CommandError> {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let report =
        let_sdk::sources::build_source(&paths.resolved.sources, source).map_err(map_sdk_error)?;

    Ok(SourceRunResult {
        source: report.source,
        rows: report.rows,
        duration_ms: report.duration_ms,
        db_path: report.db_path.display().to_string(),
        status: "ok".to_owned(),
    })
}

fn map_sdk_error(error: let_sdk::errors::LetError) -> CommandError {
    let exit_code = error.exit_code();
    let code = error.code.as_str().to_owned();
    let message = error.message;
    let hint = error.hint;
    CommandError::new(code, message, hint, exit_code)
}
