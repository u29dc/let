#![forbid(unsafe_code)]

use let_sdk::config::{config_profiles_dir, list_config_profiles, load_config};
use serde_json::json;

use crate::commands::{CommandOutput, CommandResult, SharedArgs};

pub fn show(shared: &SharedArgs) -> CommandResult {
    let paths = shared.resolved_paths();
    let path = shared.config_path(&paths)?;
    let config = load_config(Some(&path))?;
    let path_display = path.display().to_string();
    let location_count = config.search.locations.len();

    let data = json!({
        "path": path_display,
        "profile": shared.profile.as_deref(),
        "config": config,
    });

    Ok(CommandOutput::new(data).with_count(location_count))
}

pub fn profiles(shared: &SharedArgs) -> CommandResult {
    let default_path = shared.resolved_paths().derived.config_file;
    let profile_dir = config_profiles_dir(&default_path);
    let profiles = list_config_profiles(&default_path)?
        .into_iter()
        .map(|profile| {
            json!({
                "name": profile.name,
                "path": profile.path.display().to_string(),
            })
        })
        .collect::<Vec<_>>();
    let count = profiles.len();

    let data = json!({
        "profileDir": profile_dir.display().to_string(),
        "profiles": profiles,
    });

    Ok(CommandOutput::new(data)
        .with_count(count)
        .with_total(count)
        .with_has_more(false))
}

#[allow(dead_code)]
pub fn validate(shared: &SharedArgs) -> CommandResult {
    let paths = shared.resolved_paths();
    let path = shared.config_path(&paths)?;
    let config = load_config(Some(&path))?;
    config.validate()?;
    let path_display = path.display().to_string();

    let data = json!({
        "path": path_display,
        "valid": true,
        "errors": [],
    });
    Ok(CommandOutput::new(data))
}
