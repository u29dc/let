#![forbid(unsafe_code)]

use let_sdk::intelligence::{
    EvidenceListParams, EvidenceParams, EvidenceSection, ListingListFilters,
};
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

pub fn list(shared: &SharedArgs, filters: ListingListFilters) -> CommandResult {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let response = let_sdk::intelligence::evidence_list(EvidenceListParams {
        filters,
        database_path: paths.derived.database,
    })?;
    let count = response.listings.len();

    Ok(CommandOutput::new(to_camel_json(&response))
        .with_count(count)
        .with_total(count))
}
