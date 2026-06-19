#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::config::{FetchConfig, load_config};
use crate::errors::{ErrorCode, LetError, Result};
use crate::intelligence::repository::{IntelligenceDb, extract_rightmove_id, normalize_entity_id};
use crate::intelligence::types::{
    AddressCandidateEvidence, AddressEvidence, AssessmentRecord, BroadbandEvidence, ClaimEvidence,
    ConfidenceLevel, CorrectionKind, CorrectionRecord, DescriptionEvidence, EpcEvidence,
    EvidenceBundle, EvidenceSection, FactEvidence, FactProvider, InspectDepth, MediaEvidence,
    MediaItemEvidence, RefreshPolicy, RightmoveEvidence, SectionState, SectionStatus, SourceRef,
    SourceSnapshotEvidence, VerificationEvidence, VerificationStatus,
};
use crate::pipeline::enrich::{BroadbandProfile, EnrichmentMode, SourceEnricher};
use crate::pipeline::epc::{EpcCredentials, EpcLookup, lookup_domestic_epc};
use crate::pipeline::fetch::media::{MediaNormalizationConfig, populate_listing_media};
use crate::pipeline::fetch::rightmove::{
    RightmovePropertyExtract, extract_property_evidence, fetch_page_capture, listing_from_capture,
};
use crate::pipeline::geocode::mapbox_forward_geocode;
use crate::schema::listing::{Listing, PinType, RemoteLocalAsset};
use crate::utils::time::now_iso;

#[derive(Debug, Clone)]
pub struct InspectParams {
    pub id_or_url: String,
    pub depth: InspectDepth,
    pub refresh: RefreshPolicy,
    pub sections: Vec<EvidenceSection>,
    pub database_path: PathBuf,
    pub config_path: PathBuf,
    pub env_path: PathBuf,
    pub cache_dir: PathBuf,
    pub sources_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct EvidenceParams {
    pub id: String,
    pub sections: Vec<EvidenceSection>,
    pub database_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct VerifyParams {
    pub id: String,
    pub claim: String,
    pub refresh: RefreshPolicy,
    pub inspect: InspectParams,
}

#[derive(Debug, Clone)]
pub struct AssessSaveParams {
    pub id: String,
    pub assessment: Value,
    pub database_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AssessGetParams {
    pub id: String,
    pub database_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CorrectionSaveParams {
    pub id: String,
    pub kind: CorrectionKind,
    pub payload: Value,
    pub note: Option<String>,
    pub database_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CorrectionClearParams {
    pub id: String,
    pub kind: CorrectionKind,
    pub correction_id: String,
    pub database_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceResponse {
    pub bundle: EvidenceBundle,
    pub requested_sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub id: String,
    pub claim: String,
    pub verifications: Vec<VerificationEvidence>,
    pub sections: BTreeMap<String, SectionState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionResponse {
    pub correction: CorrectionRecord,
    pub affected_sections: Vec<String>,
    pub warnings: Vec<String>,
    pub next_commands: Vec<String>,
}

pub fn inspect(params: InspectParams) -> Result<EvidenceBundle> {
    let rightmove_id = extract_rightmove_id(&params.id_or_url).ok_or_else(|| {
        LetError::new(
            ErrorCode::InvalidInput,
            format!(
                "`{}` is not a Rightmove listing id or URL",
                params.id_or_url
            ),
            "pass a Rightmove portal id or https://www.rightmove.co.uk/properties/<id> URL",
        )
    })?;
    let entity_id = normalize_entity_id(&rightmove_id);
    let mut db = IntelligenceDb::open(&params.database_path)?;

    if params.refresh == RefreshPolicy::None
        && let Some(bundle) = db.load_bundle(&entity_id)?
    {
        return Ok(bundle);
    }

    let sections = requested_sections(params.depth, &params.sections);
    let fetch_config = load_fetch_config(&params.config_path)?;
    let runtime = tokio::runtime::Runtime::new().map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to create async runtime: {error}"),
            "retry the command",
        )
    })?;
    let client = reqwest::Client::builder()
        .user_agent("let-rust/0.0.1")
        .timeout(Duration::from_millis(fetch_config.media_timeout_ms))
        .build()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!("failed to create HTTP client: {error}"),
                "check TLS/network runtime dependencies",
            )
        })?;

    let capture = fetch_page_capture(&runtime, &client, &rightmove_id, fetch_config.max_retries)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Network,
                format!("failed to fetch Rightmove listing {rightmove_id}: {error}"),
                "retry later or verify that the listing is still available",
            )
        })?;
    let extracted = extract_property_evidence(&capture).map_err(|error| {
        LetError::new(
            ErrorCode::Parse,
            format!("failed to extract Rightmove evidence for {rightmove_id}: {error}"),
            "inspect the stored source snapshot or update the Rightmove extractor",
        )
    })?;
    let mut source_listing = listing_from_capture(&capture, None, true).ok();

    let source_ref = SourceRef {
        source: "rightmove".to_owned(),
        snapshot_id: Some(snapshot_id(&entity_id, "rightmove", &rightmove_id)),
        observation_id: None,
        url: Some(extracted.url.clone()),
        captured_at: Some(extracted.fetched_at.clone()),
    };
    let corrections = db.load_active_corrections(&entity_id)?;
    apply_coordinate_correction(source_listing.as_mut(), &corrections);
    let corrected_postcode = correction_string(&corrections, CorrectionKind::Address, "postcode");
    let corrected_address = correction_string(&corrections, CorrectionKind::Address, "address");
    let broadband = resolve_broadband(
        &params.sources_dir,
        &extracted,
        corrected_postcode.as_deref(),
    );
    let (epc, epc_warnings) = resolve_epc(EpcResolutionContext {
        env_path: &params.env_path,
        runtime: &runtime,
        client: &client,
        extracted: &extracted,
        corrections: &corrections,
        address_override: corrected_address.as_deref(),
        postcode_override: corrected_postcode.as_deref(),
        include_external: sections.contains(&EvidenceSection::Epc),
    });
    let address = resolve_address(
        &params.sources_dir,
        &params.env_path,
        &runtime,
        &client,
        &extracted,
        &corrections,
        sections.contains(&EvidenceSection::Address),
    );
    let media = resolve_media(
        &runtime,
        &client,
        &fetch_config,
        &params.env_path,
        &params.cache_dir,
        params.depth,
        &sections,
        &extracted,
        source_listing.as_mut(),
    );
    let claims = extract_claims(&entity_id, &extracted, &source_ref);
    let verifications = verify_claims(&entity_id, &claims, broadband.as_ref(), &source_ref);
    let mut facts = facts_from_extract(&extracted, broadband.as_ref(), epc.as_ref(), &source_ref);
    facts.extend(correction_facts(&corrections));
    if let Some(listing) = source_listing.as_mut() {
        facts.extend(source_facts(&params.sources_dir, listing));
    }
    let assessment = db.load_assessment(&entity_id)?;
    let mut bundle = EvidenceBundle {
        entity_id,
        rightmove_id: rightmove_id.clone(),
        url: extracted.url.clone(),
        generated_at: now_iso(),
        depth: params.depth,
        refresh: params.refresh,
        sections: BTreeMap::new(),
        source_snapshots: vec![SourceSnapshotEvidence {
            id: source_ref.snapshot_id.clone().expect("snapshot id present"),
            source: "rightmove".to_owned(),
            source_key: rightmove_id,
            url: extracted.url.clone(),
            captured_at: extracted.fetched_at.clone(),
            status: extracted.page_status.clone(),
            content_hash: extracted.content_hash.clone(),
            raw_json: Some(capture.page_model),
        }],
        rightmove: rightmove_evidence(&extracted, &media),
        address,
        facts,
        broadband,
        epc,
        claims,
        verifications,
        media,
        assessment,
        corrections,
        next_actions: Vec::new(),
    };
    bundle.sections = section_states(&bundle, &sections, &source_ref, epc_warnings);
    bundle.next_actions = next_actions(&bundle);
    db.save_bundle(&bundle)?;
    Ok(bundle)
}

