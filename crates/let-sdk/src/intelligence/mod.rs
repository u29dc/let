#![forbid(unsafe_code)]

pub mod repository;
pub mod service;
pub mod types;

pub use repository::{IntelligenceDb, database_overview};
pub use service::{
    AreaPostcodeParams, AreaPostcodeResponse, AssessGetParams, AssessListParams,
    AssessListResponse, AssessSaveParams, CorrectionClearParams, CorrectionResponse,
    CorrectionSaveParams, EvidenceListParams, EvidenceListResponse, EvidenceParams, InspectParams,
    VerifyParams, area_postcode, assess_get, assess_list, assess_save, clear_correction, evidence,
    evidence_list, inspect, save_correction, verify,
};
pub use types::{
    AddressCandidateEvidence, AddressEvidence, AssessmentRecord, BroadbandEvidence, ClaimEvidence,
    ConfidenceLevel, CorrectionKind, CorrectionRecord, DescriptionEvidence, EpcEvidence,
    EvidenceBundle, EvidenceSection, FactEvidence, FactProvider, InspectDepth, ListingListFilters,
    MediaEvidence, MediaItemEvidence, RefreshPolicy, RightmoveEvidence, SectionState,
    SectionStatus, SourceRef, SourceSnapshotEvidence, StoredAssessmentSummary,
    StoredListingSummary, VerificationEvidence, VerificationStatus,
};
