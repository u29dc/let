#![forbid(unsafe_code)]

use let_sdk::schema::listing::{
    FamilySuitability, Listing, ListingAssessment, ListingStatus, MaintenanceRating, Recommendation,
};
use let_sdk::{
    calculate_assessed_score, find_listing_by_id_from_db, load_listing_summaries,
    update_listing_assessment,
};
use serde_json::{Value, json};
use std::cmp::Ordering;

use crate::commands::{
    CommandError, CommandOutput, CommandResult, ErrorDetail, SharedArgs, to_camel_json,
};

#[derive(Debug, Clone)]
pub struct CandidatesParams {
    pub top: usize,
    pub region: Option<String>,
    pub min_score: Option<f64>,
}

pub fn candidates(shared: &SharedArgs, params: &CandidatesParams) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let summaries = load_listing_summaries(&db_path)?;
    let total = summaries.len();
    let assessed = summaries
        .iter()
        .filter(|listing| listing.has_assessment)
        .count();

    let mut shortlist = summaries
        .into_iter()
        .filter(|listing| {
            !listing.has_assessment && matches!(listing.status, ListingStatus::Active)
        })
        .collect::<Vec<_>>();

    if let Some(region) = &params.region {
        let target = region.to_lowercase();
        shortlist.retain(|listing| {
            listing
                .region
                .as_deref()
                .is_some_and(|value| value.to_lowercase().contains(&target))
        });
    }

    if let Some(min_score) = params.min_score {
        shortlist.retain(|listing| listing.score.is_some_and(|score| score >= min_score));
    }

    shortlist.sort_by(|a, b| {
        let left = a.score.unwrap_or(0.0);
        let right = b.score.unwrap_or(0.0);
        right.partial_cmp(&left).unwrap_or(Ordering::Equal)
    });

    if shortlist.len() > params.top {
        shortlist.truncate(params.top);
    }

    let candidates = shortlist
        .iter()
        .map(|listing| {
            json!({
                "id": listing.id,
                "portalId": listing.portal_rightmove,
                "address": listing.address,
                "score": listing.score,
                "region": listing.region,
                "url": listing.url,
            })
        })
        .collect::<Vec<_>>();

    Ok(CommandOutput::new(json!({
        "candidates": candidates,
        "total": total,
        "assessed": assessed,
        "remaining": total.saturating_sub(assessed),
    }))
    .with_count(shortlist.len())
    .with_total(total)
    .with_has_more(shortlist.len() < total))
}

pub fn context(shared: &SharedArgs, id: &str) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let Some(listing) = find_listing_by_id_from_db(&db_path, id)? else {
        return Err(CommandError::new(
            "NOT_FOUND",
            format!("listing not found: {id}"),
            "check id using `let view list`",
            1,
        ));
    };

    let listing_json = to_camel_json(&listing);
    let score_breakdown = score_breakdown_json(&listing);
    let media = resolve_media_paths(&listing, &paths.resolved.cache);
    let links = json!({
        "rightmove": listing.url,
        "googleMaps": listing.google_maps_url,
        "streetView": listing.google_maps_street_view_url,
        "epcSearch": listing.epc_search_url,
    });

    let data = json!({
        "listing": listing_json,
        "scoreBreakdown": score_breakdown,
        "assessmentSchema": assessment_schema(),
        "media": media,
        "links": links,
        "description": listing.description,
        "notes": listing.notes,
    });

    Ok(CommandOutput::new(data))
}

pub fn submit(shared: &SharedArgs, id: &str, assessment_raw: &str) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let parsed: Value = serde_json::from_str(assessment_raw).map_err(|error| {
        CommandError::runtime(
            "PARSE_ERROR",
            format!("invalid assessment json: {error}"),
            "check JSON syntax in assessment payload",
        )
    })?;

    let validation = validate_assessment_json(&parsed);
    if !validation.errors.is_empty() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "assessment payload failed validation",
            "fix the invalid fields in `error.details` and retry",
        )
        .with_details(validation.errors));
    }

    let assessment = validation
        .assessment
        .expect("validated assessment must exist");

    let Some(listing) = find_listing_by_id_from_db(&db_path, id)? else {
        return Err(CommandError::new(
            "NOT_FOUND",
            format!("listing not found: {id}"),
            "check id using `let view list`",
            1,
        ));
    };

    let algo_score = listing.scores.as_ref().map_or(0.0, |scores| scores.overall);
    let assessed_score = calculate_assessed_score(algo_score, &assessment);
    let assessed_at = let_sdk::utils::time::now_iso();

    update_listing_assessment(
        &db_path,
        &listing.id,
        &assessment,
        assessed_score,
        &assessed_at,
    )?;

    let data = json!({
        "id": listing.id,
        "assessedScore": assessed_score,
        "algoScore": algo_score,
        "scoreAdjustment": assessment.score_adjustment,
    });

    Ok(CommandOutput::new(data))
}