pub fn evidence(params: EvidenceParams) -> Result<EvidenceResponse> {
    let db = IntelligenceDb::open(&params.database_path)?;
    let bundle = db.load_bundle(&params.id)?.ok_or_else(|| {
        LetError::new(
            ErrorCode::NotFound,
            format!("no evidence bundle found for `{}`", params.id),
            "run `let inspect <id>` first",
        )
    })?;
    let requested_sections = requested_sections(bundle.depth, &params.sections)
        .into_iter()
        .map(EvidenceSection::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    Ok(EvidenceResponse {
        bundle,
        requested_sections,
    })
}

pub fn verify(params: VerifyParams) -> Result<VerifyResponse> {
    let bundle = if params.refresh == RefreshPolicy::None {
        let db = IntelligenceDb::open(&params.inspect.database_path)?;
        db.load_bundle(&params.id)?.ok_or_else(|| {
            LetError::new(
                ErrorCode::NotFound,
                format!("no evidence bundle found for `{}`", params.id),
                "run `let inspect <id>` first or use `--refresh stale`",
            )
        })?
    } else {
        inspect(params.inspect)?
    };

    let claim = params.claim.trim().to_ascii_lowercase();
    let verifications = bundle
        .verifications
        .iter()
        .filter(|verification| claim == "all" || verification.claim_type == claim)
        .cloned()
        .collect::<Vec<_>>();
    Ok(VerifyResponse {
        id: params.id,
        claim: params.claim,
        verifications,
        sections: bundle.sections,
    })
}

pub fn save_correction(params: CorrectionSaveParams) -> Result<CorrectionResponse> {
    let db = IntelligenceDb::open(&params.database_path)?;
    let affected_sections = affected_sections_for_correction(params.kind);
    let correction = db.save_correction(
        &params.id,
        params.kind,
        params.payload,
        params.note,
        affected_sections.clone(),
    )?;
    let id = correction
        .entity_id
        .trim_start_matches("rightmove:")
        .to_owned();

    Ok(CorrectionResponse {
        correction,
        affected_sections: affected_sections.clone(),
        warnings: Vec::new(),
        next_commands: vec![
            format!(
                "let inspect {id} --refresh all --section {}",
                affected_sections.join(",")
            ),
            format!(
                "let evidence {id} --section {}",
                affected_sections.join(",")
            ),
        ],
    })
}

pub fn clear_correction(params: CorrectionClearParams) -> Result<CorrectionResponse> {
    let db = IntelligenceDb::open(&params.database_path)?;
    let affected_sections = affected_sections_for_correction(params.kind);
    let Some(correction) = db.clear_correction(&params.id, params.kind, &params.correction_id)?
    else {
        return Err(LetError::new(
            ErrorCode::NotFound,
            format!(
                "no active {} correction `{}` was found for `{}`",
                params.kind.as_str(),
                params.correction_id,
                params.id
            ),
            "check `let evidence <id>` for active corrections or pass the exact correction id",
        ));
    };
    let id = correction
        .entity_id
        .trim_start_matches("rightmove:")
        .to_owned();

    Ok(CorrectionResponse {
        correction,
        affected_sections: affected_sections.clone(),
        warnings: Vec::new(),
        next_commands: vec![
            format!(
                "let inspect {id} --refresh all --section {}",
                affected_sections.join(",")
            ),
            format!(
                "let evidence {id} --section {}",
                affected_sections.join(",")
            ),
        ],
    })
}

pub fn assess_save(params: AssessSaveParams) -> Result<AssessmentRecord> {
    let db = IntelligenceDb::open(params.database_path)?;
    db.save_assessment(&params.id, params.assessment)
}

pub fn assess_get(params: AssessGetParams) -> Result<AssessmentRecord> {
    let db = IntelligenceDb::open(params.database_path)?;
    db.load_assessment(&params.id)?.ok_or_else(|| {
        LetError::new(
            ErrorCode::NotFound,
            format!("no assessment found for `{}`", params.id),
            "run `let assess save <id> <assessment-json>` first",
        )
    })
}

fn requested_sections(
    depth: InspectDepth,
    explicit: &[EvidenceSection],
) -> BTreeSet<EvidenceSection> {
    let sections = if explicit.is_empty() {
        EvidenceSection::default_sections(depth)
    } else {
        explicit.to_vec()
    };
    sections.into_iter().collect()
}

fn load_fetch_config(config_path: &Path) -> Result<FetchConfig> {
    match load_config(Some(config_path)) {
        Ok(config) => Ok(config.fetch),
        Err(error) if error.code == ErrorCode::NoConfig => Ok(FetchConfig::default()),
        Err(error) => Err(error),
    }
}

fn affected_sections_for_correction(kind: CorrectionKind) -> Vec<String> {
    let sections: &[&str] = match kind {
        CorrectionKind::Address => &[
            "address",
            "facts",
            "broadband",
            "epc",
            "media",
            "verifications",
        ],
        CorrectionKind::Epc => &["epc", "facts", "verifications"],
        CorrectionKind::Media => &["media"],
    };
    sections.iter().copied().map(ToOwned::to_owned).collect()
}

fn correction_string(
    corrections: &[CorrectionRecord],
    kind: CorrectionKind,
    key: &str,
) -> Option<String> {
    corrections
        .iter()
        .rev()
        .filter(|correction| correction.active && correction.kind == kind)
        .find_map(|correction| {
            correction
                .payload
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .map(ToOwned::to_owned)
}

fn correction_f64(
    corrections: &[CorrectionRecord],
    kind: CorrectionKind,
    key: &str,
) -> Option<f64> {
    corrections
        .iter()
        .rev()
        .filter(|correction| correction.active && correction.kind == kind)
        .filter_map(|correction| correction.payload.get(key))
        .find_map(Value::as_f64)
}

fn address_candidates_from_corrections(
    corrections: &[CorrectionRecord],
) -> Vec<AddressCandidateEvidence> {
    corrections
        .iter()
        .filter(|correction| correction.active && correction.kind == CorrectionKind::Address)
        .filter_map(|correction| {
            let address = correction
                .payload
                .get("address")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let postcode = correction
                .payload
                .get("postcode")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            let latitude = correction.payload.get("lat").and_then(Value::as_f64);
            let longitude = correction.payload.get("lng").and_then(Value::as_f64);
            let label = address
                .map(ToOwned::to_owned)
                .or_else(|| postcode.clone())
                .or_else(|| {
                    latitude
                        .zip(longitude)
                        .map(|(lat, lng)| format!("{lat:.6},{lng:.6}"))
                })?;
            let confidence = if address.is_some() && postcode.is_some() && latitude.is_some() {
                ConfidenceLevel::Exact
            } else {
                ConfidenceLevel::Probable
            };
            Some(AddressCandidateEvidence {
                source: "manualCorrection".to_owned(),
                label,
                postcode,
                latitude,
                longitude,
                confidence,
                raw: Some(json!({
                    "correctionId": correction.id,
                    "note": correction.note,
                    "createdAt": correction.created_at,
                })),
            })
        })
        .collect()
}

fn epc_from_correction(corrections: &[CorrectionRecord]) -> Option<EpcEvidence> {
    let correction = corrections
        .iter()
        .rev()
        .find(|correction| correction.active && correction.kind == CorrectionKind::Epc)?;
    let lmk_key = correction
        .payload
        .get("lmkKey")
        .and_then(Value::as_str)
        .or_else(|| {
            correction
                .payload
                .get("certificateUrl")
                .and_then(Value::as_str)
        })
        .or_else(|| correction.payload.get("uprn").and_then(Value::as_str))?
        .trim()
        .to_owned();
    let rating = correction
        .payload
        .get("rating")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let floor_area_sqm = correction
        .payload
        .get("floorAreaSqm")
        .and_then(Value::as_f64);
    let uprn = correction
        .payload
        .get("uprn")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    Some(EpcEvidence {
        lmk_key,
        rating,
        floor_area_sqm,
        lodgement_date: None,
        address_match: true,
        matched_address: correction
            .note
            .clone()
            .unwrap_or_else(|| "manual EPC correction".to_owned()),
        uprn,
        uprn_source: Some("manualCorrection".to_owned()),
    })
}

fn apply_coordinate_correction(
    source_listing: Option<&mut Listing>,
    corrections: &[CorrectionRecord],
) {
    let Some(listing) = source_listing else {
        return;
    };
    let coordinates = correction_f64(corrections, CorrectionKind::Media, "mapLat")
        .zip(correction_f64(corrections, CorrectionKind::Media, "mapLng"))
        .or_else(|| {
            correction_f64(corrections, CorrectionKind::Address, "lat").zip(correction_f64(
                corrections,
                CorrectionKind::Address,
                "lng",
            ))
        });
    if let Some((lat, lng)) = coordinates {
        listing.location.lat = lat;
        listing.location.lng = lng;
        listing.location.pin_type = Some(PinType::AccuratePoint);
    }
}

fn correction_facts(corrections: &[CorrectionRecord]) -> Vec<FactEvidence> {
    corrections
        .iter()
        .filter(|correction| correction.active)
        .map(|correction| FactEvidence {
            provider: FactProvider::ManualCorrection,
            category: "correction".to_owned(),
            name: correction.kind.as_str().to_owned(),
            value: json!({
                "correctionId": correction.id,
                "payload": correction.payload,
                "note": correction.note,
                "affectedSections": correction.affected_sections,
            }),
            confidence: ConfidenceLevel::Probable,
            sources: vec![SourceRef {
                source: "manualCorrection".to_owned(),
                snapshot_id: None,
                observation_id: Some(correction.id.clone()),
                url: None,
                captured_at: Some(correction.created_at.clone()),
            }],
        })
        .collect()
}

fn rightmove_evidence(
    extracted: &RightmovePropertyExtract,
    media: &MediaEvidence,
) -> RightmoveEvidence {
    RightmoveEvidence {
        rightmove_id: extracted.rightmove_id.clone(),
        url: extracted.url.clone(),
        page_status: extracted.page_status.clone(),
        fetched_at: extracted.fetched_at.clone(),
        content_hash: extracted.content_hash.clone(),
        title: extracted.title.clone(),
        address: extracted.address.clone(),
        postcode: extracted.postcode.clone(),
        display_price: extracted.display_price.clone(),
        price_pcm: extracted.price_pcm,
        bedrooms: extracted.bedrooms,
        bathrooms: extracted.bathrooms,
        property_type: extracted.property_type.clone(),
        agent_name: extracted.agent_name.clone(),
        agent_phone: extracted.agent_phone.clone(),
        latitude: extracted.latitude,
        longitude: extracted.longitude,
        pin_type: extracted.pin_type.clone(),
        listed_date: extracted.listed_date.clone(),
        available_date: extracted.available_date.clone(),
        deposit: extracted.deposit,
        description: DescriptionEvidence {
            raw_html: extracted.description.raw_html.clone(),
            text: extracted.description.text.clone(),
            key_features: extracted.description.key_features.clone(),
            normalized_text: extracted.description.normalized_text.clone(),
        },
        media: media
            .photos
            .iter()
            .chain(media.floorplans.iter())
            .chain(media.epc_graphs.iter())
            .cloned()
            .collect(),
    }
}

fn resolve_broadband(
    sources_dir: &Path,
    extracted: &RightmovePropertyExtract,
    postcode_override: Option<&str>,
) -> Option<BroadbandEvidence> {
    let postcode = postcode_override
        .filter(|value| !value.trim().is_empty())
        .or(extracted.postcode.as_deref())?;
    let enricher = SourceEnricher::open(sources_dir).ok()?;
    let profile = enricher.lookup_broadband_profile(postcode).ok().flatten()?;
    Some(broadband_evidence(profile))
}

fn broadband_evidence(profile: BroadbandProfile) -> BroadbandEvidence {
    BroadbandEvidence {
        postcode: profile.postcode,
        postcode_display: profile.postcode_display,
        outward: profile.outward,
        area: profile.area,
        gigabit_availability: profile.gigabit_availability,
        pct_over_300mbps: profile.pct_over_300mbps,
        ufbb_availability: profile.ufbb_availability,
        sfbb_availability: profile.sfbb_availability,
    }
}

struct EpcResolutionContext<'a> {
    env_path: &'a Path,
    runtime: &'a tokio::runtime::Runtime,
    client: &'a reqwest::Client,
    extracted: &'a RightmovePropertyExtract,
    corrections: &'a [CorrectionRecord],
    address_override: Option<&'a str>,
    postcode_override: Option<&'a str>,
    include_external: bool,
}

fn resolve_epc(context: EpcResolutionContext<'_>) -> (Option<EpcEvidence>, Vec<String>) {
    if !context.include_external {
        return (None, Vec::new());
    }
    if let Some(epc) = epc_from_correction(context.corrections) {
        return (
            Some(epc),
            vec!["EPC evidence is pinned by an active manual correction".to_owned()],
        );
    }
    let Some(address) = context
        .address_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            context
                .extracted
                .address
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        })
    else {
        return (
            None,
            vec!["listing did not expose an address suitable for EPC lookup".to_owned()],
        );
    };
    let Some(postcode) = context
        .postcode_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            context
                .extracted
                .postcode
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        })
    else {
        return (
            None,
            vec!["listing did not expose a postcode suitable for EPC lookup".to_owned()],
        );
    };
    let Some(credentials) = resolve_epc_credentials(context.env_path) else {
        return (
            None,
            vec!["EPC credentials are not set; EPC lookup skipped".to_owned()],
        );
    };

    match context.runtime.block_on(lookup_domestic_epc(
        context.client,
        &credentials,
        address,
        postcode,
    )) {
        Ok(Some(lookup)) => (Some(epc_evidence(&lookup)), Vec::new()),
        Ok(None) => (
            None,
            vec!["EPC API returned no confident domestic certificate match".to_owned()],
        ),
        Err(error) => (None, vec![format!("EPC lookup failed: {}", error.message)]),
    }
}

