#![forbid(unsafe_code)]

use serde::Serialize;

use crate::commands::{CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct ScoreComputeCommandParams {
    pub id: String,
    pub scorecard_id: String,
}

#[derive(Debug, Clone)]
pub struct ScoreGetCommandParams {
    pub id: String,
    pub scorecard_id: String,
}

#[derive(Debug, Clone)]
pub struct ScoreListCommandParams {
    pub scorecard_id: Option<String>,
}

pub fn compute(shared: &SharedArgs, params: ScoreComputeCommandParams) -> CommandResult {
    let paths = shared.resolved_paths();
    let config_path = shared.config_path(&paths)?;
    let response = let_sdk::score_compute(let_sdk::ScoreComputeParams {
        id: params.id,
        scorecard_id: params.scorecard_id,
        database_path: paths.derived.database,
        config_path,
    })?;
    Ok(CommandOutput::new(to_camel_json(&response)))
}

pub fn get(shared: &SharedArgs, params: ScoreGetCommandParams) -> CommandResult {
    let paths = shared.resolved_paths();
    let score = let_sdk::score_get(let_sdk::ScoreGetParams {
        id: params.id,
        scorecard_id: params.scorecard_id,
        database_path: paths.derived.database,
    })?;
    Ok(CommandOutput::new(to_camel_json(&score)))
}

pub fn list(shared: &SharedArgs, params: ScoreListCommandParams) -> CommandResult {
    let paths = shared.resolved_paths();
    let response = let_sdk::score_list(let_sdk::ScoreListParams {
        scorecard_id: params.scorecard_id,
        database_path: paths.derived.database,
    })?;
    let count = response.scores.len();
    Ok(CommandOutput::new(to_camel_json(&response))
        .with_count(count)
        .with_total(count))
}

pub fn scorecards_list(shared: &SharedArgs) -> CommandResult {
    let paths = shared.resolved_paths();
    let config_path = shared.config_path(&paths)?;
    let response = let_sdk::scorecards(let_sdk::ScorecardsParams { config_path })?;
    let count = response.scorecards.len();
    Ok(CommandOutput::new(to_camel_json(&response))
        .with_count(count)
        .with_total(count))
}

pub fn scorecards_validate(shared: &SharedArgs) -> CommandResult {
    let paths = shared.resolved_paths();
    let config_path = shared.config_path(&paths)?;
    let response = let_sdk::scorecards(let_sdk::ScorecardsParams { config_path })?;
    let output = ScorecardsValidateResponse {
        status: "ok",
        scorecards: response.scorecards,
        default_scorecard: response.default_scorecard,
    };
    let count = output.scorecards.len();
    Ok(CommandOutput::new(to_camel_json(&output))
        .with_count(count)
        .with_total(count))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScorecardsValidateResponse {
    status: &'static str,
    scorecards: Vec<let_sdk::ScorecardConfig>,
    default_scorecard: let_sdk::score::ScorecardRef,
}
