#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssessmentInput {
    pub maintenance: Maintenance,
    pub light_and_space: String,
    pub photo_analysis: String,
    pub tradeoffs: Option<String>,
    pub neighborhood_analysis: Option<String>,
    pub recommendation: Recommendation,
    pub family_suitability: FamilySuitability,
    pub reasoning: String,
    pub score_adjustment: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Maintenance {
    Excellent,
    Good,
    Fair,
    Poor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Recommendation {
    StrongRecommend,
    Recommend,
    Neutral,
    Avoid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FamilySuitability {
    Excellent,
    Good,
    Fair,
    Poor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssessmentResult {
    pub listing_id: String,
    pub assessed_score: f64,
    pub assessed_at: String,
    pub assessment: AssessmentInput,
}