fn epc_evidence(lookup: &EpcLookup) -> EpcEvidence {
    EpcEvidence {
        lmk_key: lookup.lmk_key.clone(),
        rating: lookup.epc_rating.as_ref().map(|band| format!("{band:?}")),
        floor_area_sqm: lookup.floor_area_sqm,
        lodgement_date: lookup.lodgement_date.clone(),
        address_match: lookup.address_match,
        matched_address: lookup.matched_address.clone(),
        uprn: lookup.uprn.clone(),
        uprn_source: lookup.uprn_source.clone(),
    }
}

fn resolve_epc_credentials(env_path: &Path) -> Option<EpcCredentials> {
    if let Some(token) = resolve_env_var("EPC_API_BEARER_TOKEN", env_path) {
        return Some(EpcCredentials::bearer_token(token));
    }
    let email = resolve_env_var("EPC_API_EMAIL", env_path)?;
    let api_key = resolve_env_var("EPC_API_KEY", env_path)?;
    Some(EpcCredentials::legacy_basic(email, api_key))
}

fn resolve_address(
    sources_dir: &Path,
    env_path: &Path,
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    extracted: &RightmovePropertyExtract,
    corrections: &[CorrectionRecord],
    include_external: bool,
) -> AddressEvidence {
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();

    candidates.extend(address_candidates_from_corrections(corrections));

    if let Some(label) = extracted.address.as_ref().or(extracted.postcode.as_ref()) {
        candidates.push(AddressCandidateEvidence {
            source: "rightmove".to_owned(),
            label: label.clone(),
            postcode: extracted.postcode.clone(),
            latitude: extracted.latitude,
            longitude: extracted.longitude,
            confidence: match extracted.pin_type.as_deref() {
                Some("ACCURATE_POINT") => ConfidenceLevel::Exact,
                Some("APPROXIMATE_POINT") => ConfidenceLevel::Heuristic,
                _ => ConfidenceLevel::Unknown,
            },
            raw: None,
        });
    }

    let lookup_postcode = correction_string(corrections, CorrectionKind::Address, "postcode")
        .or_else(|| extracted.postcode.clone());

    if let Some(postcode) = lookup_postcode.as_ref()
        && let Ok(enricher) = SourceEnricher::open(sources_dir)
    {
        match enricher.lookup_postcode_coordinates(postcode) {
            Ok(Some(coords)) => candidates.push(AddressCandidateEvidence {
                source: "postcodesDb".to_owned(),
                label: postcode.clone(),
                postcode: Some(postcode.clone()),
                latitude: Some(coords.lat),
                longitude: Some(coords.lng),
                confidence: ConfidenceLevel::Probable,
                raw: None,
            }),
            Ok(None) => {
                warnings.push("postcode was not found in the local postcode database".to_owned())
            }
            Err(error) => warnings.push(format!(
                "postcode database lookup failed: {}",
                error.message
            )),
        }
    }

    if include_external {
        if let Some(token) = resolve_env_var("MAPBOX_ACCESS_TOKEN", env_path) {
            let corrected_address =
                correction_string(corrections, CorrectionKind::Address, "address");
            let corrected_postcode =
                correction_string(corrections, CorrectionKind::Address, "postcode");
            let query = [
                corrected_address
                    .as_deref()
                    .or(extracted.address.as_deref()),
                corrected_postcode
                    .as_deref()
                    .or(extracted.postcode.as_deref()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            if !query.is_empty() {
                match runtime.block_on(mapbox_forward_geocode(client, &query, &token)) {
                    Ok(Some(coords)) => candidates.push(AddressCandidateEvidence {
                        source: "mapbox".to_owned(),
                        label: query,
                        postcode: extracted.postcode.clone(),
                        latitude: Some(coords.lat),
                        longitude: Some(coords.lng),
                        confidence: ConfidenceLevel::Probable,
                        raw: Some(json!({ "source": coords.source.as_str() })),
                    }),
                    Ok(None) => warnings.push("Mapbox returned no address candidate".to_owned()),
                    Err(error) => warnings.push(format!("Mapbox lookup failed: {}", error.message)),
                }
            }
        } else {
            warnings.push("MAPBOX_ACCESS_TOKEN is not set; external geocoding skipped".to_owned());
        }
    }

    let selected = candidates
        .iter()
        .find(|candidate| candidate.source == "manualCorrection")
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| candidate.confidence == ConfidenceLevel::Exact)
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| candidate.source == "postcodesDb")
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| candidate.source == "mapbox")
        })
        .or_else(|| candidates.first())
        .cloned();
    let confidence = selected
        .as_ref()
        .map(|candidate| candidate.confidence)
        .unwrap_or(ConfidenceLevel::Unknown);
    let status = if selected.is_some() {
        if warnings.is_empty() {
            SectionStatus::Ok
        } else {
            SectionStatus::Partial
        }
    } else {
        SectionStatus::Degraded
    };

    AddressEvidence {
        candidates,
        selected,
        status,
        confidence,
        warnings,
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_media(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    fetch_config: &FetchConfig,
    env_path: &Path,
    cache_dir: &Path,
    depth: InspectDepth,
    requested: &BTreeSet<EvidenceSection>,
    extracted: &RightmovePropertyExtract,
    source_listing: Option<&mut Listing>,
) -> MediaEvidence {
    let should_download =
        requested.contains(&EvidenceSection::Media) && depth != InspectDepth::Quick;
    if !should_download {
        return media_evidence(extracted);
    }

    let Some(listing) = source_listing else {
        return media_evidence(extracted);
    };

    let media_config = media_normalization_config(fetch_config, env_path);
    let _stats = runtime.block_on(populate_listing_media(
        client,
        listing,
        cache_dir,
        &media_config,
    ));

    media_evidence_from_listing(listing, cache_dir)
}

fn media_normalization_config(
    fetch_config: &FetchConfig,
    env_path: &Path,
) -> MediaNormalizationConfig {
    MediaNormalizationConfig {
        photo_landscape_width: fetch_config.media_photo_landscape_width,
        photo_landscape_height: fetch_config.media_photo_landscape_height,
        photo_portrait_width: fetch_config.media_photo_portrait_width,
        photo_portrait_height: fetch_config.media_photo_portrait_height,
        aux_width: fetch_config.media_aux_width,
        aux_height: fetch_config.media_aux_height,
        map_width: fetch_config.media_map_width,
        map_height: fetch_config.media_map_height,
        photo_quality: fetch_config.media_quality_photo,
        aux_quality: fetch_config.media_quality_aux,
        map_quality: fetch_config.media_quality_map,
        timeout: Duration::from_millis(fetch_config.media_timeout_ms),
        max_retries: fetch_config.max_retries,
        download_concurrency: fetch_config.media_download_concurrency,
        process_concurrency: fetch_config.media_process_concurrency,
        download_maps: fetch_config.download_maps,
        download_floorplan: fetch_config.download_floorplan,
        download_epc_asset: fetch_config.download_epc_asset,
        mapbox_access_token: resolve_env_var("MAPBOX_ACCESS_TOKEN", env_path),
    }
}

fn media_evidence(extracted: &RightmovePropertyExtract) -> MediaEvidence {
    MediaEvidence {
        photos: extracted
            .media
            .photos
            .iter()
            .map(|url| remote_media("photo", url))
            .collect(),
        floorplans: extracted
            .media
            .floorplans
            .iter()
            .map(|url| remote_media("floorplan", url))
            .collect(),
        epc_graphs: extracted
            .media
            .epc_graphs
            .iter()
            .map(|url| remote_media("epcGraph", url))
            .collect(),
        maps: Vec::new(),
    }
}

fn remote_media(kind: &str, url: &str) -> MediaItemEvidence {
    MediaItemEvidence {
        kind: kind.to_owned(),
        remote_url: url.to_owned(),
        local_path: None,
        width: None,
        height: None,
        content_hash: None,
        status: "remote".to_owned(),
    }
}

fn media_evidence_from_listing(listing: &Listing, cache_dir: &Path) -> MediaEvidence {
    let mut maps = Vec::new();
    if let Some(item) = asset_media("mapSatellite", &listing.map_views.satellite, cache_dir) {
        maps.push(item);
    }
    if let Some(item) = asset_media("mapStreet", &listing.map_views.street, cache_dir) {
        maps.push(item);
    }

    MediaEvidence {
        photos: listing
            .images
            .iter()
            .map(|image| media_item("photo", &image.remote, image.local.as_deref(), cache_dir))
            .collect(),
        floorplans: asset_media("floorplan", &listing.floorplan, cache_dir)
            .into_iter()
            .collect(),
        epc_graphs: asset_media("epcGraph", &listing.epc, cache_dir)
            .into_iter()
            .collect(),
        maps,
    }
}

fn asset_media(
    kind: &str,
    asset: &RemoteLocalAsset,
    cache_dir: &Path,
) -> Option<MediaItemEvidence> {
    let remote = asset.remote.as_ref()?;
    Some(media_item(kind, remote, asset.local.as_deref(), cache_dir))
}

fn media_item(
    kind: &str,
    remote_url: &str,
    local_path: Option<&str>,
    cache_dir: &Path,
) -> MediaItemEvidence {
    let absolute_local_path = local_path.map(|value| {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            cache_dir.join(path)
        }
    });
    let local_exists = absolute_local_path
        .as_ref()
        .is_some_and(|path| path.is_file());
    let dimensions = absolute_local_path
        .as_ref()
        .filter(|_| local_exists)
        .and_then(|path| image::image_dimensions(path).ok());
    let content_hash = absolute_local_path
        .as_ref()
        .filter(|_| local_exists)
        .and_then(|path| file_sha256(path).ok());
    let local_path = absolute_local_path
        .as_ref()
        .filter(|_| local_exists)
        .map(|path| path.display().to_string());

    MediaItemEvidence {
        kind: kind.to_owned(),
        remote_url: remote_url.to_owned(),
        local_path,
        width: dimensions.map(|(width, _)| width),
        height: dimensions.map(|(_, height)| height),
        content_hash,
        status: if local_exists {
            "cached".to_owned()
        } else {
            "remote".to_owned()
        },
    }
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    Ok(hex_encode(&digest))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for value in bytes {
        out.push(char::from(HEX[(value >> 4) as usize]));
        out.push(char::from(HEX[(value & 0x0f) as usize]));
    }
    out
}

