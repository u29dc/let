#![forbid(unsafe_code)]

pub mod engine;
pub mod judgment;
pub mod scorecard;
pub mod types;

pub use engine::compute_score;
pub use judgment::score_judgment;
pub use scorecard::{
    DEFAULT_SCORECARD_ID, ScorecardOverrideConfig, configured_scorecards, default_scorecard,
    resolve_scorecard, validate_scorecard_overrides,
};
pub use types::{
    CriterionScore, DomainScore, ScoreBand, ScoreCap, ScoreConfidence, ScoreJudgment,
    ScoreJudgmentPolicy, ScoreJudgmentSource, ScoreResult, ScoreSummary, ScorecardConfig,
    ScorecardRef, ScorecardWeights,
};
