#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

use let_sdk::load_listings_file;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};

pub fn export_json(shared: &SharedArgs, output: Option<PathBuf>) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;
    let output_path = output.unwrap_or(paths.derived.json_export);

    let data = load_listings_file(&db_path)?;
    let payload = to_camel_json(&data);
    let pretty = serde_json::to_string_pretty(&payload).expect("json serialization should succeed");

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CommandError::runtime(
                "IO_ERROR",
                format!("failed to create export directory: {error}"),
                "check filesystem permissions for output path",
            )
        })?;
    }
    fs::write(&output_path, pretty).map_err(|error| {
        CommandError::runtime(
            "IO_ERROR",
            format!("failed to write export file: {error}"),
            "check filesystem permissions and free space",
        )
    })?;

    Ok(CommandOutput::new(json!({
        "path": output_path.display().to_string(),
        "count": data.listings.len(),
    }))
    .with_count(data.listings.len())
    .with_text(format!("json export saved: {}", output_path.display())))
}