fn extract_claims(
    entity_id: &str,
    extracted: &RightmovePropertyExtract,
    source: &SourceRef,
) -> Vec<ClaimEvidence> {
    let mut claims = Vec::new();
    let text = format!(
        "{} {}",
        extracted.description.key_features.join(" "),
        extracted.description.text
    );
    let lower = text.to_ascii_lowercase();
    if lower.contains("gigabit") {
        claims.push(ClaimEvidence {
            id: stable_id(&[entity_id, "claim", "broadband", "gigabit"]),
            claim_type: "broadband".to_owned(),
            claim_text: "description mentions gigabit broadband".to_owned(),
            value: json!({ "claimedCapability": "gigabit" }),
            source: source.clone(),
        });
    }
    if lower.contains("full fibre") || lower.contains("fttp") {
        claims.push(ClaimEvidence {
            id: stable_id(&[entity_id, "claim", "broadband", "fullFibre"]),
            claim_type: "broadband".to_owned(),
            claim_text: "description mentions full fibre broadband".to_owned(),
            value: json!({ "claimedCapability": "fullFibre" }),
            source: source.clone(),
        });
    }
    claims
}

fn verify_claims(
    entity_id: &str,
    claims: &[ClaimEvidence],
    broadband: Option<&BroadbandEvidence>,
    source: &SourceRef,
) -> Vec<VerificationEvidence> {
    claims
        .iter()
        .map(|claim| {
            if claim.claim_type == "broadband" {
                verify_broadband_claim(entity_id, claim, broadband, source)
            } else {
                VerificationEvidence {
                    id: stable_id(&[entity_id, "verification", &claim.id]),
                    claim_id: Some(claim.id.clone()),
                    claim_type: claim.claim_type.clone(),
                    status: VerificationStatus::InsufficientEvidence,
                    confidence: ConfidenceLevel::Unknown,
                    explanation: "no verifier is implemented for this claim type".to_owned(),
                    evidence: vec![source.clone()],
                }
            }
        })
        .collect()
}