fn score_breakdown_json(listing: &Listing) -> Value {
    let Some(scores) = &listing.scores else {
        return Value::Null;
    };

    json!({
        "overall": scores.overall,
        "assessedScore": listing.assessed_score,
        "confidence": scores.confidence,
        "affordability": scores.affordability,
        "location": scores.location,
        "liveability": scores.liveability,
        "factors": to_camel_json(&scores.factors),
        "penalties": to_camel_json(&scores.penalties),
    })
}

fn resolve_media_paths(listing: &Listing, cache_root: &std::path::Path) -> Value {
    let listing_id = listing
        .portal_ids
        .rightmove
        .as_deref()
        .unwrap_or(&listing.id);
    let entry_dir = cache_root.join(listing_id);

    let images = listing
        .images
        .iter()
        .filter_map(|img| img.local.as_ref())
        .map(|local| cache_root.join(local))
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    let floorplan = listing
        .floorplan
        .local
        .as_ref()
        .map(|local| cache_root.join(local))
        .filter(|path| path.exists())
        .map(|path| path.display().to_string());

    let satellite = listing
        .map_views
        .satellite
        .local
        .as_ref()
        .map(|local| cache_root.join(local))
        .filter(|path| path.exists())
        .map(|path| path.display().to_string());

    let street = listing
        .map_views
        .street
        .local
        .as_ref()
        .map(|local| cache_root.join(local))
        .filter(|path| path.exists())
        .map(|path| path.display().to_string());

    json!({
        "images": images,
        "floorplan": floorplan,
        "satellite": satellite,
        "street": street,
        "cacheDir": if entry_dir.exists() {
            Some(entry_dir.display().to_string())
        } else {
            None
        },
    })
}

fn assessment_schema() -> Value {
    json!({
        "type": "object",
        "required": [
            "maintenance",
            "lightAndSpace",
            "photoAnalysis",
            "recommendation",
            "familySuitability",
            "reasoning",
            "scoreAdjustment"
        ],
        "properties": {
            "maintenance": {
                "type": "string",
                "enum": ["excellent", "good", "fair", "poor"]
            },
            "lightAndSpace": { "type": "string" },
            "photoAnalysis": { "type": "string" },
            "tradeoffs": { "type": "string" },
            "neighborhoodAnalysis": { "type": "string" },
            "recommendation": {
                "type": "string",
                "enum": ["strong-recommend", "recommend", "neutral", "avoid"]
            },
            "familySuitability": {
                "type": "string",
                "enum": ["excellent", "good", "fair", "poor"]
            },
            "reasoning": { "type": "string" },
            "scoreAdjustment": {
                "type": "number",
                "minimum": -30,
                "maximum": 30
            }
        }
    })
}

#[derive(Debug, Default)]
struct AssessmentValidation {
    assessment: Option<ListingAssessment>,
    errors: Vec<ErrorDetail>,
}

fn validate_assessment_json(value: &Value) -> AssessmentValidation {
    let mut result = AssessmentValidation::default();
    let Some(object) = value.as_object() else {
        result.errors.push(validation_error(
            "",
            "INVALID_TYPE",
            "expected object",
            Some("object".to_owned()),
            Some(value_kind(value)),
        ));
        return result;
    };

    let maintenance = parse_enum(
        object.get("maintenance"),
        "maintenance",
        &["excellent", "good", "fair", "poor"],
        &mut result.errors,
    );
    let light_and_space = parse_string(
        object.get("lightAndSpace"),
        "lightAndSpace",
        &mut result.errors,
    );
    let photo_analysis = parse_string(
        object.get("photoAnalysis"),
        "photoAnalysis",
        &mut result.errors,
    );
    let tradeoffs = parse_optional_string(object.get("tradeoffs"), "tradeoffs", &mut result.errors);
    let neighborhood_analysis = parse_optional_string(
        object.get("neighborhoodAnalysis"),
        "neighborhoodAnalysis",
        &mut result.errors,
    );
    let recommendation = parse_enum(
        object.get("recommendation"),
        "recommendation",
        &["strong-recommend", "recommend", "neutral", "avoid"],
        &mut result.errors,
    );
    let family_suitability = parse_enum(
        object.get("familySuitability"),
        "familySuitability",
        &["excellent", "good", "fair", "poor"],
        &mut result.errors,
    );
    let reasoning = parse_string(object.get("reasoning"), "reasoning", &mut result.errors);
    let score_adjustment =
        parse_score_adjustment(object.get("scoreAdjustment"), &mut result.errors);

    if result.errors.is_empty() {
        result.assessment = Some(ListingAssessment {
            maintenance: map_maintenance(&maintenance.expect("maintenance exists")),
            light_and_space: light_and_space.expect("lightAndSpace exists"),
            photo_analysis: photo_analysis.expect("photoAnalysis exists"),
            tradeoffs,
            neighborhood_analysis,
            recommendation: map_recommendation(&recommendation.expect("recommendation exists")),
            family_suitability: map_family(&family_suitability.expect("familySuitability exists")),
            reasoning: reasoning.expect("reasoning exists"),
            score_adjustment: score_adjustment.expect("score adjustment exists"),
        });
    }

    result
}

