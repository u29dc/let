#![forbid(unsafe_code)]

use std::time::Instant;

use let_sdk::intelligence::{EvidenceSection, InspectDepth, InspectParams, RefreshPolicy};
use let_sdk::paths::PathBundle;

use crate::commands::batch::{BatchItem, BatchResponse, bundle_warnings, resolve_inputs};
use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct InspectCommandParams {
    pub id_or_urls: Vec<String>,
    pub depth: InspectDepth,
    pub refresh: RefreshPolicy,
    pub sections: Vec<EvidenceSection>,
}

pub fn run(shared: &SharedArgs, params: InspectCommandParams) -> CommandResult {
    let inputs = resolve_inputs(&params.id_or_urls, "inspect", "Rightmove ids or URLs")?;
    let paths = shared.resolved_paths();
    let config_path = shared.config_path(&paths)?;

    if inputs.len() == 1 {
        let bundle = inspect_one(&paths, &config_path, &params, inputs[0].clone())?;
        return Ok(CommandOutput::new(to_camel_json(&bundle)));
    }

    let mut items = Vec::with_capacity(inputs.len());
    for input in inputs {
        let started = Instant::now();
        match inspect_one(&paths, &config_path, &params, input.clone()) {
            Ok(bundle) => {
                let warnings = bundle_warnings(&bundle);
                let id = bundle.rightmove_id.clone();
                items.push(BatchItem::success(
                    input,
                    id,
                    to_camel_json(&bundle),
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

fn inspect_one(
    paths: &PathBundle,
    config_path: &std::path::Path,
    params: &InspectCommandParams,
    id_or_url: String,
) -> Result<let_sdk::intelligence::EvidenceBundle, CommandError> {
    let bundle = let_sdk::intelligence::inspect(InspectParams {
        id_or_url,
        depth: params.depth,
        refresh: params.refresh,
        sections: params.sections.clone(),
        database_path: paths.derived.database.clone(),
        config_path: config_path.to_path_buf(),
        env_path: paths.derived.env_file.clone(),
        cache_dir: paths.resolved.cache.clone(),
        sources_dir: paths.resolved.sources.clone(),
    })?;

    Ok(bundle)
}