fn verify_broadband_claim(
    entity_id: &str,
    claim: &ClaimEvidence,
    broadband: Option<&BroadbandEvidence>,
    source: &SourceRef,
) -> VerificationEvidence {
    let Some(profile) = broadband else {
        return VerificationEvidence {
            id: stable_id(&[entity_id, "verification", &claim.id]),
            claim_id: Some(claim.id.clone()),
            claim_type: claim.claim_type.clone(),
            status: VerificationStatus::InsufficientEvidence,
            confidence: ConfidenceLevel::Unknown,
            explanation: "no broadband database match was available for the listing postcode"
                .to_owned(),
            evidence: vec![source.clone()],
        };
    };
    let availability = profile.gigabit_availability.unwrap_or(0.0);
    let (status, confidence, explanation) = if availability >= 75.0 {
        (
            VerificationStatus::Supported,
            ConfidenceLevel::Probable,
            format!(
                "Ofcom postcode data reports {availability:.1}% gigabit availability for {}",
                profile
                    .postcode_display
                    .as_deref()
                    .unwrap_or(profile.postcode.as_str())
            ),
        )
    } else if availability < 1.0 {
        (
            VerificationStatus::Contradicted,
            ConfidenceLevel::Probable,
            format!(
                "Ofcom postcode data reports {availability:.1}% gigabit availability for {}",
                profile
                    .postcode_display
                    .as_deref()
                    .unwrap_or(profile.postcode.as_str())
            ),
        )
    } else {
        (
            VerificationStatus::Unknown,
            ConfidenceLevel::Ambiguous,
            format!(
                "Ofcom postcode data reports partial gigabit availability ({availability:.1}%) for {}; the claim needs provider-level confirmation",
                profile
                    .postcode_display
                    .as_deref()
                    .unwrap_or(profile.postcode.as_str())
            ),
        )
    };

    VerificationEvidence {
        id: stable_id(&[entity_id, "verification", &claim.id]),
        claim_id: Some(claim.id.clone()),
        claim_type: claim.claim_type.clone(),
        status,
        confidence,
        explanation,
        evidence: vec![SourceRef {
            source: "broadbandDb".to_owned(),
            snapshot_id: None,
            observation_id: None,
            url: None,
            captured_at: None,
        }],
    }
}

