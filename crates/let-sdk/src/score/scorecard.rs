#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, LetError, Result};
use crate::score::types::{
    ScoreCaps, ScoreJudgmentPolicy, ScoreThresholds, ScorecardConfig, ScorecardWeights,
};

pub const DEFAULT_SCORECARD_ID: &str = "default";
const DEFAULT_SCORECARD_VERSION: i64 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScorecardOverrideConfig {
    pub label: Option<String>,
    pub version: Option<i64>,
    pub weights: Option<ScorecardWeightsOverride>,
    pub thresholds: Option<ScoreThresholdsOverride>,
    pub caps: Option<ScoreCapsOverride>,
    pub judgment: Option<ScoreJudgmentPolicyOverride>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScorecardWeightsOverride {
    pub value: Option<f64>,
    pub property: Option<f64>,
    pub location: Option<f64>,
    pub daily_life: Option<f64>,
    pub risk: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreThresholdsOverride {
    pub rent_good_pcm: Option<f64>,
    pub rent_high_pcm: Option<f64>,
    pub deposit_weeks_good: Option<f64>,
    pub deposit_weeks_high: Option<f64>,
    pub station_good_miles: Option<f64>,
    pub station_high_miles: Option<f64>,
    pub broadband_good_pct: Option<f64>,
    pub broadband_low_pct: Option<f64>,
    pub crime_low_per_1k: Option<f64>,
    pub crime_high_per_1k: Option<f64>,
    pub imd_good_decile: Option<f64>,
    pub min_photo_count: Option<f64>,
    pub target_bedrooms: Option<f64>,
    pub spacious_bedroom_sqm: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreCapsOverride {
    pub partial_evidence_cap: Option<f64>,
    pub low_evidence_cap: Option<f64>,
    pub blocker_cap: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoreJudgmentPolicyOverride {
    pub enabled: Option<bool>,
    pub blend: Option<f64>,
    pub max_adjustment: Option<f64>,
}

pub fn default_scorecard() -> ScorecardConfig {
    ScorecardConfig {
        id: DEFAULT_SCORECARD_ID.to_owned(),
        version: DEFAULT_SCORECARD_VERSION,
        label: "Baseline rental scorecard".to_owned(),
        weights: ScorecardWeights {
            value: 0.20,
            property: 0.25,
            location: 0.20,
            daily_life: 0.20,
            risk: 0.15,
        },
        thresholds: ScoreThresholds {
            rent_good_pcm: 1_450.0,
            rent_high_pcm: 2_200.0,
            deposit_weeks_good: 5.0,
            deposit_weeks_high: 6.0,
            station_good_miles: 0.8,
            station_high_miles: 2.5,
            broadband_good_pct: 80.0,
            broadband_low_pct: 20.0,
            crime_low_per_1k: 40.0,
            crime_high_per_1k: 140.0,
            imd_good_decile: 8.0,
            min_photo_count: 6.0,
            target_bedrooms: 2.0,
            spacious_bedroom_sqm: 32.0,
        },
        caps: ScoreCaps {
            partial_evidence_cap: 78.0,
            low_evidence_cap: 62.0,
            blocker_cap: 45.0,
        },
        judgment: ScoreJudgmentPolicy::default(),
    }
}

pub fn configured_scorecards(
    overrides: &BTreeMap<String, ScorecardOverrideConfig>,
) -> Result<Vec<ScorecardConfig>> {
    let mut scorecards = Vec::new();
    let default_override = overrides.get(DEFAULT_SCORECARD_ID);
    scorecards.push(apply_override(DEFAULT_SCORECARD_ID, default_override)?);

    for (id, override_config) in overrides {
        if id == DEFAULT_SCORECARD_ID {
            continue;
        }
        scorecards.push(apply_override(id, Some(override_config))?);
    }
    scorecards.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(scorecards)
}

pub fn resolve_scorecard(
    id: &str,
    overrides: &BTreeMap<String, ScorecardOverrideConfig>,
) -> Result<ScorecardConfig> {
    let id = normalized_scorecard_id(id);
    if id == DEFAULT_SCORECARD_ID {
        return apply_override(DEFAULT_SCORECARD_ID, overrides.get(DEFAULT_SCORECARD_ID));
    }
    let Some(override_config) = overrides.get(id) else {
        return Err(LetError::new(
            ErrorCode::InvalidInput,
            format!("unknown scorecard `{id}`"),
            "run `let scorecards list` and pass one of the configured scorecard ids",
        ));
    };
    apply_override(id, Some(override_config))
}

pub fn validate_scorecard_overrides(
    overrides: &BTreeMap<String, ScorecardOverrideConfig>,
) -> Result<()> {
    for scorecard in configured_scorecards(overrides)? {
        validate_scorecard(&scorecard)?;
    }
    Ok(())
}

fn apply_override(
    id: &str,
    override_config: Option<&ScorecardOverrideConfig>,
) -> Result<ScorecardConfig> {
    validate_scorecard_id(id)?;
    let mut scorecard = default_scorecard();
    scorecard.id = id.to_owned();
    if id != DEFAULT_SCORECARD_ID {
        scorecard.label = id.replace(['-', '_'], " ");
    }

    if let Some(override_config) = override_config {
        if let Some(label) = override_config
            .label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            scorecard.label = label.to_owned();
        }
        if let Some(version) = override_config.version {
            scorecard.version = version;
        }
        if let Some(weights) = override_config.weights {
            apply_weight_overrides(&mut scorecard.weights, weights);
        }
        if let Some(thresholds) = override_config.thresholds {
            apply_threshold_overrides(&mut scorecard.thresholds, thresholds);
        }
        if let Some(caps) = override_config.caps {
            apply_cap_overrides(&mut scorecard.caps, caps);
        }
        if let Some(judgment) = override_config.judgment {
            apply_judgment_overrides(&mut scorecard.judgment, judgment);
        }
    }

    validate_scorecard(&scorecard)?;
    Ok(scorecard)
}

fn apply_weight_overrides(weights: &mut ScorecardWeights, overrides: ScorecardWeightsOverride) {
    if let Some(value) = overrides.value {
        weights.value = value;
    }
    if let Some(value) = overrides.property {
        weights.property = value;
    }
    if let Some(value) = overrides.location {
        weights.location = value;
    }
    if let Some(value) = overrides.daily_life {
        weights.daily_life = value;
    }
    if let Some(value) = overrides.risk {
        weights.risk = value;
    }
}

fn apply_threshold_overrides(thresholds: &mut ScoreThresholds, overrides: ScoreThresholdsOverride) {
    if let Some(value) = overrides.rent_good_pcm {
        thresholds.rent_good_pcm = value;
    }
    if let Some(value) = overrides.rent_high_pcm {
        thresholds.rent_high_pcm = value;
    }
    if let Some(value) = overrides.deposit_weeks_good {
        thresholds.deposit_weeks_good = value;
    }
    if let Some(value) = overrides.deposit_weeks_high {
        thresholds.deposit_weeks_high = value;
    }
    if let Some(value) = overrides.station_good_miles {
        thresholds.station_good_miles = value;
    }
    if let Some(value) = overrides.station_high_miles {
        thresholds.station_high_miles = value;
    }
    if let Some(value) = overrides.broadband_good_pct {
        thresholds.broadband_good_pct = value;
    }
    if let Some(value) = overrides.broadband_low_pct {
        thresholds.broadband_low_pct = value;
    }
    if let Some(value) = overrides.crime_low_per_1k {
        thresholds.crime_low_per_1k = value;
    }
    if let Some(value) = overrides.crime_high_per_1k {
        thresholds.crime_high_per_1k = value;
    }
    if let Some(value) = overrides.imd_good_decile {
        thresholds.imd_good_decile = value;
    }
    if let Some(value) = overrides.min_photo_count {
        thresholds.min_photo_count = value;
    }
    if let Some(value) = overrides.target_bedrooms {
        thresholds.target_bedrooms = value;
    }
    if let Some(value) = overrides.spacious_bedroom_sqm {
        thresholds.spacious_bedroom_sqm = value;
    }
}

fn apply_cap_overrides(caps: &mut ScoreCaps, overrides: ScoreCapsOverride) {
    if let Some(value) = overrides.partial_evidence_cap {
        caps.partial_evidence_cap = value;
    }
    if let Some(value) = overrides.low_evidence_cap {
        caps.low_evidence_cap = value;
    }
    if let Some(value) = overrides.blocker_cap {
        caps.blocker_cap = value;
    }
}

fn apply_judgment_overrides(
    judgment: &mut ScoreJudgmentPolicy,
    overrides: ScoreJudgmentPolicyOverride,
) {
    if let Some(value) = overrides.enabled {
        judgment.enabled = value;
    }
    if let Some(value) = overrides.blend {
        judgment.blend = value;
    }
    if let Some(value) = overrides.max_adjustment {
        judgment.max_adjustment = value;
    }
}

fn validate_scorecard(scorecard: &ScorecardConfig) -> Result<()> {
    if scorecard.version <= 0 {
        return invalid_scorecard(scorecard, "version must be a positive integer");
    }
    validate_weight_set(scorecard)?;
    validate_thresholds(scorecard)?;
    validate_caps(scorecard)?;
    validate_judgment(scorecard)
}

fn validate_weight_set(scorecard: &ScorecardConfig) -> Result<()> {
    let weights = [
        ("value", scorecard.weights.value),
        ("property", scorecard.weights.property),
        ("location", scorecard.weights.location),
        ("dailyLife", scorecard.weights.daily_life),
        ("risk", scorecard.weights.risk),
    ];
    for (name, value) in weights {
        if !value.is_finite() || value < 0.0 {
            return invalid_scorecard(
                scorecard,
                format!("weight `{name}` must be a finite non-negative number"),
            );
        }
    }
    let total: f64 = weights.iter().map(|(_, value)| value).sum();
    if total <= 0.0 {
        return invalid_scorecard(scorecard, "at least one scorecard weight must be positive");
    }
    Ok(())
}

fn validate_thresholds(scorecard: &ScorecardConfig) -> Result<()> {
    let thresholds = scorecard.thresholds;
    for (name, value) in [
        ("rentGoodPcm", thresholds.rent_good_pcm),
        ("rentHighPcm", thresholds.rent_high_pcm),
        ("depositWeeksGood", thresholds.deposit_weeks_good),
        ("depositWeeksHigh", thresholds.deposit_weeks_high),
        ("stationGoodMiles", thresholds.station_good_miles),
        ("stationHighMiles", thresholds.station_high_miles),
        ("broadbandGoodPct", thresholds.broadband_good_pct),
        ("broadbandLowPct", thresholds.broadband_low_pct),
        ("crimeLowPer1k", thresholds.crime_low_per_1k),
        ("crimeHighPer1k", thresholds.crime_high_per_1k),
        ("imdGoodDecile", thresholds.imd_good_decile),
        ("minPhotoCount", thresholds.min_photo_count),
        ("targetBedrooms", thresholds.target_bedrooms),
        ("spaciousBedroomSqm", thresholds.spacious_bedroom_sqm),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return invalid_scorecard(
                scorecard,
                format!("threshold `{name}` must be a finite positive number"),
            );
        }
    }

    if thresholds.rent_good_pcm > thresholds.rent_high_pcm {
        return invalid_scorecard(
            scorecard,
            "rentGoodPcm must be less than or equal to rentHighPcm",
        );
    }
    if thresholds.deposit_weeks_good > thresholds.deposit_weeks_high {
        return invalid_scorecard(
            scorecard,
            "depositWeeksGood must be less than or equal to depositWeeksHigh",
        );
    }
    if thresholds.station_good_miles > thresholds.station_high_miles {
        return invalid_scorecard(
            scorecard,
            "stationGoodMiles must be less than or equal to stationHighMiles",
        );
    }
    if thresholds.broadband_low_pct > thresholds.broadband_good_pct {
        return invalid_scorecard(
            scorecard,
            "broadbandLowPct must be less than or equal to broadbandGoodPct",
        );
    }
    if thresholds.crime_low_per_1k > thresholds.crime_high_per_1k {
        return invalid_scorecard(
            scorecard,
            "crimeLowPer1k must be less than or equal to crimeHighPer1k",
        );
    }
    Ok(())
}

fn validate_caps(scorecard: &ScorecardConfig) -> Result<()> {
    for (name, value) in [
        ("partialEvidenceCap", scorecard.caps.partial_evidence_cap),
        ("lowEvidenceCap", scorecard.caps.low_evidence_cap),
        ("blockerCap", scorecard.caps.blocker_cap),
    ] {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return invalid_scorecard(
                scorecard,
                format!("cap `{name}` must be a finite value from 0 to 100"),
            );
        }
    }
    if scorecard.caps.low_evidence_cap > scorecard.caps.partial_evidence_cap {
        return invalid_scorecard(
            scorecard,
            "lowEvidenceCap must be less than or equal to partialEvidenceCap",
        );
    }
    if scorecard.caps.blocker_cap > scorecard.caps.low_evidence_cap {
        return invalid_scorecard(
            scorecard,
            "blockerCap must be less than or equal to lowEvidenceCap",
        );
    }
    Ok(())
}

fn validate_judgment(scorecard: &ScorecardConfig) -> Result<()> {
    let judgment = scorecard.judgment;
    if !judgment.blend.is_finite() || !(0.0..=1.0).contains(&judgment.blend) {
        return invalid_scorecard(
            scorecard,
            "judgment blend must be a finite value from 0 to 1",
        );
    }
    if !judgment.max_adjustment.is_finite() || !(0.0..=100.0).contains(&judgment.max_adjustment) {
        return invalid_scorecard(
            scorecard,
            "judgment maxAdjustment must be a finite value from 0 to 100",
        );
    }
    Ok(())
}

fn validate_scorecard_id(id: &str) -> Result<()> {
    let valid = (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        && id
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric());
    if valid {
        return Ok(());
    }
    Err(LetError::new(
        ErrorCode::InvalidInput,
        format!("invalid scorecard id `{id}`"),
        "use 1-64 ASCII letters, digits, hyphens, or underscores",
    ))
}

fn normalized_scorecard_id(id: &str) -> &str {
    let id = id.trim();
    if id.is_empty() {
        DEFAULT_SCORECARD_ID
    } else {
        id
    }
}

fn invalid_scorecard(scorecard: &ScorecardConfig, message: impl Into<String>) -> Result<()> {
    Err(LetError::new(
        ErrorCode::InvalidInput,
        format!("invalid scorecard `{}`: {}", scorecard.id, message.into()),
        "fix the [scorecards.<id>] config values or remove the override",
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        DEFAULT_SCORECARD_ID, ScoreJudgmentPolicyOverride, ScorecardOverrideConfig,
        ScorecardWeightsOverride, configured_scorecards, resolve_scorecard,
    };

    #[test]
    fn default_scorecard_can_be_partially_overridden() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            DEFAULT_SCORECARD_ID.to_owned(),
            ScorecardOverrideConfig {
                label: Some("Personal baseline".to_owned()),
                weights: Some(ScorecardWeightsOverride {
                    location: Some(0.35),
                    ..ScorecardWeightsOverride::default()
                }),
                judgment: Some(ScoreJudgmentPolicyOverride {
                    blend: Some(0.7),
                    max_adjustment: Some(12.0),
                    ..ScoreJudgmentPolicyOverride::default()
                }),
                ..ScorecardOverrideConfig::default()
            },
        );

        let scorecard = resolve_scorecard(DEFAULT_SCORECARD_ID, &overrides)
            .expect("resolve overridden default");
        assert_eq!(scorecard.label, "Personal baseline");
        assert_eq!(scorecard.weights.location, 0.35);
        assert_eq!(scorecard.judgment.blend, 0.7);
        assert_eq!(scorecard.judgment.max_adjustment, 12.0);
    }

    #[test]
    fn configured_scorecards_include_default_once() {
        let overrides = BTreeMap::new();
        let scorecards = configured_scorecards(&overrides).expect("list scorecards");
        assert_eq!(scorecards.len(), 1);
        assert_eq!(scorecards[0].id, DEFAULT_SCORECARD_ID);
        assert!(scorecards[0].judgment.enabled);
        assert_eq!(scorecards[0].judgment.blend, 0.5);
    }

    #[test]
    fn invalid_judgment_policy_is_rejected() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            DEFAULT_SCORECARD_ID.to_owned(),
            ScorecardOverrideConfig {
                judgment: Some(ScoreJudgmentPolicyOverride {
                    blend: Some(1.5),
                    ..ScoreJudgmentPolicyOverride::default()
                }),
                ..ScorecardOverrideConfig::default()
            },
        );
        assert!(resolve_scorecard(DEFAULT_SCORECARD_ID, &overrides).is_err());

        let mut overrides = BTreeMap::new();
        overrides.insert(
            DEFAULT_SCORECARD_ID.to_owned(),
            ScorecardOverrideConfig {
                judgment: Some(ScoreJudgmentPolicyOverride {
                    max_adjustment: Some(-1.0),
                    ..ScoreJudgmentPolicyOverride::default()
                }),
                ..ScorecardOverrideConfig::default()
            },
        );
        assert!(resolve_scorecard(DEFAULT_SCORECARD_ID, &overrides).is_err());
    }
}