fn parse_enum(
    value: Option<&Value>,
    path: &str,
    allowed: &[&str],
    errors: &mut Vec<ErrorDetail>,
) -> Option<String> {
    match value.and_then(Value::as_str) {
        Some(raw) => {
            if allowed.contains(&raw) {
                Some(raw.to_owned())
            } else {
                errors.push(validation_error(
                    path,
                    "INVALID_VALUE",
                    format!("must be one of {}", allowed.join(", ")),
                    Some(format!("one of {}", allowed.join(", "))),
                    Some(format!("string `{raw}`")),
                ));
                None
            }
        }
        None => {
            errors.push(validation_error(
                path,
                "MISSING_FIELD",
                "required string",
                Some("string".to_owned()),
                value.map(value_kind),
            ));
            None
        }
    }
}

fn parse_string(
    value: Option<&Value>,
    path: &str,
    errors: &mut Vec<ErrorDetail>,
) -> Option<String> {
    match value.and_then(Value::as_str) {
        Some(raw) if !raw.trim().is_empty() => Some(raw.to_owned()),
        Some(_) => {
            errors.push(validation_error(
                path,
                "INVALID_VALUE",
                "must not be empty",
                Some("non-empty string".to_owned()),
                Some("empty string".to_owned()),
            ));
            None
        }
        None => {
            errors.push(validation_error(
                path,
                "MISSING_FIELD",
                "required string",
                Some("string".to_owned()),
                value.map(value_kind),
            ));
            None
        }
    }
}

fn parse_optional_string(
    value: Option<&Value>,
    path: &str,
    errors: &mut Vec<ErrorDetail>,
) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) => Some(raw.to_owned()),
        Some(_) => {
            errors.push(validation_error(
                path,
                "INVALID_TYPE",
                "must be a string",
                Some("string or null".to_owned()),
                value.map(value_kind),
            ));
            None
        }
    }
}

fn parse_score_adjustment(value: Option<&Value>, errors: &mut Vec<ErrorDetail>) -> Option<f64> {
    match value.and_then(Value::as_f64) {
        Some(raw) if (-30.0..=30.0).contains(&raw) => Some(raw),
        Some(raw) => {
            errors.push(validation_error(
                "scoreAdjustment",
                "OUT_OF_RANGE",
                "must be between -30 and 30",
                Some("-30..30".to_owned()),
                Some(raw.to_string()),
            ));
            None
        }
        None => {
            errors.push(validation_error(
                "scoreAdjustment",
                "MISSING_FIELD",
                "required number",
                Some("number".to_owned()),
                value.map(value_kind),
            ));
            None
        }
    }
}

fn validation_error(
    path: impl Into<String>,
    code: impl Into<String>,
    message: impl Into<String>,
    expected: Option<String>,
    actual: Option<String>,
) -> ErrorDetail {
    ErrorDetail {
        path: path.into(),
        code: code.into(),
        message: message.into(),
        expected,
        actual,
        suggestion: None,
    }
}

fn value_kind(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(_) => "boolean".to_owned(),
        Value::Number(_) => "number".to_owned(),
        Value::String(_) => "string".to_owned(),
        Value::Array(_) => "array".to_owned(),
        Value::Object(_) => "object".to_owned(),
    }
}

fn map_maintenance(value: &str) -> MaintenanceRating {
    match value {
        "excellent" => MaintenanceRating::Excellent,
        "good" => MaintenanceRating::Good,
        "fair" => MaintenanceRating::Fair,
        _ => MaintenanceRating::Poor,
    }
}

fn map_recommendation(value: &str) -> Recommendation {
    match value {
        "strong-recommend" => Recommendation::StrongRecommend,
        "recommend" => Recommendation::Recommend,
        "neutral" => Recommendation::Neutral,
        _ => Recommendation::Avoid,
    }
}

fn map_family(value: &str) -> FamilySuitability {
    match value {
        "excellent" => FamilySuitability::Excellent,
        "good" => FamilySuitability::Good,
        "fair" => FamilySuitability::Fair,
        _ => FamilySuitability::Poor,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_assessment_json;

    #[test]
    fn validate_assessment_accepts_valid_payload() {
        let value = json!({
            "maintenance": "good",
            "lightAndSpace": "bright",
            "photoAnalysis": "clean",
            "recommendation": "recommend",
            "familySuitability": "good",
            "reasoning": "close to school",
            "scoreAdjustment": 4,
        });

        let result = validate_assessment_json(&value);
        assert!(result.errors.is_empty());
        assert!(result.assessment.is_some());
    }

    #[test]
    fn validate_assessment_rejects_bad_payload() {
        let value = json!({
            "maintenance": "invalid",
            "scoreAdjustment": 99,
        });

        let result = validate_assessment_json(&value);
        assert!(!result.errors.is_empty());
        assert!(result.assessment.is_none());
    }
}
