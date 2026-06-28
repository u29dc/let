#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScorecardConfig {
    pub id: String,
    pub version: i64,
    pub label: String,
    pub weights: ScorecardWeights,
    pub thresholds: ScoreThresholds,
    pub caps: ScoreCaps,
    pub judgment: ScoreJudgmentPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScorecardWeights {
    pub value: f64,
    pub property: f64,
    pub location: f64,
    pub daily_life: f64,
    pub risk: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreThresholds {
    pub rent_good_pcm: f64,
    pub rent_high_pcm: f64,
    pub deposit_weeks_good: f64,
    pub deposit_weeks_high: f64,
    pub station_good_miles: f64,
    pub station_high_miles: f64,
    pub broadband_good_pct: f64,
    pub broadband_low_pct: f64,
    pub crime_low_per_1k: f64,
    pub crime_high_per_1k: f64,
    pub imd_good_decile: f64,
    pub min_photo_count: f64,
    pub target_bedrooms: f64,
    pub spacious_bedroom_sqm: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreCaps {
    pub partial_evidence_cap: f64,
    pub low_evidence_cap: f64,
    pub blocker_cap: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreJudgmentPolicy {
    pub enabled: bool,
    pub blend: f64,
    pub max_adjustment: f64,
}

impl Default for ScoreJudgmentPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            blend: 0.5,
            max_adjustment: 15.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScorecardRef {
    pub id: String,
    pub version: i64,
    pub label: String,
}

impl From<&ScorecardConfig> for ScorecardRef {
    fn from(scorecard: &ScorecardConfig) -> Self {
        Self {
            id: scorecard.id.clone(),
            version: scorecard.version,
            label: scorecard.label.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreResult {
    pub entity_id: String,
    pub rightmove_id: String,
    pub scorecard: ScorecardRef,
    pub computed_at: String,
    pub base_overall: f64,
    pub overall: f64,
    pub band: ScoreBand,
    pub confidence: ScoreConfidence,
    pub judgment: ScoreJudgment,
    pub domains: Vec<DomainScore>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<ScoreCap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<ScoreBlocker>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DomainScore {
    pub domain: String,
    pub label: String,
    pub score: f64,
    pub weight: f64,
    pub weighted_points: f64,
    pub confidence: ScoreConfidence,
    pub summary: String,
    pub criteria: Vec<CriterionScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CriterionScore {
    pub key: String,
    pub label: String,
    pub score: f64,
    pub weight: f64,
    pub confidence: ScoreConfidence,
    pub evidence: String,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreCap {
    pub cap: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBlocker {
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreJudgment {
    pub source: ScoreJudgmentSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judgment_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_adjustment: Option<f64>,
    pub applied_adjustment: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl ScoreJudgment {
    pub fn none() -> Self {
        Self {
            source: ScoreJudgmentSource::None,
            judgment_score: None,
            requested_adjustment: None,
            applied_adjustment: 0.0,
            rationale: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScoreJudgmentSource {
    None,
    ScoreAdjustment,
    JudgmentScore,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreSummary {
    pub id: String,
    pub entity_id: String,
    pub rightmove_id: String,
    pub scorecard_id: String,
    pub scorecard_version: i64,
    pub base_overall: f64,
    pub overall: f64,
    pub judgment_adjustment: f64,
    pub judgment_score: Option<f64>,
    pub judgment_rationale: Option<String>,
    pub band: ScoreBand,
    pub confidence: ScoreConfidence,
    pub computed_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum ScoreConfidence {
    High,
    Medium,
    Low,
}

impl ScoreConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScoreBand {
    Excellent,
    Strong,
    Viable,
    Weak,
    Reject,
}

impl ScoreBand {
    pub fn from_score(score: f64) -> Self {
        if score >= 85.0 {
            Self::Excellent
        } else if score >= 72.0 {
            Self::Strong
        } else if score >= 58.0 {
            Self::Viable
        } else if score >= 42.0 {
            Self::Weak
        } else {
            Self::Reject
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Excellent => "excellent",
            Self::Strong => "strong",
            Self::Viable => "viable",
            Self::Weak => "weak",
            Self::Reject => "reject",
        }
    }
}
