#![forbid(unsafe_code)]

use let_sdk::intelligence::{
    EvidenceSection, InspectDepth, InspectParams, RefreshPolicy, VerifyParams,
};
use let_sdk::paths::resolve_paths;

use crate::commands::{CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct VerifyCommandParams {
    pub id: String,
    pub claim: String,
    pub refresh: RefreshPolicy,
}

pub fn run(shared: &SharedArgs, params: VerifyCommandParams) -> CommandResult {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let sections = sections_for_claim(&params.claim);
    let response = let_sdk::intelligence::verify(VerifyParams {
        id: params.id.clone(),
        claim: params.claim,
        refresh: params.refresh,
        inspect: InspectParams {
            id_or_url: params.id,
            depth: InspectDepth::Standard,
            refresh: params.refresh,
            sections,
            database_path: paths.derived.database,
            config_path: paths.derived.config_file,
            env_path: paths.derived.env_file,
            cache_dir: paths.resolved.cache,
            sources_dir: paths.resolved.sources,
        },
    })?;

    Ok(CommandOutput::new(to_camel_json(&response)))
}

fn sections_for_claim(claim: &str) -> Vec<EvidenceSection> {
    match claim.trim().to_ascii_lowercase().as_str() {
        "media" => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Media,
            EvidenceSection::Verifications,
        ],
        "epc" => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Description,
            EvidenceSection::Claims,
            EvidenceSection::Epc,
            EvidenceSection::Verifications,
        ],
        "address" => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Address,
            EvidenceSection::Verifications,
        ],
        "description" => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Description,
            EvidenceSection::Claims,
            EvidenceSection::Verifications,
        ],
        "broadband" => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Description,
            EvidenceSection::Claims,
            EvidenceSection::Broadband,
            EvidenceSection::Verifications,
        ],
        _ => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Description,
            EvidenceSection::Address,
            EvidenceSection::Facts,
            EvidenceSection::Claims,
            EvidenceSection::Broadband,
            EvidenceSection::Verifications,
        ],
    }
}

#[cfg(test)]
mod tests {
    use let_sdk::intelligence::EvidenceSection;

    use super::sections_for_claim;

    #[test]
    fn broadband_verify_refresh_excludes_media() {
        let sections = sections_for_claim("broadband");

        assert!(sections.contains(&EvidenceSection::Broadband));
        assert!(sections.contains(&EvidenceSection::Claims));
        assert!(!sections.contains(&EvidenceSection::Media));
    }

    #[test]
    fn media_verify_refresh_includes_media() {
        let sections = sections_for_claim("media");

        assert_eq!(
            sections,
            vec![
                EvidenceSection::Rightmove,
                EvidenceSection::Media,
                EvidenceSection::Verifications,
            ]
        );
    }
}