fn facts_from_extract(
    extracted: &RightmovePropertyExtract,
    broadband: Option<&BroadbandEvidence>,
    epc: Option<&EpcEvidence>,
    source: &SourceRef,
) -> Vec<FactEvidence> {
    let mut facts = Vec::new();
    push_fact(
        &mut facts,
        FactProvider::Rightmove,
        "rent",
        "pricePcm",
        extracted.price_pcm.map(|value| json!(value)),
        source,
    );
    push_fact(
        &mut facts,
        FactProvider::Rightmove,
        "layout",
        "bedrooms",
        extracted.bedrooms.map(|value| json!(value)),
        source,
    );
    push_fact(
        &mut facts,
        FactProvider::Rightmove,
        "layout",
        "bathrooms",
        extracted.bathrooms.map(|value| json!(value)),
        source,
    );
    if let Some(profile) = broadband {
        facts.push(FactEvidence {
            provider: FactProvider::BroadbandDb,
            category: "broadband".to_owned(),
            name: "gigabitAvailability".to_owned(),
            value: json!(profile.gigabit_availability),
            confidence: ConfidenceLevel::Probable,
            sources: vec![SourceRef {
                source: "broadbandDb".to_owned(),
                snapshot_id: None,
                observation_id: None,
                url: None,
                captured_at: None,
            }],
        });
    }
    if let Some(epc) = epc {
        push_epc_fact(
            &mut facts,
            "rating",
            epc.rating.as_ref().map(|value| json!(value)),
        );
        push_epc_fact(
            &mut facts,
            "floorAreaSqm",
            epc.floor_area_sqm.map(|value| json!(value)),
        );
        push_epc_fact(
            &mut facts,
            "uprn",
            epc.uprn.as_ref().map(|value| json!(value)),
        );
    }
    facts
}

