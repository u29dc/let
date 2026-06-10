#![forbid(unsafe_code)]

use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    Auto,
    Plain,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressStyle {
    Tty,
    Plain,
    Off,
}

impl ProgressMode {
    fn resolve(self) -> ProgressStyle {
        match self {
            Self::Off => ProgressStyle::Off,
            Self::Plain => ProgressStyle::Plain,
            Self::Auto => {
                if io::stderr().is_terminal() {
                    ProgressStyle::Tty
                } else {
                    ProgressStyle::Plain
                }
            }
        }
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

#[derive(Debug)]
enum WorkerEvent {
    Started(String),
    Finished {
        source: String,
        result: Result<SourceRunResult, CommandError>,
    },
}

struct ProgressReporter {
    style: ProgressStyle,
    target: String,
    total: usize,
    jobs: usize,
    started: Instant,
    spinner_index: usize,
    last_plain_tick: Instant,
}

impl ProgressReporter {
    fn new(style: ProgressStyle, target: &str, total: usize, jobs: usize) -> Self {
        let now = Instant::now();
        Self {
            style,
            target: target.to_owned(),
            total,
            jobs,
            started: now,
            spinner_index: 0,
            last_plain_tick: now,
        }
    }

    fn begin(&mut self) {
        match self.style {
            ProgressStyle::Off => {}
            ProgressStyle::Plain => {
                eprintln!(
                    "build sources {} started (sources={}, jobs={})",
                    self.target, self.total, self.jobs
                );
            }
            ProgressStyle::Tty => self.render_tty(0, &[]),
        }
    }

    fn on_started(&mut self, source: &str, completed: usize, active: &[String]) {
        match self.style {
            ProgressStyle::Off => {}
            ProgressStyle::Plain => {
                eprintln!("starting {source} ({completed}/{})", self.total);
            }
            ProgressStyle::Tty => self.render_tty(completed, active),
        }
    }

    fn on_finished_ok(
        &mut self,
        source: &str,
        rows: usize,
        duration_ms: u64,
        completed: usize,
        active: &[String],
    ) {
        match self.style {
            ProgressStyle::Off => {}
            ProgressStyle::Plain => {
                eprintln!("ok {source} rows={rows} duration={}ms", duration_ms);
            }
            ProgressStyle::Tty => self.render_tty(completed, active),
        }
    }

    fn on_finished_error(
        &mut self,
        source: &str,
        error: &CommandError,
        completed: usize,
        active: &[String],
    ) {
        match self.style {
            ProgressStyle::Off => {}
            ProgressStyle::Plain => {
                eprintln!("fail {source} {}: {}", error.code, error.message);
            }
            ProgressStyle::Tty => self.render_tty(completed, active),
        }
    }

    fn tick(&mut self, completed: usize, active: &[String]) {
        match self.style {
            ProgressStyle::Off => {}
            ProgressStyle::Plain => {
                if self.last_plain_tick.elapsed() >= Duration::from_secs(10) {
                    eprintln!(
                        "progress {completed}/{} active={} elapsed={}",
                        self.total,
                        summarize_active(active),
                        format_elapsed(self.started.elapsed())
                    );
                    self.last_plain_tick = Instant::now();
                }
            }
            ProgressStyle::Tty => self.render_tty(completed, active),
        }
    }

    fn finish(&mut self, succeeded: usize, failed: usize) {
        match self.style {
            ProgressStyle::Off => {}
            ProgressStyle::Plain => {
                eprintln!(
                    "build sources {} finished: ok={}, failed={}, elapsed={}",
                    self.target,
                    succeeded,
                    failed,
                    format_elapsed(self.started.elapsed())
                );
            }
            ProgressStyle::Tty => {
                let summary = format!(
                    "\r\x1b[2Kbuild {} done {}/{} ok, {} failed, elapsed {}",
                    self.target,
                    succeeded,
                    self.total,
                    failed,
                    format_elapsed(self.started.elapsed())
                );
                let mut stderr = io::stderr().lock();
                let _ = writeln!(stderr, "{summary}");
                let _ = stderr.flush();
            }
        }
    }

    fn render_tty(&mut self, completed: usize, active: &[String]) {
        const SPINNER: [&str; 4] = ["-", "\\", "|", "/"];
        let mut stderr = io::stderr().lock();
        let spinner = SPINNER[self.spinner_index % SPINNER.len()];
        self.spinner_index = self.spinner_index.wrapping_add(1);
        let bar = render_bar(completed, self.total, 24);
        let active_text = summarize_active(active);
        let elapsed = format_elapsed(self.started.elapsed());
        let line = format!(
            "\r\x1b[2K{spinner} build {} {completed}/{} {bar} active:{active_text} elapsed:{elapsed}",
            self.target, self.total
        );
        let _ = write!(stderr, "{line}");
        let _ = stderr.flush();
    }
}

pub fn run_sources(
    target: SourceTarget,
    jobs: usize,
    shared: &SharedArgs,
    progress_mode: ProgressMode,
) -> CommandResult {
    if target == SourceTarget::List {
        return Ok(CommandOutput::new(json!({
            "sources": SourceTarget::all_sources(),
            "defaultJobs": 3,
        }))
        .with_count(SourceTarget::all_sources().len())
        .with_total(SourceTarget::all_sources().len())
        .with_has_more(false));
    }

    let started = Instant::now();
    let job_count = jobs.max(1);
    let progress_style = progress_mode.resolve();
    let source_results = match target {
        SourceTarget::All => run_all_sources(job_count, shared, progress_style, target.as_str())?,
        _ => vec![run_single_source(
            target.as_str(),
            shared,
            progress_style,
            job_count,
        )?],
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
    })))
}

