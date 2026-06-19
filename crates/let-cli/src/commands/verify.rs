#![forbid(unsafe_code)]

use let_sdk::intelligence::{
    EvidenceSection, InspectDepth, InspectParams, RefreshPolicy, VerifyParams,
};

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct VerifyCommandParams {
    pub id: String,
    pub claim: String,
    pub refresh: RefreshPolicy,
}

pub fn run(shared: &SharedArgs, params: VerifyCommandParams) -> CommandResult {
    let claim = VerifyClaim::parse(&params.claim)?;
    let paths = shared.resolved_paths();
    let config_path = shared.config_path(&paths)?;
    let sections = sections_for_claim(claim);
    let response = let_sdk::intelligence::verify(VerifyParams {
        id: params.id.clone(),
        claim: claim.as_str().to_owned(),
        refresh: params.refresh,
        inspect: InspectParams {
            id_or_url: params.id,
            depth: InspectDepth::Standard,
            refresh: params.refresh,
            sections,
            database_path: paths.derived.database,
            config_path,
            env_path: paths.derived.env_file,
            cache_dir: paths.resolved.cache,
            sources_dir: paths.resolved.sources,
        },
    })?;

    Ok(CommandOutput::new(to_camel_json(&response)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifyClaim {
    All,
    Media,
    Epc,
    Address,
    Description,
    Broadband,
}

impl VerifyClaim {
    fn parse(value: &str) -> Result<Self, CommandError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "media" => Ok(Self::Media),
            "epc" => Ok(Self::Epc),
            "address" => Ok(Self::Address),
            "description" => Ok(Self::Description),
            "broadband" => Ok(Self::Broadband),
            _ => Err(CommandError::runtime(
                "VALIDATION_ERROR",
                format!("unsupported verify claim `{}`", value.trim()),
                "use one of all, address, broadband, epc, media, or description",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Media => "media",
            Self::Epc => "epc",
            Self::Address => "address",
            Self::Description => "description",
            Self::Broadband => "broadband",
        }
    }
}

fn sections_for_claim(claim: VerifyClaim) -> Vec<EvidenceSection> {
    match claim {
        VerifyClaim::Media => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Media,
            EvidenceSection::Verifications,
        ],
        VerifyClaim::Epc => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Description,
            EvidenceSection::Claims,
            EvidenceSection::Epc,
            EvidenceSection::Verifications,
        ],
        VerifyClaim::Address => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Address,
            EvidenceSection::Verifications,
        ],
        VerifyClaim::Description => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Description,
            EvidenceSection::Claims,
            EvidenceSection::Verifications,
        ],
        VerifyClaim::Broadband => vec![
            EvidenceSection::Rightmove,
            EvidenceSection::Description,
            EvidenceSection::Claims,
            EvidenceSection::Broadband,
            EvidenceSection::Verifications,
        ],
        VerifyClaim::All => vec![
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

    use super::{VerifyClaim, sections_for_claim};

    #[test]
    fn broadband_verify_refresh_excludes_media() {
        let sections = sections_for_claim(VerifyClaim::Broadband);

        assert!(sections.contains(&EvidenceSection::Broadband));
        assert!(sections.contains(&EvidenceSection::Claims));
        assert!(!sections.contains(&EvidenceSection::Media));
    }

    #[test]
    fn media_verify_refresh_includes_media() {
        let sections = sections_for_claim(VerifyClaim::Media);

        assert_eq!(
            sections,
            vec![
                EvidenceSection::Rightmove,
                EvidenceSection::Media,
                EvidenceSection::Verifications,
            ]
        );
    }

    #[test]
    fn verify_claim_rejects_unknown_values() {
        let error = VerifyClaim::parse("broadbnd").expect_err("claim should be rejected");

        assert_eq!(error.code, "VALIDATION_ERROR");
    }
}
