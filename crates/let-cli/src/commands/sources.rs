#![forbid(unsafe_code)]

use std::fs;

use let_sdk::paths::resolve_paths;
use serde::Serialize;
use serde_json::json;

use crate::commands::build as build_command;
use crate::commands::{CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTarget {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    Auto,
    Plain,
    Off,
}

pub fn list() -> CommandResult {
    let sources = let_sdk::sources::list_sources();
    Ok(CommandOutput::new(json!({
        "sources": sources,
        "defaultJobs": 3,
    }))
    .with_count(sources.len())
    .with_total(sources.len())
    .with_has_more(false))
}

pub fn status(shared: &SharedArgs) -> CommandResult {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let items = let_sdk::sources::list_sources()
        .iter()
        .map(|source| {
            let path = paths.derived.source_db(&paths.resolved.sources, source);
            let metadata = fs::metadata(&path).ok();
            SourceStatus {
                source: (*source).to_owned(),
                path: path.display().to_string(),
                present: metadata.is_some(),
                size_bytes: metadata.map(|item| item.len()),
            }
        })
        .collect::<Vec<_>>();
    let present = items.iter().filter(|item| item.present).count();
    let total = items.len();
    Ok(CommandOutput::new(json!({
        "sources": items,
        "present": present,
        "missing": total - present,
    }))
    .with_count(total)
    .with_total(total)
    .with_has_more(false))
}

pub fn build(
    shared: &SharedArgs,
    target: SourceTarget,
    jobs: usize,
    progress: ProgressMode,
) -> CommandResult {
    build_command::run_sources(
        map_target(target),
        jobs,
        shared,
        match progress {
            ProgressMode::Auto => build_command::ProgressMode::Auto,
            ProgressMode::Plain => build_command::ProgressMode::Plain,
            ProgressMode::Off => build_command::ProgressMode::Off,
        },
    )
}

fn map_target(target: SourceTarget) -> build_command::SourceTarget {
    match target {
        SourceTarget::All => build_command::SourceTarget::All,
        SourceTarget::Broadband => build_command::SourceTarget::Broadband,
        SourceTarget::Postcodes => build_command::SourceTarget::Postcodes,
        SourceTarget::Deprivation => build_command::SourceTarget::Deprivation,
        SourceTarget::Census => build_command::SourceTarget::Census,
        SourceTarget::Population => build_command::SourceTarget::Population,
        SourceTarget::Income => build_command::SourceTarget::Income,
        SourceTarget::Flood => build_command::SourceTarget::Flood,
        SourceTarget::Naptan => build_command::SourceTarget::Naptan,
        SourceTarget::Uprn => build_command::SourceTarget::Uprn,
        SourceTarget::Crime => build_command::SourceTarget::Crime,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceStatus {
    source: String,
    path: String,
    present: bool,
    size_bytes: Option<u64>,
}
