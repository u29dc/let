#![forbid(unsafe_code)]

use let_sdk::intelligence::{AssessGetParams, AssessSaveParams};
use let_sdk::paths::resolve_paths;
use serde_json::Value;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};

pub fn save(shared: &SharedArgs, id: &str, assessment_raw: &str) -> CommandResult {
    let assessment = serde_json::from_str::<Value>(assessment_raw).map_err(|error| {
        CommandError::runtime(
            "VALIDATION_ERROR",
            format!("assessment must be valid JSON: {error}"),
            "pass an object such as '{\"recommendation\":\"view\"}'",
        )
    })?;
    if !assessment.is_object() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "assessment must be a JSON object",
            "pass an object such as '{\"recommendation\":\"view\"}'",
        ));
    }

    let paths = resolve_paths(Some(shared.overrides.clone()));
    let record = let_sdk::intelligence::assess_save(AssessSaveParams {
        id: id.to_owned(),
        assessment,
        database_path: paths.derived.database,
    })?;
    Ok(CommandOutput::new(to_camel_json(&record)))
}

pub fn get(shared: &SharedArgs, id: &str) -> CommandResult {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let record = let_sdk::intelligence::assess_get(AssessGetParams {
        id: id.to_owned(),
        database_path: paths.derived.database,
    })?;
    Ok(CommandOutput::new(to_camel_json(&record)))
}
