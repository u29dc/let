#![forbid(unsafe_code)]

use let_sdk::intelligence::AreaPostcodeParams;

use crate::commands::{CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct AreaPostcodeCommandParams {
    pub postcode: String,
    pub radius_m: f64,
    pub limit: usize,
}

pub fn postcode(shared: &SharedArgs, params: AreaPostcodeCommandParams) -> CommandResult {
    let paths = shared.resolved_paths();
    let response = let_sdk::intelligence::area_postcode(AreaPostcodeParams {
        postcode: params.postcode,
        radius_m: params.radius_m,
        limit: params.limit,
        sources_dir: paths.resolved.sources,
    })?;

    Ok(CommandOutput::new(to_camel_json(&response))
        .with_count(response.facts.len())
        .with_total(response.facts.len())
        .with_has_more(false))
}
