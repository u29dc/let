#![forbid(unsafe_code)]

use serde_json::Value;

use crate::intelligence::types::NormalizedAssessment;

const RECOMMENDATION_VALUES: &[&str] = &["view", "consider", "hold", "watch", "pass", "benchmark"];
const CONFIDENCE_VALUES: &[&str] = &["high", "medium_high", "medium", "low"];

pub fn normalize_assessment(assessment: &Value) -> NormalizedAssessment {
    let mut normalized = NormalizedAssessment {
        recommendation: normalized_enum_text(assessment_text(assessment, &["recommendation"])),
        confidence: normalized_enum_text(assessment_text(assessment, &["confidence"])),
        summary: assessment_text(assessment, &["summary"]),
        score_adjustment: assessment_number(assessment, &["scoreAdjustment", "score_adjustment"]),
        judgment_score: assessment_number(assessment, &["judgmentScore", "judgment_score"]),
        judgment_rationale: assessment_text(
            assessment,
            &["judgmentRationale", "judgment_rationale"],
        ),
        positives: assessment_text_list(assessment, &["positives", "pros"]),
        risks: assessment_text_list(assessment, &["risks", "cons"]),
        next_actions: assessment_text_list(assessment, &["nextActions", "next_actions"]),
        tradeoffs: assessment_text_list(assessment, &["tradeoffs", "tradeOffs", "trade_offs"]),
        area_notes: assessment_text(assessment, &["areaNotes", "area_notes"]),
        commute_notes: assessment_text(assessment, &["commuteNotes", "commute_notes"]),
        family_fit: assessment_text(assessment, &["familyFit", "family_fit"]),
        evidence_gaps: assessment_text_list(assessment, &["evidenceGaps", "evidence_gaps"]),
        source: assessment_text(assessment, &["source"]),
        warnings: Vec::new(),
    };

    let recommendation = normalized.recommendation.clone();
    warn_unknown_enum(
        &mut normalized,
        "recommendation",
        recommendation.as_deref(),
        RECOMMENDATION_VALUES,
    );
    let confidence = normalized.confidence.clone();
    warn_unknown_enum(
        &mut normalized,
        "confidence",
        confidence.as_deref(),
        CONFIDENCE_VALUES,
    );
    warn_invalid_number(
        &mut normalized,
        assessment,
        &["scoreAdjustment", "score_adjustment"],
        "scoreAdjustment",
    );
    warn_invalid_number(
        &mut normalized,
        assessment,
        &["judgmentScore", "judgment_score"],
        "judgmentScore",
    );

    normalized
}

fn assessment_text(assessment: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = assessment.get(*key).and_then(value_to_text) {
            return Some(value);
        }
    }
    None
}

fn assessment_text_list(assessment: &Value, keys: &[&str]) -> Vec<String> {
    let Some(value) = keys.iter().find_map(|key| assessment.get(*key)) else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items.iter().filter_map(value_to_text).collect(),
        other => value_to_text(other).into_iter().collect(),
    }
}

fn assessment_number(assessment: &Value, keys: &[&str]) -> Option<f64> {
    let value = keys.iter().find_map(|key| assessment.get(*key))?;
    value_to_number(value)
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => non_empty_text(text),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn value_to_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite()),
        _ => None,
    }
}

fn normalized_enum_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| normalize_token(&text))
        .filter(|text| !text.is_empty())
}

fn warn_unknown_enum(
    assessment: &mut NormalizedAssessment,
    field: &str,
    value: Option<&str>,
    allowed: &[&str],
) {
    let Some(value) = value else {
        return;
    };
    if !allowed.iter().any(|allowed| allowed == &value) {
        assessment.warnings.push(format!(
            "`{field}` value `{value}` is outside the recommended assessment vocabulary"
        ));
    }
}

fn warn_invalid_number(
    normalized: &mut NormalizedAssessment,
    assessment: &Value,
    keys: &[&str],
    field: &str,
) {
    let Some(value) = keys.iter().find_map(|key| assessment.get(*key)) else {
        return;
    };
    if value_to_number(value).is_none() {
        normalized
            .warnings
            .push(format!("`{field}` must be a finite number"));
    }
}

fn normalize_token(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace([' ', '-'], "_")
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::normalize_assessment;

    #[test]
    fn normalizes_recommended_assessment_fields() {
        let normalized = normalize_assessment(&json!({
            "recommendation": "consider",
            "confidence": "medium-high",
            "summary": "Worth viewing if evidence checks out.",
            "score_adjustment": "7.5",
            "judgmentScore": 86,
            "judgment_rationale": "Strong family fit after visual review.",
            "positives": "station access",
            "risks": ["EPC mismatch"],
            "next_actions": ["call agent"],
            "tradeoffs": ["smaller garden"],
            "area_notes": "walkable",
            "commuteNotes": "direct train",
            "family_fit": "plausible",
            "evidenceGaps": "floor area",
            "source": "agent"
        }));

        assert_eq!(normalized.recommendation.as_deref(), Some("consider"));
        assert_eq!(normalized.confidence.as_deref(), Some("medium_high"));
        assert_eq!(normalized.score_adjustment, Some(7.5));
        assert_eq!(normalized.judgment_score, Some(86.0));
        assert_eq!(
            normalized.judgment_rationale.as_deref(),
            Some("Strong family fit after visual review.")
        );
        assert_eq!(normalized.positives, vec!["station access"]);
        assert_eq!(normalized.risks, vec!["EPC mismatch"]);
        assert_eq!(normalized.next_actions, vec!["call agent"]);
        assert_eq!(normalized.evidence_gaps, vec!["floor area"]);
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn accepts_hold_without_recommendation_warning() {
        let normalized = normalize_assessment(&json!({"recommendation": "hold"}));
        assert_eq!(normalized.recommendation.as_deref(), Some("hold"));
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn warns_on_invalid_numeric_judgment_fields() {
        let normalized = normalize_assessment(&json!({
            "scoreAdjustment": "high",
            "judgmentScore": {}
        }));
        assert_eq!(normalized.score_adjustment, None);
        assert_eq!(normalized.judgment_score, None);
        assert_eq!(normalized.warnings.len(), 2);
    }
}
