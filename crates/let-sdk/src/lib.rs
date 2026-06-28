#![forbid(unsafe_code)]

pub mod config;
pub mod context;
pub mod errors;
pub mod intelligence;
pub mod paths;
pub mod pipeline;
pub mod schema;
pub mod score;
pub mod sources;
pub mod utils;

pub use errors::{ErrorCode, LetError, Result};
pub use intelligence::{
    AreaPostcodeParams, AssessGetParams, AssessListParams, AssessSaveParams, EvidenceListParams,
    EvidenceParams, InspectParams, IntelligenceDb, ListingListFilters, RefreshPolicy,
    ScoreComputeParams, ScoreGetParams, ScoreListParams, ScorecardsParams, VerifyParams,
    area_postcode, assess_get, assess_list, assess_save, database_overview, evidence,
    evidence_list, inspect, score_compute, score_get, score_list, scorecards, verify,
};
pub use pipeline::enrich::{
    AreaPostcodeSnapshot, BroadbandProfile, EnrichmentMode, ListingEnrichmentReport,
    PostcodeCoordinates, SourceEnricher,
};
pub use pipeline::geocode::{GeocodeSource, GeocodedCoordinates, mapbox_forward_geocode};
pub use score::{
    ScoreJudgment, ScoreJudgmentPolicy, ScoreJudgmentSource, ScoreResult, ScoreSummary,
    ScorecardConfig, compute_score,
};