fn source_facts(sources_dir: &Path, listing: &mut Listing) -> Vec<FactEvidence> {
    let Ok(enricher) = SourceEnricher::open(sources_dir) else {
        return Vec::new();
    };
    if enricher
        .enrich_listing(listing, EnrichmentMode::FillMissingFromSources)
        .is_err()
    {
        return Vec::new();
    }

    let mut facts = Vec::new();
    push_source_fact(
        &mut facts,
        FactProvider::PostcodesDb,
        "area",
        "lsoaCode",
        listing.area.lsoa.code.as_ref().map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::PostcodesDb,
        "area",
        "lsoaName",
        listing.area.lsoa.name.as_ref().map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::PostcodesDb,
        "area",
        "msoaCode",
        listing.area.msoa.code.as_ref().map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::PostcodesDb,
        "area",
        "msoaName",
        listing.area.msoa.name.as_ref().map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::DeprivationDb,
        "deprivation",
        "imdRank",
        listing.area.imd.rank.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::DeprivationDb,
        "deprivation",
        "imdDecile",
        listing.area.imd.decile.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::DeprivationDb,
        "deprivation",
        "imdScore",
        listing.area.imd.score.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::IncomeDb,
        "income",
        "incomeBhc",
        listing.area.income.bhc.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::IncomeDb,
        "income",
        "incomeAhc",
        listing.area.income.ahc.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::CensusDb,
        "housing",
        "socialHousingPct",
        listing.area.social_housing_pct.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::PopulationDb,
        "population",
        "population",
        listing.area.population.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::FloodDb,
        "flood",
        "riskLevel",
        listing
            .area
            .flood_risk
            .level
            .as_ref()
            .map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::FloodDb,
        "flood",
        "riskSource",
        listing
            .area
            .flood_risk
            .source
            .as_ref()
            .map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::CrimeDb,
        "crime",
        "count12m",
        listing.area.crime.count_12m.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::CrimeDb,
        "crime",
        "ratePer1k",
        listing.area.crime.rate_per_1k.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::CrimeDb,
        "crime",
        "violent12m",
        listing.area.crime.violent_12m.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::CrimeDb,
        "crime",
        "burglary12m",
        listing.area.crime.burglary_12m.map(|value| json!(value)),
    );
    push_source_fact(
        &mut facts,
        FactProvider::CrimeDb,
        "crime",
        "robbery12m",
        listing.area.crime.robbery_12m.map(|value| json!(value)),
    );
    facts
}

fn push_source_fact(
    facts: &mut Vec<FactEvidence>,
    provider: FactProvider,
    category: &str,
    name: &str,
    value: Option<Value>,
) {
    if let Some(value) = value {
        facts.push(FactEvidence {
            provider,
            category: category.to_owned(),
            name: name.to_owned(),
            value,
            confidence: ConfidenceLevel::Probable,
            sources: vec![SourceRef {
                source: format!("{provider:?}"),
                snapshot_id: None,
                observation_id: None,
                url: None,
                captured_at: None,
            }],
        });
    }
}

fn push_epc_fact(facts: &mut Vec<FactEvidence>, name: &str, value: Option<Value>) {
    if let Some(value) = value {
        facts.push(FactEvidence {
            provider: FactProvider::EpcApi,
            category: "epc".to_owned(),
            name: name.to_owned(),
            value,
            confidence: ConfidenceLevel::Probable,
            sources: vec![SourceRef {
                source: "epcApi".to_owned(),
                snapshot_id: None,
                observation_id: None,
                url: None,
                captured_at: None,
            }],
        });
    }
}

fn push_fact(
    facts: &mut Vec<FactEvidence>,
    provider: FactProvider,
    category: &str,
    name: &str,
    value: Option<Value>,
    source: &SourceRef,
) {
    if let Some(value) = value {
        facts.push(FactEvidence {
            provider,
            category: category.to_owned(),
            name: name.to_owned(),
            value,
            confidence: ConfidenceLevel::Probable,
            sources: vec![source.clone()],
        });
    }
}

fn section_states(
    bundle: &EvidenceBundle,
    requested: &BTreeSet<EvidenceSection>,
    source_ref: &SourceRef,
    epc_warnings: Vec<String>,
) -> BTreeMap<String, SectionState> {
    let mut sections = BTreeMap::new();
    for section in requested {
        let mut state = match section {
            EvidenceSection::Rightmove => SectionState::ok(
                "Rightmove PAGE_MODEL captured and extracted",
                ConfidenceLevel::Probable,
            ),
            EvidenceSection::Description => SectionState::ok(
                "description text, raw HTML, and key features preserved",
                ConfidenceLevel::Probable,
            ),
            EvidenceSection::Address => SectionState {
                status: bundle.address.status,
                confidence: bundle.address.confidence,
                summary: if bundle.address.selected.is_some() {
                    "address candidates resolved".to_owned()
                } else {
                    "no address candidate could be selected".to_owned()
                },
                warnings: bundle.address.warnings.clone(),
                sources: Vec::new(),
            },
            EvidenceSection::Facts => {
                if bundle.facts.is_empty() {
                    SectionState::degraded("no facts extracted", Vec::new())
                } else {
                    SectionState::ok(
                        "facts extracted from available providers",
                        ConfidenceLevel::Probable,
                    )
                }
            }
            EvidenceSection::Claims => {
                if bundle.claims.is_empty() {
                    SectionState::skipped("no checkable claims found in the listing description")
                } else {
                    SectionState::ok("checkable claims extracted", ConfidenceLevel::Heuristic)
                }
            }
            EvidenceSection::Broadband => {
                if bundle.broadband.is_some() {
                    SectionState::ok(
                        "broadband database matched the listing postcode",
                        ConfidenceLevel::Probable,
                    )
                } else {
                    SectionState::degraded(
                        "broadband evidence unavailable",
                        vec!["build broadband sources or verify the listing postcode".to_owned()],
                    )
                }
            }
            EvidenceSection::Epc => {
                if bundle.epc.is_some() {
                    SectionState::ok(
                        "EPC certificate matched the listing address",
                        ConfidenceLevel::Probable,
                    )
                } else {
                    SectionState::degraded("EPC evidence unavailable", epc_warnings.clone())
                }
            }
            EvidenceSection::Media => {
                let count = bundle.media.photos.len()
                    + bundle.media.floorplans.len()
                    + bundle.media.epc_graphs.len()
                    + bundle.media.maps.len();
                let local_count = bundle
                    .media
                    .photos
                    .iter()
                    .chain(bundle.media.floorplans.iter())
                    .chain(bundle.media.epc_graphs.iter())
                    .chain(bundle.media.maps.iter())
                    .filter(|item| item.local_path.is_some())
                    .count();
                if count == 0 {
                    SectionState::degraded("no Rightmove media URLs extracted", Vec::new())
                } else if local_count > 0 {
                    SectionState::ok(
                        format!("{count} media assets extracted; {local_count} cached locally"),
                        ConfidenceLevel::Probable,
                    )
                } else {
                    SectionState::ok(
                        format!("{count} remote media assets extracted"),
                        ConfidenceLevel::Probable,
                    )
                }
            }
            EvidenceSection::Verifications => {
                if bundle.verifications.is_empty() {
                    SectionState::skipped("no verifications were needed")
                } else {
                    SectionState::ok(
                        "claims verified against available sources",
                        ConfidenceLevel::Probable,
                    )
                }
            }
            EvidenceSection::Assessment => {
                if bundle.assessment.is_some() {
                    SectionState::ok("agent assessment is saved", ConfidenceLevel::Exact)
                } else {
                    SectionState::skipped("no agent assessment saved")
                }
            }
        };
        if matches!(
            section,
            EvidenceSection::Rightmove | EvidenceSection::Description | EvidenceSection::Media
        ) {
            state.sources.push(source_ref.clone());
        }
        sections.insert(section.as_str().to_owned(), state);
    }
    sections
}

