#![forbid(unsafe_code)]

pub mod repository;
pub mod service;
pub mod types;

pub use repository::{IntelligenceDb, database_overview};
pub use service::{
    AssessGetParams, AssessSaveParams, CorrectionClearParams, CorrectionResponse,
    CorrectionSaveParams, EvidenceParams, InspectParams, VerifyParams, assess_get, assess_save,
    clear_correction, evidence, inspect, save_correction, verify,
};
pub use types::{
    AddressCandidateEvidence, AddressEvidence, AssessmentRecord, BroadbandEvidence, ClaimEvidence,
    ConfidenceLevel, CorrectionKind, CorrectionRecord, DescriptionEvidence, EpcEvidence,
    EvidenceBundle, EvidenceSection, FactEvidence, FactProvider, InspectDepth, MediaEvidence,
    MediaItemEvidence, RefreshPolicy, RightmoveEvidence, SectionState, SectionStatus, SourceRef,
    SourceSnapshotEvidence, VerificationEvidence, VerificationStatus,
};