fn run_all_sources(
    jobs: usize,
    shared: &SharedArgs,
    progress_style: ProgressStyle,
    target_label: &str,
) -> Result<Vec<SourceRunResult>, CommandError> {
    let sources = SourceTarget::all_sources()
        .iter()
        .map(|source| source.to_string())
        .collect::<Vec<_>>();

    let (task_tx, task_rx) = mpsc::channel::<String>();
    let (event_tx, event_rx) = mpsc::channel::<WorkerEvent>();
    let task_rx = Arc::new(Mutex::new(task_rx));
    let worker_count = jobs.min(sources.len().max(1));
    let total = sources.len();

    let mut reporter = ProgressReporter::new(progress_style, target_label, total, worker_count);
    reporter.begin();

    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let task_rx = Arc::clone(&task_rx);
        let event_tx = event_tx.clone();
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

                let _ = event_tx.send(WorkerEvent::Started(source.clone()));
                let result = run_one_source(&source, &shared);
                let _ = event_tx.send(WorkerEvent::Finished { source, result });
            }
        }));
    }
    drop(event_tx);

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
    let mut first_error: Option<CommandError> = None;
    let mut failed_count = 0usize;
    let mut active = Vec::new();
    let mut completed = 0usize;

    while completed < total {
        match event_rx.recv_timeout(Duration::from_millis(150)) {
            Ok(WorkerEvent::Started(source)) => {
                active.push(source.clone());
                reporter.on_started(&source, completed, &active);
            }
            Ok(WorkerEvent::Finished { source, result }) => {
                completed += 1;
                active.retain(|entry| entry != &source);
                match result {
                    Ok(run) => {
                        reporter.on_finished_ok(
                            &run.source,
                            run.rows,
                            run.duration_ms,
                            completed,
                            &active,
                        );
                        results.push(run);
                    }
                    Err(error) => {
                        failed_count += 1;
                        reporter.on_finished_error(&source, &error, completed, &active);
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                reporter.tick(completed, &active);
            }
            Err(RecvTimeoutError::Disconnected) => {
                if completed < total && first_error.is_none() {
                    failed_count += total - completed;
                    first_error = Some(CommandError::runtime(
                        "PROCESS_ERROR",
                        "build worker channel disconnected unexpectedly",
                        "retry build sources command",
                    ));
                }
                break;
            }
        }
    }

    for worker in workers {
        let _ = worker.join();
    }

    reporter.finish(results.len(), failed_count);
    if let Some(error) = first_error {
        return Err(error);
    }

    results.sort_by(|a, b| a.source.cmp(&b.source));
    Ok(results)
}

fn run_single_source(
    source: &str,
    shared: &SharedArgs,
    progress_style: ProgressStyle,
    jobs: usize,
) -> Result<SourceRunResult, CommandError> {
    let mut reporter = ProgressReporter::new(progress_style, source, 1, jobs);
    reporter.begin();
    let active = vec![source.to_owned()];
    reporter.on_started(source, 0, &active);

    let result = run_one_source(source, shared);
    match &result {
        Ok(run) => {
            reporter.on_finished_ok(&run.source, run.rows, run.duration_ms, 1, &[]);
            reporter.finish(1, 0);
        }
        Err(error) => {
            reporter.on_finished_error(source, error, 1, &[]);
            reporter.finish(0, 1);
        }
    }

    result
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

fn summarize_active(active: &[String]) -> String {
    if active.is_empty() {
        return "-".to_owned();
    }

    let preview = active.iter().take(3).cloned().collect::<Vec<_>>().join(",");
    if active.len() > 3 {
        format!("{preview}+{}", active.len() - 3)
    } else {
        preview
    }
}

fn render_bar(completed: usize, total: usize, width: usize) -> String {
    if width == 0 {
        return "[]".to_owned();
    }
    if total == 0 {
        return format!("[{}]", ".".repeat(width));
    }
    let filled = ((completed.saturating_mul(width)) / total).min(width);
    format!("[{}{}]", "=".repeat(filled), ".".repeat(width - filled))
}

fn format_elapsed(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::{ProgressMode, ProgressStyle, format_elapsed, render_bar};
    use std::time::Duration;

    #[test]
    fn auto_progress_uses_terminal_detection() {
        let resolved = ProgressMode::Auto.resolve();
        assert!(matches!(
            resolved,
            ProgressStyle::Tty | ProgressStyle::Plain
        ));
    }

    #[test]
    fn plain_progress_stays_plain() {
        assert_eq!(ProgressMode::Plain.resolve(), ProgressStyle::Plain);
    }

    #[test]
    fn progress_bar_renders_expected_fill() {
        assert_eq!(render_bar(0, 4, 8), "[........]");
        assert_eq!(render_bar(2, 4, 8), "[====....]");
        assert_eq!(render_bar(4, 4, 8), "[========]");
    }

    #[test]
    fn elapsed_format_is_mm_ss() {
        assert_eq!(format_elapsed(Duration::from_secs(5)), "00:05");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "02:05");
    }
}
