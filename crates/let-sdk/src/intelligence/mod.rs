#![forbid(unsafe_code)]

pub mod assessment;
pub mod repository;
mod schema;
pub mod service;
pub mod types;

pub use assessment::normalize_assessment;
pub use repository::{IntelligenceDb, database_overview};
pub use service::{
    AreaPostcodeParams, AreaPostcodeResponse, AssessGetParams, AssessListParams,
    AssessListResponse, AssessSaveParams, CorrectionClearParams, CorrectionResponse,
    CorrectionSaveParams, EvidenceListParams, EvidenceListResponse, EvidenceParams, InspectParams,
    ScoreComputeParams, ScoreComputeResponse, ScoreGetParams, ScoreListParams, ScoreListResponse,
    ScorecardsParams, ScorecardsResponse, VerifyParams, area_postcode, assess_get, assess_list,
    assess_save, clear_correction, evidence, evidence_list, inspect, save_correction,
    score_compute, score_get, score_list, scorecards, verify,
};
pub use types::{
    AddressCandidateEvidence, AddressEvidence, AssessmentRecord, BroadbandEvidence, ClaimEvidence,
    ConfidenceLevel, ContactSheetEvidence, CorrectionKind, CorrectionRecord, DescriptionEvidence,
    EpcEvidence, EvidenceBundle, EvidenceSection, FactEvidence, FactProvider, InspectDepth,
    ListingListFilters, MediaEvidence, MediaItemEvidence, NearestStationEvidence,
    NormalizedAssessment, RefreshPolicy, RightmoveEvidence, SectionState, SectionStatus, SourceRef,
    SourceSnapshotEvidence, StoredAssessmentSummary, StoredListingSummary, VerificationEvidence,
    VerificationStatus,
};
