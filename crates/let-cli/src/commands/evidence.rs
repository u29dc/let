#![forbid(unsafe_code)]

use std::time::Instant;

use let_sdk::intelligence::service::EvidenceResponse;
use let_sdk::intelligence::{
    EvidenceListParams, EvidenceParams, EvidenceSection, ListingListFilters,
};
use let_sdk::paths::PathBundle;

use crate::commands::batch::{BatchItem, BatchResponse, bundle_warnings, resolve_inputs};
use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct EvidenceCommandParams {
    pub ids: Vec<String>,
    pub sections: Vec<EvidenceSection>,
}

pub fn run(shared: &SharedArgs, params: EvidenceCommandParams) -> CommandResult {
    let inputs = resolve_inputs(&params.ids, "evidence", "Rightmove ids or entity ids")?;
    reject_list_token(&inputs)?;
    let paths = shared.resolved_paths();

    if inputs.len() == 1 {
        let response = evidence_one(&paths, &params, inputs[0].clone())?;
        return Ok(CommandOutput::new(to_camel_json(&response)));
    }

    let mut items = Vec::with_capacity(inputs.len());
    for input in inputs {
        let started = Instant::now();
        match evidence_one(&paths, &params, input.clone()) {
            Ok(response) => {
                let warnings = bundle_warnings(&response.bundle);
                let id = response.bundle.rightmove_id.clone();
                items.push(BatchItem::success(
                    input,
                    id,
                    to_camel_json(&response.bundle),
                    started.elapsed(),
                    warnings,
                ));
            }
            Err(error) => {
                items.push(BatchItem::failure(input, error, started.elapsed()));
            }
        }
    }

    let response = BatchResponse::new(items);
    Ok(CommandOutput::new(to_camel_json(&response))
        .with_count(response.count)
        .with_total(response.count))
}

pub fn list(shared: &SharedArgs, filters: ListingListFilters) -> CommandResult {
    let paths = shared.resolved_paths();
    let response = let_sdk::intelligence::evidence_list(EvidenceListParams {
        filters,
        database_path: paths.derived.database,
    })?;
    let count = response.listings.len();

    Ok(CommandOutput::new(to_camel_json(&response))
        .with_count(count)
        .with_total(count))
}

fn evidence_one(
    paths: &PathBundle,
    params: &EvidenceCommandParams,
    id: String,
) -> Result<EvidenceResponse, CommandError> {
    let response = let_sdk::intelligence::evidence(EvidenceParams {
        id,
        sections: params.sections.clone(),
        database_path: paths.derived.database.clone(),
    })?;

    Ok(response)
}

fn reject_list_token(inputs: &[String]) -> Result<(), CommandError> {
    if inputs.iter().any(|value| value == "list") {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "`list` is only valid as the sole `let evidence list` argument",
            "run `let evidence list` or pass only listing ids",
        ));
    }
    Ok(())
}
