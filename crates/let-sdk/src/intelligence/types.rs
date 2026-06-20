#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InspectDepth {
    Quick,
    Standard,
    Deep,
}

impl InspectDepth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Standard => "standard",
            Self::Deep => "deep",
        }
    }
}

impl FromStr for InspectDepth {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "quick" => Ok(Self::Quick),
            "standard" => Ok(Self::Standard),
            "deep" => Ok(Self::Deep),
            other => Err(format!(
                "invalid inspect depth `{other}`; expected quick, standard, or deep"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshPolicy {
    None,
    Stale,
    All,
}

impl RefreshPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Stale => "stale",
            Self::All => "all",
        }
    }
}

impl FromStr for RefreshPolicy {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "stale" => Ok(Self::Stale),
            "all" => Ok(Self::All),
            other => Err(format!(
                "invalid refresh policy `{other}`; expected none, stale, or all"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceSection {
    Rightmove,
    Description,
    Address,
    Facts,
    Claims,
    Broadband,
    Epc,
    Media,
    Verifications,
    Assessment,
}

impl EvidenceSection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rightmove => "rightmove",
            Self::Description => "description",
            Self::Address => "address",
            Self::Facts => "facts",
            Self::Claims => "claims",
            Self::Broadband => "broadband",
            Self::Epc => "epc",
            Self::Media => "media",
            Self::Verifications => "verifications",
            Self::Assessment => "assessment",
        }
    }

    pub fn default_sections(depth: InspectDepth) -> Vec<Self> {
        match depth {
            InspectDepth::Quick => vec![Self::Rightmove, Self::Description, Self::Media],
            InspectDepth::Standard => vec![
                Self::Rightmove,
                Self::Description,
                Self::Address,
                Self::Facts,
                Self::Claims,
                Self::Broadband,
                Self::Media,
                Self::Verifications,
            ],
            InspectDepth::Deep => vec![
                Self::Rightmove,
                Self::Description,
                Self::Address,
                Self::Facts,
                Self::Claims,
                Self::Broadband,
                Self::Epc,
                Self::Media,
                Self::Verifications,
                Self::Assessment,
            ],
        }
    }
}

