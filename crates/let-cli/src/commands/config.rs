#![forbid(unsafe_code)]

use let_sdk::config::{load_config, reset_config_cache};
use let_sdk::paths::resolve_paths;
use serde_json::json;

use crate::commands::{CommandOutput, CommandResult, SharedArgs};

pub fn show(shared: &SharedArgs) -> CommandResult {
    let path = resolve_paths(Some(shared.overrides.clone()))
        .derived
        .config_file;
    reset_config_cache();
    let config = load_config(Some(&path))?;
    let path_display = path.display().to_string();
    let location_count = config.search.locations.len();

    let data = json!({
        "path": path_display,
        "config": config,
    });

    Ok(CommandOutput::new(data)
        .with_count(location_count)
        .with_text(format!("config loaded from {}", path.display())))
}

pub fn validate(shared: &SharedArgs) -> CommandResult {
    let path = resolve_paths(Some(shared.overrides.clone()))
        .derived
        .config_file;
    reset_config_cache();
    let config = load_config(Some(&path))?;
    config.validate()?;
    let path_display = path.display().to_string();

    let data = json!({
        "path": path_display,
        "valid": true,
        "errors": [],
    });
    Ok(CommandOutput::new(data).with_text(format!("config valid: {}", path.display())))
}
