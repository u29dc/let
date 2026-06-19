#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::Command;

use let_sdk::intelligence::EvidenceSection;
use let_sdk::paths::resolve_paths;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone)]
pub struct StartParams {
    pub id: Option<String>,
    pub sections: Vec<EvidenceSection>,
}

pub fn run(shared: &SharedArgs, params: StartParams) -> CommandResult {
    let binary = resolve_tui_binary().ok_or_else(|| {
        CommandError::runtime(
            "TUI_NOT_FOUND",
            "could not locate `let-tui` binary",
            "build with `bun run build` or set LET_TUI_BIN",
        )
    })?;
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let section_names = params
        .sections
        .iter()
        .map(|section| section.as_str())
        .collect::<Vec<_>>()
        .join(",");

    let mut command = Command::new(&binary);
    command
        .env("LET_DATA_DIR", &paths.resolved.data)
        .env("LET_CONFIG_DIR", &paths.resolved.config)
        .env("LET_CACHE_DIR", &paths.resolved.cache)
        .env("LET_SOURCES_DIR", &paths.resolved.sources);

    if let Some(id) = &params.id {
        command.env("LET_START_ID", id);
    }

    if !section_names.is_empty() {
        command.env("LET_START_SECTIONS", &section_names);
    }

    let status = command.status().map_err(|error| {
        CommandError::runtime(
            "START_ERROR",
            format!("failed to start TUI at {}: {error}", binary.display()),
            "ensure the terminal supports crossterm and the binary is executable",
        )
    })?;

    if status.success() {
        Ok(CommandOutput::new(json!({
            "status": "exited",
            "code": status.code(),
            "binary": binary,
            "id": params.id,
            "sections": section_names,
        })))
    } else {
        Err(CommandError::new(
            "START_ERROR",
            format!("TUI exited with code {}", status.code().unwrap_or(1)),
            "check terminal output for crash details",
            status.code().unwrap_or(1),
        ))
    }
}

fn resolve_tui_binary() -> Option<PathBuf> {
    if let Ok(path) = env::var("LET_TUI_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let current_exe = env::current_exe().ok()?;
    let parent = current_exe.parent()?;

    let sibling = parent.join("let-tui");
    if sibling.is_file() {
        return Some(sibling);
    }

    let windows_sibling = parent.join("let-tui.exe");
    if windows_sibling.is_file() {
        return Some(windows_sibling);
    }

    None
}
