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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentRecord {
    pub entity_id: String,
    pub assessment: Value,
    pub saved_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListingListFilters {
    pub recommendation: Option<String>,
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
}