fn next_actions(bundle: &EvidenceBundle) -> Vec<String> {
    let mut actions = Vec::new();
    if bundle.broadband.is_none() {
        actions.push(
            "run `let sources build broadband` if broadband source data is missing".to_owned(),
        );
    }
    if bundle.address.confidence != ConfidenceLevel::Exact {
        actions.push(
            "verify the address manually or provide a more precise postcode/address".to_owned(),
        );
    }
    if bundle.epc.is_none() && bundle.sections.contains_key("epc") {
        actions.push("set EPC credentials or verify the EPC certificate manually".to_owned());
    }
    if bundle.assessment.is_none() {
        actions.push(
            "save the agent assessment with `let assess save <id> <assessment-json>`".to_owned(),
        );
    }
    actions
}

fn resolve_env_var(key: &str, env_file: &Path) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    let content = fs::read_to_string(env_file).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some((candidate_key, raw_value)) = assignment.split_once('=') else {
            continue;
        };
        if candidate_key.trim() != key {
            continue;
        }
        let value = parse_env_value(raw_value.trim());
        if !value.trim().is_empty() {
            return Some(value);
        }
    }
    None
}

fn parse_env_value(raw: &str) -> String {
    if raw.len() >= 2 {
        let single = raw.starts_with('\'') && raw.ends_with('\'');
        let double = raw.starts_with('"') && raw.ends_with('"');
        if single || double {
            return raw[1..raw.len() - 1].to_owned();
        }
    }

    let mut output = String::with_capacity(raw.len());
    let mut previous_whitespace = true;
    for ch in raw.chars() {
        if ch == '#' && previous_whitespace {
            break;
        }
        output.push(ch);
        previous_whitespace = ch.is_whitespace();
    }
    output.trim_end().to_owned()
}

fn snapshot_id(entity_id: &str, source: &str, key: &str) -> String {
    stable_id(&[entity_id, "snapshot", source, key])
}

fn stable_id(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod media_tests {
    use tempfile::TempDir;

    use crate::intelligence::types::{CorrectionKind, CorrectionRecord};

    use super::{
        address_candidates_from_corrections, correction_f64, correction_string,
        epc_from_correction, media_item,
    };

    #[test]
    fn missing_local_media_path_stays_remote() {
        let cache = TempDir::new().expect("temp dir");

        let item = media_item(
            "photo",
            "https://media.example/photo.jpg",
            Some("missing-photo.jpg"),
            cache.path(),
        );

        assert_eq!(item.status, "remote");
        assert_eq!(item.local_path, None);
        assert_eq!(item.width, None);
        assert_eq!(item.height, None);
        assert_eq!(item.content_hash, None);
    }

    #[test]
    fn address_correction_creates_manual_candidate() {
        let corrections = vec![CorrectionRecord {
            id: "correction-1".to_owned(),
            entity_id: "rightmove:1".to_owned(),
            kind: CorrectionKind::Address,
            payload: serde_json::json!({
                "address": "Flat 2, 10 Example Street",
                "postcode": "YO1 7HH",
                "lat": 53.959,
                "lng": -1.0815
            }),
            note: Some("manual map check".to_owned()),
            active: true,
            created_at: "2026-06-18T00:00:00Z".to_owned(),
            cleared_at: None,
            affected_sections: vec!["address".to_owned()],
        }];

        let candidates = address_candidates_from_corrections(&corrections);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source, "manualCorrection");
        assert_eq!(candidates[0].postcode.as_deref(), Some("YO1 7HH"));
        assert_eq!(candidates[0].latitude, Some(53.959));
        assert_eq!(candidates[0].longitude, Some(-1.0815));
    }

    #[test]
    fn epc_correction_pins_selected_evidence() {
        let corrections = vec![CorrectionRecord {
            id: "correction-1".to_owned(),
            entity_id: "rightmove:1".to_owned(),
            kind: CorrectionKind::Epc,
            payload: serde_json::json!({
                "lmkKey": "abc123",
                "rating": "C",
                "floorAreaSqm": 92.5,
                "uprn": "1001"
            }),
            note: Some("matched gov EPC".to_owned()),
            active: true,
            created_at: "2026-06-18T00:00:00Z".to_owned(),
            cleared_at: None,
            affected_sections: vec!["epc".to_owned()],
        }];

        let epc = epc_from_correction(&corrections).expect("epc correction");

        assert_eq!(epc.lmk_key, "abc123");
        assert_eq!(epc.rating.as_deref(), Some("C"));
        assert_eq!(epc.floor_area_sqm, Some(92.5));
        assert_eq!(epc.uprn.as_deref(), Some("1001"));
        assert_eq!(epc.uprn_source.as_deref(), Some("manualCorrection"));
    }

    #[test]
    fn correction_lookup_skips_newer_records_without_requested_field() {
        let corrections = vec![
            CorrectionRecord {
                id: "correction-1".to_owned(),
                entity_id: "rightmove:1".to_owned(),
                kind: CorrectionKind::Address,
                payload: serde_json::json!({
                    "postcode": "YO1 7HH"
                }),
                note: None,
                active: true,
                created_at: "2026-06-18T00:00:00Z".to_owned(),
                cleared_at: None,
                affected_sections: vec!["address".to_owned()],
            },
            CorrectionRecord {
                id: "correction-2".to_owned(),
                entity_id: "rightmove:1".to_owned(),
                kind: CorrectionKind::Address,
                payload: serde_json::json!({
                    "lat": 53.959,
                    "lng": -1.0815
                }),
                note: None,
                active: true,
                created_at: "2026-06-18T01:00:00Z".to_owned(),
                cleared_at: None,
                affected_sections: vec!["address".to_owned()],
            },
        ];

        assert_eq!(
            correction_string(&corrections, CorrectionKind::Address, "postcode").as_deref(),
            Some("YO1 7HH")
        );
        assert_eq!(
            correction_f64(&corrections, CorrectionKind::Address, "lat"),
            Some(53.959)
        );
    }
}
