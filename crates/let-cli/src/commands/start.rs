#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::Command;

use let_sdk::paths::resolve_paths;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

pub fn run(shared: &SharedArgs) -> CommandResult {
    let binary = resolve_tui_binary().ok_or_else(|| {
        CommandError::runtime(
            "TUI_NOT_FOUND",
            "could not locate `let-tui` binary",
            "build with `cargo build --workspace --release` or set LET_TUI_BIN",
        )
    })?;
    let paths = resolve_paths(Some(shared.overrides.clone()));

    let status = Command::new(&binary)
        .env("LET_DATA_DIR", &paths.resolved.data)
        .env("LET_CONFIG_DIR", &paths.resolved.config)
        .env("LET_CACHE_DIR", &paths.resolved.cache)
        .env("LET_SOURCES_DIR", &paths.resolved.sources)
        .status()
        .map_err(|error| {
            CommandError::runtime(
                "START_ERROR",
                format!("failed to start tui at {}: {error}", binary.display()),
                "ensure terminal supports crossterm and binary is executable",
            )
        })?;

    if status.success() {
        Ok(CommandOutput::new(json!({
            "status": "exited",
            "code": 0,
        }))
        .with_text("tui session ended"))
    } else {
        Err(CommandError::new(
            "START_ERROR",
            format!("tui exited with code {}", status.code().unwrap_or(1)),
            "check terminal output for crash details",
            status.code().unwrap_or(1),
        ))
    }
}

fn resolve_tui_binary() -> Option<PathBuf> {
    if let Ok(path) = env::var("LET_TUI_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let current_exe = env::current_exe().ok()?;
    let parent = current_exe.parent()?;

    let mut sibling = parent.join("let-tui");
    if sibling.exists() {
        return Some(sibling);
    }

    sibling.set_extension("exe");
    if sibling.exists() {
        return Some(sibling);
    }

    None
}