impl FromStr for EvidenceSection {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rightmove" => Ok(Self::Rightmove),
            "description" => Ok(Self::Description),
            "address" => Ok(Self::Address),
            "facts" => Ok(Self::Facts),
            "claims" => Ok(Self::Claims),
            "broadband" => Ok(Self::Broadband),
            "epc" => Ok(Self::Epc),
            "media" => Ok(Self::Media),
            "verifications" | "verify" => Ok(Self::Verifications),
            "assessment" | "assess" => Ok(Self::Assessment),
            other => Err(format!("unknown evidence section `{other}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SectionStatus {
    Ok,
    Partial,
    Degraded,
    Blocked,
    Skipped,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfidenceLevel {
    Exact,
    Probable,
    Heuristic,
    Ambiguous,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerificationStatus {
    Supported,
    Contradicted,
    Unknown,
    InsufficientEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CorrectionKind {
    Address,
    Epc,
    Media,
}

impl CorrectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Address => "address",
            Self::Epc => "epc",
            Self::Media => "media",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionState {
    pub status: SectionStatus,
    pub confidence: ConfidenceLevel,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRef>,
}

impl SectionState {
    pub fn ok(summary: impl Into<String>, confidence: ConfidenceLevel) -> Self {
        Self {
            status: SectionStatus::Ok,
            confidence,
            summary: summary.into(),
            warnings: Vec::new(),
            sources: Vec::new(),
        }
    }

    pub fn skipped(summary: impl Into<String>) -> Self {
        Self {
            status: SectionStatus::Skipped,
            confidence: ConfidenceLevel::Unknown,
            summary: summary.into(),
            warnings: Vec::new(),
            sources: Vec::new(),
        }
    }

    pub fn degraded(summary: impl Into<String>, warnings: Vec<String>) -> Self {
        Self {
            status: SectionStatus::Degraded,
            confidence: ConfidenceLevel::Unknown,
            summary: summary.into(),
            warnings,
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSnapshotEvidence {
    pub id: String,
    pub source: String,
    pub source_key: String,
    pub url: String,
    pub captured_at: String,
    pub status: String,
    pub content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_json: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RightmoveEvidence {
    pub rightmove_id: String,
    pub url: String,
    pub page_status: String,
    pub fetched_at: String,
    pub content_hash: String,
    pub title: Option<String>,
    pub address: Option<String>,
    pub postcode: Option<String>,
    pub display_price: Option<String>,
    pub price_pcm: Option<i64>,
    pub bedrooms: Option<i64>,
    pub bathrooms: Option<i64>,
    pub property_type: Option<String>,
    pub agent_name: Option<String>,
    pub agent_phone: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub pin_type: Option<String>,
    pub listed_date: Option<String>,
    pub available_date: Option<String>,
    pub deposit: Option<i64>,
    pub description: DescriptionEvidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaItemEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionEvidence {
    pub raw_html: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_features: Vec<String>,
    pub normalized_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressCandidateEvidence {
    pub source: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postcode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    pub confidence: ConfidenceLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressEvidence {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<AddressCandidateEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<AddressCandidateEvidence>,
    pub status: SectionStatus,
    pub confidence: ConfidenceLevel,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactEvidence {
    pub provider: FactProvider,
    pub category: String,
    pub name: String,
    pub value: Value,
    pub confidence: ConfidenceLevel,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FactProvider {
    Rightmove,
    ManualCorrection,
    BroadbandDb,
    PostcodesDb,
    DeprivationDb,
    CensusDb,
    PopulationDb,
    IncomeDb,
    FloodDb,
    CrimeDb,
    NaptanDb,
    UprnDb,
    EpcApi,
    Mapbox,
    Derived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadbandEvidence {
    pub postcode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postcode_display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outward: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gigabit_availability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pct_over_300mbps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ufbb_availability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sfbb_availability: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpcEvidence {
    pub lmk_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor_area_sqm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lodgement_date: Option<String>,
    pub address_match: bool,
    pub matched_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uprn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uprn_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimEvidence {
    pub id: String,
    pub claim_type: String,
    pub claim_text: String,
    pub value: Value,
    pub source: SourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationEvidence {
    pub id: String,
    pub claim_id: Option<String>,
    pub claim_type: String,
    pub status: VerificationStatus,
    pub confidence: ConfidenceLevel,
    pub explanation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<SourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItemEvidence {
    pub kind: String,
    pub remote_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaEvidence {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub photos: Vec<MediaItemEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floorplans: Vec<MediaItemEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub epc_graphs: Vec<MediaItemEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maps: Vec<MediaItemEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_sheet: Option<ContactSheetEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContactSheetEvidence {
    pub status: String,
    pub photo_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentRecord {
    pub entity_id: String,
    pub assessment: Value,
    #[serde(default)]
    pub normalized_assessment: NormalizedAssessment,
    pub saved_at: String,
}

impl AssessmentRecord {
    pub fn new(entity_id: String, assessment: Value, saved_at: String) -> Self {
        let normalized_assessment = normalize_assessment(&assessment);
        Self {
            entity_id,
            assessment,
            normalized_assessment,
            saved_at,
        }
    }

    pub fn refresh_normalized(&mut self) {
        self.normalized_assessment = normalize_assessment(&self.assessment);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedAssessment {
    pub recommendation: Option<String>,
    pub confidence: Option<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub positives: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub tradeoffs: Vec<String>,
    pub area_notes: Option<String>,
    pub commute_notes: Option<String>,
    pub family_fit: Option<String>,
    #[serde(default)]
    pub evidence_gaps: Vec<String>,
    pub source: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListingListFilters {
    pub recommendation: Option<String>,
    pub confidence: Option<String>,
    pub area: Option<String>,
    pub max_price: Option<i64>,
    pub postcode_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredListingSummary {
    pub id: String,
    pub entity_id: String,
    pub url: Option<String>,
    pub address: Option<String>,
    pub postcode: Option<String>,
    pub area: Option<String>,
    pub price: Option<String>,
    pub price_pcm: Option<i64>,
    pub recommendation: Option<String>,
    pub confidence: Option<String>,
    pub saved_at: Option<String>,
    pub inspected_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredAssessmentSummary {
    #[serde(flatten)]
    pub listing: StoredListingSummary,
    pub summary: Option<String>,
    #[serde(default)]
    pub positives: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub tradeoffs: Vec<String>,
    pub area_notes: Option<String>,
    pub commute_notes: Option<String>,
    pub family_fit: Option<String>,
    #[serde(default)]
    pub evidence_gaps: Vec<String>,
    pub source: Option<String>,
    #[serde(default)]
    pub normalized_warnings: Vec<String>,
    pub normalized_assessment: NormalizedAssessment,
    pub assessment: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionRecord {
    pub id: String,
    pub entity_id: String,
    pub kind: CorrectionKind,
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub active: bool,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceBundle {
    pub entity_id: String,
    pub rightmove_id: String,
    pub url: String,
    pub generated_at: String,
    pub depth: InspectDepth,
    pub refresh: RefreshPolicy,
    #[serde(default)]
    pub sections: BTreeMap<String, SectionState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_snapshots: Vec<SourceSnapshotEvidence>,
    pub rightmove: RightmoveEvidence,
    pub address: AddressEvidence,
    #[serde(default)]
    pub facts: Vec<FactEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadband: Option<BroadbandEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epc: Option<EpcEvidence>,
    #[serde(default)]
    pub claims: Vec<ClaimEvidence>,
    #[serde(default)]
    pub verifications: Vec<VerificationEvidence>,
    pub media: MediaEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assessment: Option<AssessmentRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corrections: Vec<CorrectionRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<EvidenceQualityFlag>,
}

impl EvidenceBundle {
    pub fn refresh_derived(&mut self) {
        if let Some(assessment) = self.assessment.as_mut() {
            assessment.refresh_normalized();
        }
        self.flags = compute_evidence_quality_flags(self);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceQualityFlag {
    pub severity: String,
    pub category: String,
    pub code: String,
    pub summary: String,
    #[serde(default)]
    pub sources: Vec<String>,
    pub recommended_action: String,
}

pub fn normalize_assessment(assessment: &Value) -> NormalizedAssessment {
    let mut normalized = NormalizedAssessment {
        recommendation: normalized_enum_text(
            assessment_text(assessment, &["recommendation"]),
            &[
                "view",
                "stretch_view",
                "backup_view",
                "watch",
                "pass",
                "benchmark",
            ],
            "recommendation",
        ),
        confidence: normalized_enum_text(
            assessment_text(assessment, &["confidence"]),
            &["high", "medium_high", "medium", "low"],
            "confidence",
        ),
        summary: assessment_text(assessment, &["summary"]),
        positives: assessment_text_list(assessment, &["positives", "pros"]),
        risks: assessment_text_list(assessment, &["risks", "cons"]),
        next_actions: assessment_text_list(assessment, &["nextActions", "next_actions"]),
        tradeoffs: assessment_text_list(assessment, &["tradeoffs", "tradeOffs", "trade_offs"]),
        area_notes: assessment_text(assessment, &["areaNotes", "area_notes"]),
        commute_notes: assessment_text(assessment, &["commuteNotes", "commute_notes"]),
        family_fit: assessment_text(assessment, &["familyFit", "family_fit"]),
        evidence_gaps: assessment_text_list(assessment, &["evidenceGaps", "evidence_gaps"]),
        source: assessment_text(assessment, &["source"]),
        warnings: Vec::new(),
    };

    warn_unknown_enum(
        &mut normalized,
        "recommendation",
        &[
            "view",
            "stretch_view",
            "backup_view",
            "watch",
            "pass",
            "benchmark",
        ],
    );
    warn_unknown_enum(
        &mut normalized,
        "confidence",
        &["high", "medium_high", "medium", "low"],
    );

    normalized
}

fn assessment_text(assessment: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = assessment.get(*key).and_then(value_to_text) {
            return Some(value);
        }
    }
    None
}

fn assessment_text_list(assessment: &Value, keys: &[&str]) -> Vec<String> {
    let Some(value) = keys.iter().find_map(|key| assessment.get(*key)) else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items.iter().filter_map(value_to_text).collect(),
        other => value_to_text(other).into_iter().collect(),
    }
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => non_empty_text(text),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn normalized_enum_text(value: Option<String>, _allowed: &[&str], _field: &str) -> Option<String> {
    value
        .map(|text| normalize_token(&text))
        .filter(|text| !text.is_empty())
}

fn warn_unknown_enum(assessment: &mut NormalizedAssessment, field: &str, allowed: &[&str]) {
    let value = match field {
        "recommendation" => assessment.recommendation.as_deref(),
        "confidence" => assessment.confidence.as_deref(),
        _ => None,
    };
    let Some(value) = value else {
        return;
    };
    if !allowed.iter().any(|allowed| allowed == &value) {
        assessment.warnings.push(format!(
            "`{field}` value `{value}` is outside the recommended assessment vocabulary"
        ));
    }
}

fn normalize_token(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

pub fn compute_evidence_quality_flags(bundle: &EvidenceBundle) -> Vec<EvidenceQualityFlag> {
    let mut flags = Vec::new();

    if bundle.sections.contains_key("epc") && bundle.epc.is_none() {
        push_flag(
            &mut flags,
            "warning",
            "missing_required_evidence",
            "missing_epc",
            "EPC evidence is unavailable for this listing.",
            ["epcApi"],
            "set EPC credentials, inspect again, or verify the EPC certificate manually",
        );
    }

    if bundle.epc.as_ref().is_some_and(|epc| !epc.address_match) {
        push_flag(
            &mut flags,
            "warning",
            "source_conflict",
            "epc_address_mismatch",
            "The selected EPC certificate address does not match the resolved listing address.",
            ["epcApi", "rightmove"],
            "verify the EPC certificate at viewing or record an EPC correction",
        );
    }

    if suspicious_floor_area(bundle) {
        push_flag(
            &mut flags,
            "warning",
            "suspicious_value",
            "suspicious_floor_area",
            "The EPC floor area is unusually large for the captured listing type and bedroom count.",
            ["epcApi", "rightmove"],
            "confirm the floor area against the floorplan, agent, or EPC certificate",
        );
    }

    if let Some(availability) = bundle
        .broadband
        .as_ref()
        .and_then(|broadband| broadband.gigabit_availability)
    {
        if availability < 1.0 {
            push_flag(
                &mut flags,
                "warning",
                "source_degraded",
                "low_gigabit_coverage",
                "Broadband evidence reports no or near-zero gigabit availability for the listing postcode.",
                ["broadbandDb"],
                "confirm available broadband providers before relying on listing broadband claims",
            );
        } else if availability < 75.0 {
            push_flag(
                &mut flags,
                "info",
                "manual_verification_needed",
                "partial_gigabit_coverage",
                "Broadband evidence reports only partial gigabit availability for the listing postcode.",
                ["broadbandDb"],
                "check provider-level availability for the exact address",
            );
        }
    }

    if address_is_degraded(bundle) {
        push_flag(
            &mut flags,
            "warning",
            "source_degraded",
            "address_degraded",
            "The address or postcode resolution is missing, ambiguous, or degraded.",
            ["rightmove", "postcodesDb"],
            "verify the address manually or record an address correction",
        );
    }

    if media_is_degraded(bundle) {
        push_flag(
            &mut flags,
            "warning",
            "source_degraded",
            "media_degraded",
            "Media evidence is unavailable, degraded, or has no locally cached photos.",
            ["rightmoveMedia"],
            "rerun inspect with media enabled or review the remote media manually",
        );
    }

    if bundle
        .media
        .contact_sheet
        .as_ref()
        .is_some_and(|sheet| sheet.status == "failed")
    {
        push_flag(
            &mut flags,
            "warning",
            "source_degraded",
            "contact_sheet_failed",
            "The media contact sheet could not be generated from cached photos.",
            ["rightmoveMedia"],
            "open individual cached photos or rerun inspect with media enabled",
        );
    }

    for (name, state) in &bundle.sections {
        if matches!(
            state.status,
            SectionStatus::Degraded | SectionStatus::Blocked
        ) {
            push_flag(
                &mut flags,
                "warning",
                "source_degraded",
                "source_section_degraded",
                format!(
                    "Evidence section `{name}` is {}.",
                    section_status_label(state.status)
                ),
                [name.as_str()],
                "inspect the section warnings and rerun or correct the underlying evidence",
            );
        }
    }

    if high_crime_rate(bundle) {
        push_flag(
            &mut flags,
            "warning",
            "suspicious_value",
            "high_crime_rate",
            "Crime source data reports a high crime rate per 1,000 residents for the area.",
            ["crimeDb"],
            "review the source context and compare against nearby candidate areas",
        );
    }

    if bundle
        .corrections
        .iter()
        .any(|correction| correction.active)
    {
        push_flag(
            &mut flags,
            "info",
            "manual_verification_needed",
            "active_manual_correction",
            "One or more active manual corrections are influencing the evidence bundle.",
            ["manualCorrection"],
            "keep the correction note with the viewing evidence and clear it if source data is fixed",
        );
    }

    flags
}

fn push_flag<const N: usize>(
    flags: &mut Vec<EvidenceQualityFlag>,
    severity: &str,
    category: &str,
    code: &str,
    summary: impl Into<String>,
    sources: [&str; N],
    recommended_action: &str,
) {
    flags.push(EvidenceQualityFlag {
        severity: severity.to_owned(),
        category: category.to_owned(),
        code: code.to_owned(),
        summary: summary.into(),
        sources: sources.iter().map(|source| (*source).to_owned()).collect(),
        recommended_action: recommended_action.to_owned(),
    });
}

fn suspicious_floor_area(bundle: &EvidenceBundle) -> bool {
    let Some(area) = bundle.epc.as_ref().and_then(|epc| epc.floor_area_sqm) else {
        return false;
    };
    if area > 500.0 {
        return true;
    }
    let bedrooms = bundle.rightmove.bedrooms.unwrap_or_default();
    let property_type = bundle
        .rightmove
        .property_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_flat = ["flat", "apartment", "maisonette"]
        .iter()
        .any(|token| property_type.contains(token));
    if bedrooms <= 2 && is_flat && area > 150.0 {
        return true;
    }
    bedrooms <= 2 && property_type.contains("house") && area > 220.0
}

fn address_is_degraded(bundle: &EvidenceBundle) -> bool {
    bundle.address.selected.is_none()
        || matches!(
            bundle.address.status,
            SectionStatus::Degraded | SectionStatus::Blocked | SectionStatus::Stale
        )
        || matches!(
            bundle.address.confidence,
            ConfidenceLevel::Ambiguous | ConfidenceLevel::Unknown
        )
}

fn media_is_degraded(bundle: &EvidenceBundle) -> bool {
    let Some(state) = bundle.sections.get("media") else {
        return false;
    };
    if matches!(
        state.status,
        SectionStatus::Degraded | SectionStatus::Blocked
    ) {
        return true;
    }
    let has_local_photo = bundle
        .media
        .photos
        .iter()
        .any(|item| item.local_path.is_some());
    bundle.depth != InspectDepth::Quick && !has_local_photo && !bundle.media.photos.is_empty()
}

fn high_crime_rate(bundle: &EvidenceBundle) -> bool {
    bundle.facts.iter().any(|fact| {
        fact.provider == FactProvider::CrimeDb
            && matches!(fact.name.as_str(), "ratePer1k" | "crimeRatePer1k")
            && fact.value.as_f64().is_some_and(|value| value >= 150.0)
    })
}

fn section_status_label(status: SectionStatus) -> &'static str {
    match status {
        SectionStatus::Ok => "ok",
        SectionStatus::Partial => "partial",
        SectionStatus::Degraded => "degraded",
        SectionStatus::Blocked => "blocked",
        SectionStatus::Skipped => "skipped",
        SectionStatus::Stale => "stale",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn normalizes_recommended_assessment_fields() {
        let normalized = normalize_assessment(&json!({
            "recommendation": "Stretch View",
            "confidence": "medium-high",
            "summary": "Worth viewing if evidence checks out.",
            "positives": "station access",
            "risks": ["EPC mismatch"],
            "next_actions": ["call agent"],
            "tradeoffs": ["smaller garden"],
            "area_notes": "walkable",
            "commuteNotes": "direct train",
            "family_fit": "plausible",
            "evidenceGaps": "floor area",
            "source": "agent"
        }));

        assert_eq!(normalized.recommendation.as_deref(), Some("stretch_view"));
        assert_eq!(normalized.confidence.as_deref(), Some("medium_high"));
        assert_eq!(normalized.positives, vec!["station access"]);
        assert_eq!(normalized.risks, vec!["EPC mismatch"]);
        assert_eq!(normalized.next_actions, vec!["call agent"]);
        assert_eq!(normalized.evidence_gaps, vec!["floor area"]);
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn flags_generic_evidence_quality_issues() {
        let mut bundle = test_bundle();
        bundle.broadband = Some(BroadbandEvidence {
            postcode: "M11AA".to_owned(),
            postcode_display: Some("M1 1AA".to_owned()),
            outward: Some("M1".to_owned()),
            area: Some("M".to_owned()),
            gigabit_availability: Some(0.0),
            pct_over_300mbps: None,
            ufbb_availability: None,
            sfbb_availability: None,
        });
        bundle.epc = Some(EpcEvidence {
            lmk_key: "lmk".to_owned(),
            rating: Some("C".to_owned()),
            floor_area_sqm: Some(248.0),
            lodgement_date: None,
            address_match: false,
            matched_address: "Wrong Flat".to_owned(),
            uprn: None,
            uprn_source: None,
        });
        bundle.address.selected = None;
        bundle.address.confidence = ConfidenceLevel::Unknown;
        bundle.media.photos = vec![MediaItemEvidence {
            kind: "photo".to_owned(),
            remote_url: "https://media.rightmove.co.uk/photo.jpg".to_owned(),
            local_path: None,
            width: None,
            height: None,
            content_hash: None,
            status: "remote".to_owned(),
        }];
        bundle.media.contact_sheet = Some(ContactSheetEvidence {
            status: "failed".to_owned(),
            photo_count: 1,
            local_path: None,
            generated_at: None,
            width: None,
            height: None,
            content_hash: None,
        });
        bundle.facts.push(FactEvidence {
            provider: FactProvider::CrimeDb,
            category: "crime".to_owned(),
            name: "ratePer1k".to_owned(),
            value: json!(151.0),
            confidence: ConfidenceLevel::Probable,
            sources: Vec::new(),
        });
        bundle.corrections.push(CorrectionRecord {
            id: "correction-1".to_owned(),
            entity_id: bundle.entity_id.clone(),
            kind: CorrectionKind::Address,
            payload: json!({"postcode":"M1 1AA"}),
            note: None,
            active: true,
            created_at: "2026-06-20T00:00:00Z".to_owned(),
            cleared_at: None,
            affected_sections: vec!["address".to_owned()],
        });

        let codes = compute_evidence_quality_flags(&bundle)
            .into_iter()
            .map(|flag| flag.code)
            .collect::<Vec<_>>();

        assert!(codes.contains(&"epc_address_mismatch".to_owned()));
        assert!(codes.contains(&"suspicious_floor_area".to_owned()));
        assert!(codes.contains(&"low_gigabit_coverage".to_owned()));
        assert!(codes.contains(&"address_degraded".to_owned()));
        assert!(codes.contains(&"media_degraded".to_owned()));
        assert!(codes.contains(&"contact_sheet_failed".to_owned()));
        assert!(codes.contains(&"high_crime_rate".to_owned()));
        assert!(codes.contains(&"active_manual_correction".to_owned()));
    }

    #[test]
    fn old_bundle_json_deserializes_without_new_fields() {
        let mut value = serde_json::to_value(test_bundle()).expect("serialize bundle");
        let object = value.as_object_mut().expect("bundle object");
        object.remove("flags");
        object
            .get_mut("media")
            .and_then(Value::as_object_mut)
            .expect("media object")
            .remove("contactSheet");
        object
            .get_mut("assessment")
            .and_then(Value::as_object_mut)
            .expect("assessment object")
            .remove("normalizedAssessment");

        let bundle = serde_json::from_value::<EvidenceBundle>(value).expect("deserialize bundle");

        assert!(bundle.flags.is_empty());
        assert!(bundle.media.contact_sheet.is_none());
        assert!(bundle.assessment.is_some());
        assert_eq!(
            bundle
                .assessment
                .as_ref()
                .and_then(|assessment| assessment.normalized_assessment.recommendation.as_deref()),
            None
        );
    }

    fn test_bundle() -> EvidenceBundle {
        EvidenceBundle {
            entity_id: "rightmove:170448131".to_owned(),
            rightmove_id: "170448131".to_owned(),
            url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
            generated_at: "2026-06-20T00:00:00Z".to_owned(),
            depth: InspectDepth::Standard,
            refresh: RefreshPolicy::Stale,
            sections: BTreeMap::from([
                (
                    "epc".to_owned(),
                    SectionState::ok("EPC checked", ConfidenceLevel::Probable),
                ),
                (
                    "media".to_owned(),
                    SectionState::ok("media extracted", ConfidenceLevel::Probable),
                ),
            ]),
            source_snapshots: Vec::new(),
            rightmove: RightmoveEvidence {
                rightmove_id: "170448131".to_owned(),
                url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
                page_status: "active".to_owned(),
                fetched_at: "2026-06-20T00:00:00Z".to_owned(),
                content_hash: "hash".to_owned(),
                title: Some("Two bedroom flat".to_owned()),
                address: Some("1 Example Street".to_owned()),
                postcode: Some("M1 1AA".to_owned()),
                display_price: Some("£1,250 pcm".to_owned()),
                price_pcm: Some(1250),
                bedrooms: Some(2),
                bathrooms: Some(1),
                property_type: Some("Flat".to_owned()),
                agent_name: None,
                agent_phone: None,
                latitude: None,
                longitude: None,
                pin_type: None,
                listed_date: None,
                available_date: None,
                deposit: None,
                description: DescriptionEvidence {
                    raw_html: String::new(),
                    text: String::new(),
                    key_features: Vec::new(),
                    normalized_text: String::new(),
                },
                media: Vec::new(),
            },
            address: AddressEvidence {
                candidates: Vec::new(),
                selected: Some(AddressCandidateEvidence {
                    source: "rightmove".to_owned(),
                    label: "1 Example Street".to_owned(),
                    postcode: Some("M1 1AA".to_owned()),
                    latitude: None,
                    longitude: None,
                    confidence: ConfidenceLevel::Exact,
                    raw: None,
                }),
                status: SectionStatus::Ok,
                confidence: ConfidenceLevel::Exact,
                warnings: Vec::new(),
            },
            facts: Vec::new(),
            broadband: None,
            epc: None,
            claims: Vec::new(),
            verifications: Vec::new(),
            media: MediaEvidence::default(),
            assessment: Some(AssessmentRecord::new(
                "rightmove:170448131".to_owned(),
                json!({"recommendation":"view"}),
                "2026-06-20T00:00:00Z".to_owned(),
            )),
            corrections: Vec::new(),
            next_actions: Vec::new(),
            flags: Vec::new(),
        }
    }
}
