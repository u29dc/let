#![forbid(unsafe_code)]

use crate::intelligence::types::NormalizedAssessment;
use crate::score::types::{ScoreJudgment, ScoreJudgmentPolicy, ScoreJudgmentSource};

pub fn score_judgment(
    base_overall: f64,
    assessment: Option<&NormalizedAssessment>,
    policy: ScoreJudgmentPolicy,
) -> ScoreJudgment {
    let Some(assessment) = assessment else {
        return ScoreJudgment::none();
    };
    if !policy.enabled {
        return ScoreJudgment::none();
    }

    let mut warnings = assessment
        .warnings
        .iter()
        .filter(|warning| warning.contains("scoreAdjustment") || warning.contains("judgmentScore"))
        .cloned()
        .collect::<Vec<_>>();

    let has_score_adjustment = assessment.score_adjustment.is_some();
    let has_judgment_score = assessment.judgment_score.is_some();
    if has_score_adjustment && has_judgment_score {
        warnings.push(
            "`scoreAdjustment` and `judgmentScore` are both present; using `scoreAdjustment`"
                .to_owned(),
        );
    }

    if let Some(requested) = assessment.score_adjustment {
        let applied = clamp_adjustment(requested, policy.max_adjustment, &mut warnings);
        return ScoreJudgment {
            source: ScoreJudgmentSource::ScoreAdjustment,
            judgment_score: None,
            requested_adjustment: Some(round_score(requested)),
            applied_adjustment: round_score(applied),
            rationale: assessment.judgment_rationale.clone(),
            warnings,
        };
    }

    if let Some(raw_score) = assessment.judgment_score {
        let judgment_score = clamp_judgment_score(raw_score, &mut warnings);
        let requested = (judgment_score - base_overall) * policy.blend;
        let applied = clamp_adjustment(requested, policy.max_adjustment, &mut warnings);
        return ScoreJudgment {
            source: ScoreJudgmentSource::JudgmentScore,
            judgment_score: Some(round_score(judgment_score)),
            requested_adjustment: Some(round_score(requested)),
            applied_adjustment: round_score(applied),
            rationale: assessment.judgment_rationale.clone(),
            warnings,
        };
    }

    ScoreJudgment {
        warnings,
        ..ScoreJudgment::none()
    }
}

fn clamp_adjustment(value: f64, max_adjustment: f64, warnings: &mut Vec<String>) -> f64 {
    let clamped = value.clamp(-max_adjustment, max_adjustment);
    if clamped != value {
        warnings.push(format!(
            "judgment adjustment was clamped from {:.1} to {:.1}",
            round_score(value),
            round_score(clamped)
        ));
    }
    clamped
}

fn clamp_judgment_score(value: f64, warnings: &mut Vec<String>) -> f64 {
    let clamped = value.clamp(0.0, 100.0);
    if clamped != value {
        warnings.push(format!(
            "`judgmentScore` was clamped from {:.1} to {:.1}",
            round_score(value),
            round_score(clamped)
        ));
    }
    clamped
}

fn round_score(value: f64) -> f64 {
    (value.clamp(-100.0, 100.0) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use crate::intelligence::types::NormalizedAssessment;
    use crate::score::types::{ScoreJudgmentPolicy, ScoreJudgmentSource};

    use super::score_judgment;

    #[test]
    fn explicit_positive_adjustment_lifts_score() {
        let assessment = NormalizedAssessment {
            score_adjustment: Some(8.0),
            judgment_rationale: Some("Strong family fit.".to_owned()),
            ..NormalizedAssessment::default()
        };
        let judgment = score_judgment(75.0, Some(&assessment), ScoreJudgmentPolicy::default());

        assert_eq!(judgment.source, ScoreJudgmentSource::ScoreAdjustment);
        assert_eq!(judgment.applied_adjustment, 8.0);
        assert_eq!(judgment.rationale.as_deref(), Some("Strong family fit."));
    }

    #[test]
    fn explicit_negative_adjustment_lowers_score() {
        let assessment = NormalizedAssessment {
            score_adjustment: Some(-6.0),
            ..NormalizedAssessment::default()
        };
        let judgment = score_judgment(82.0, Some(&assessment), ScoreJudgmentPolicy::default());

        assert_eq!(judgment.applied_adjustment, -6.0);
    }

    #[test]
    fn judgment_score_blends_against_base_score() {
        let assessment = NormalizedAssessment {
            judgment_score: Some(95.0),
            ..NormalizedAssessment::default()
        };
        let judgment = score_judgment(75.0, Some(&assessment), ScoreJudgmentPolicy::default());

        assert_eq!(judgment.source, ScoreJudgmentSource::JudgmentScore);
        assert_eq!(judgment.requested_adjustment, Some(10.0));
        assert_eq!(judgment.applied_adjustment, 10.0);
    }

    #[test]
    fn explicit_adjustment_wins_over_judgment_score() {
        let assessment = NormalizedAssessment {
            score_adjustment: Some(4.0),
            judgment_score: Some(95.0),
            ..NormalizedAssessment::default()
        };
        let judgment = score_judgment(75.0, Some(&assessment), ScoreJudgmentPolicy::default());

        assert_eq!(judgment.source, ScoreJudgmentSource::ScoreAdjustment);
        assert_eq!(judgment.applied_adjustment, 4.0);
        assert_eq!(judgment.warnings.len(), 1);
    }

    #[test]
    fn missing_or_disabled_judgment_applies_no_adjustment() {
        assert_eq!(
            score_judgment(75.0, None, ScoreJudgmentPolicy::default()).applied_adjustment,
            0.0
        );

        let assessment = NormalizedAssessment {
            score_adjustment: Some(8.0),
            ..NormalizedAssessment::default()
        };
        let policy = ScoreJudgmentPolicy {
            enabled: false,
            ..ScoreJudgmentPolicy::default()
        };
        assert_eq!(
            score_judgment(75.0, Some(&assessment), policy).applied_adjustment,
            0.0
        );
    }

    #[test]
    fn out_of_range_values_are_clamped_and_warned() {
        let assessment = NormalizedAssessment {
            score_adjustment: Some(30.0),
            ..NormalizedAssessment::default()
        };
        let judgment = score_judgment(75.0, Some(&assessment), ScoreJudgmentPolicy::default());

        assert_eq!(judgment.applied_adjustment, 15.0);
        assert_eq!(judgment.warnings.len(), 1);

        let assessment = NormalizedAssessment {
            judgment_score: Some(120.0),
            ..NormalizedAssessment::default()
        };
        let judgment = score_judgment(75.0, Some(&assessment), ScoreJudgmentPolicy::default());

        assert_eq!(judgment.judgment_score, Some(100.0));
        assert!(!judgment.warnings.is_empty());
    }
}
