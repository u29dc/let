#![forbid(unsafe_code)]

use let_sdk::intelligence::{EvidenceParams, EvidenceSection};
use let_sdk::paths::resolve_paths;

use crate::commands::{CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct EvidenceCommandParams {
    pub id: String,
    pub sections: Vec<EvidenceSection>,
}

pub fn run(shared: &SharedArgs, params: EvidenceCommandParams) -> CommandResult {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let response = let_sdk::intelligence::evidence(EvidenceParams {
        id: params.id,
        sections: params.sections,
        database_path: paths.derived.database,
    })?;

    Ok(CommandOutput::new(to_camel_json(&response)))
}
