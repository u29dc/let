#![forbid(unsafe_code)]

use let_sdk::intelligence::{EvidenceSection, InspectDepth, InspectParams, RefreshPolicy};

use crate::commands::{CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct InspectCommandParams {
    pub id_or_url: String,
    pub depth: InspectDepth,
    pub refresh: RefreshPolicy,
    pub sections: Vec<EvidenceSection>,
}

pub fn run(shared: &SharedArgs, params: InspectCommandParams) -> CommandResult {
    let paths = shared.resolved_paths();
    let config_path = shared.config_path(&paths)?;
    let bundle = let_sdk::intelligence::inspect(InspectParams {
        id_or_url: params.id_or_url,
        depth: params.depth,
        refresh: params.refresh,
        sections: params.sections,
        database_path: paths.derived.database,
        config_path,
        env_path: paths.derived.env_file,
        cache_dir: paths.resolved.cache,
        sources_dir: paths.resolved.sources,
    })?;

    Ok(CommandOutput::new(to_camel_json(&bundle)))
}
