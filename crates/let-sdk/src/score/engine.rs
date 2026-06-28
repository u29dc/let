#![forbid(unsafe_code)]

use crate::intelligence::types::{EvidenceBundle, SectionStatus, VerificationStatus};
use crate::score::judgment::score_judgment;
use crate::score::types::{
    CriterionScore, DomainScore, ScoreBand, ScoreBlocker, ScoreCap, ScoreConfidence, ScoreResult,
    ScorecardConfig, ScorecardRef,
};
use crate::utils::time::now_iso;

pub fn compute_score(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> ScoreResult {
    let mut domains = vec![
        value_domain(bundle, scorecard),
        property_domain(bundle, scorecard),
        location_domain(bundle, scorecard),
        daily_life_domain(bundle, scorecard),
        risk_domain(bundle, scorecard),
    ];
    let total_weight: f64 = domains.iter().map(|domain| domain.weight).sum();
    let mut base_overall = if total_weight > 0.0 {
        domains
            .iter()
            .map(|domain| domain.score * domain.weight / total_weight)
            .sum::<f64>()
    } else {
        0.0
    };
    for domain in &mut domains {
        domain.weight /= total_weight.max(1.0);
        domain.weighted_points = domain.score * domain.weight;
    }

    let blockers = blockers(bundle);
    let caps = caps(bundle, scorecard, blockers.is_empty());
    for cap in &caps {
        base_overall = base_overall.min(cap.cap);
    }
    base_overall = rounded_score(base_overall);
    let judgment = score_judgment(
        base_overall,
        bundle
            .assessment
            .as_ref()
            .map(|record| &record.normalized_assessment),
        scorecard.judgment,
    );
    let mut overall = rounded_score(base_overall + judgment.applied_adjustment);
    if !blockers.is_empty() {
        overall = rounded_score(overall.min(scorecard.caps.blocker_cap));
    }
    let confidence = result_confidence(&domains, &caps);
    let band = if blockers.is_empty() {
        ScoreBand::from_score(overall)
    } else {
        ScoreBand::Reject
    };

    ScoreResult {
        entity_id: bundle.entity_id.clone(),
        rightmove_id: bundle.rightmove_id.clone(),
        scorecard: ScorecardRef::from(scorecard),
        computed_at: now_iso(),
        base_overall,
        overall,
        band,
        confidence,
        judgment,
        summary: score_summary(base_overall, overall, band, confidence, &domains),
        domains,
        caps,
        blockers,
        next_actions: score_next_actions(bundle),
    }
}

fn value_domain(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> DomainScore {
    domain(
        "value",
        "Value",
        scorecard.weights.value,
        vec![
            rent_criterion(bundle, scorecard),
            deposit_criterion(bundle, scorecard),
            running_cost_criterion(bundle),
        ],
    )
}

fn property_domain(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> DomainScore {
    domain(
        "property",
        "Property",
        scorecard.weights.property,
        vec![
            bedrooms_criterion(bundle, scorecard),
            floor_area_criterion(bundle, scorecard),
            media_criterion(bundle, scorecard),
            features_criterion(bundle),
        ],
    )
}

fn location_domain(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> DomainScore {
    domain(
        "location",
        "Location",
        scorecard.weights.location,
        vec![
            imd_criterion(bundle, scorecard),
            crime_criterion(bundle, scorecard),
            flood_criterion(bundle),
        ],
    )
}

fn daily_life_domain(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> DomainScore {
    domain(
        "dailyLife",
        "Daily life",
        scorecard.weights.daily_life,
        vec![
            station_criterion(bundle, scorecard),
            broadband_criterion(bundle, scorecard),
            amenity_signal_criterion(bundle),
        ],
    )
}

fn risk_domain(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> DomainScore {
    domain(
        "risk",
        "Risk",
        scorecard.weights.risk,
        vec![
            page_status_criterion(bundle),
            verification_criterion(bundle),
            evidence_coverage_criterion(bundle),
            assessment_risk_criterion(bundle, scorecard),
        ],
    )
}

fn domain(domain: &str, label: &str, weight: f64, criteria: Vec<CriterionScore>) -> DomainScore {
    let total_weight: f64 = criteria.iter().map(|item| item.weight).sum();
    let score = if total_weight > 0.0 {
        criteria
            .iter()
            .map(|item| item.score * item.weight / total_weight)
            .sum::<f64>()
    } else {
        0.0
    };
    let confidence = confidence_from_criteria(&criteria);
    let summary = domain_summary(label, score, confidence);
    DomainScore {
        domain: domain.to_owned(),
        label: label.to_owned(),
        score: rounded_score(score),
        weight,
        weighted_points: 0.0,
        confidence,
        summary,
        criteria,
    }
}

fn criterion(
    key: &str,
    label: &str,
    score: f64,
    weight: f64,
    confidence: ScoreConfidence,
    evidence: impl Into<String>,
    explanation: impl Into<String>,
) -> CriterionScore {
    CriterionScore {
        key: key.to_owned(),
        label: label.to_owned(),
        score: rounded_score(score),
        weight,
        confidence,
        evidence: evidence.into(),
        explanation: explanation.into(),
    }
}

fn rent_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    match bundle.rightmove.price_pcm {
        Some(price) => {
            let score = inverse_range(
                price as f64,
                scorecard.thresholds.rent_good_pcm,
                scorecard.thresholds.rent_high_pcm,
            );
            criterion(
                "rentPcm",
                "Rent",
                score,
                0.55,
                ScoreConfidence::High,
                format!("£{price} pcm"),
                "lower rent scores higher against the scorecard rent thresholds",
            )
        }
        None => missing("rentPcm", "Rent", 0.55),
    }
}

fn deposit_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    match (bundle.rightmove.deposit, bundle.rightmove.price_pcm) {
        (Some(deposit), Some(price)) if price > 0 => {
            let weeks = deposit as f64 / (price as f64 * 12.0 / 52.0);
            let score = inverse_range(
                weeks,
                scorecard.thresholds.deposit_weeks_good,
                scorecard.thresholds.deposit_weeks_high,
            );
            criterion(
                "depositWeeks",
                "Deposit",
                score,
                0.25,
                ScoreConfidence::High,
                format!("{weeks:.1} weeks"),
                "deposit burden is normalized to rent weeks",
            )
        }
        _ => criterion(
            "depositWeeks",
            "Deposit",
            62.0,
            0.25,
            ScoreConfidence::Low,
            "missing",
            "deposit is missing, so a neutral-low score is used",
        ),
    }
}

fn running_cost_criterion(bundle: &EvidenceBundle) -> CriterionScore {
    match bundle.epc.as_ref().and_then(|epc| epc.rating.as_deref()) {
        Some(rating) => {
            let score = match rating
                .trim()
                .chars()
                .next()
                .map(|ch| ch.to_ascii_uppercase())
            {
                Some('A') => 100.0,
                Some('B') => 90.0,
                Some('C') => 78.0,
                Some('D') => 62.0,
                Some('E') => 45.0,
                Some('F') => 28.0,
                Some('G') => 18.0,
                _ => 55.0,
            };
            criterion(
                "epcRating",
                "Running cost",
                score,
                0.20,
                ScoreConfidence::High,
                rating.to_owned(),
                "EPC rating is used as the deterministic running-cost proxy",
            )
        }
        None => criterion(
            "epcRating",
            "Running cost",
            55.0,
            0.20,
            ScoreConfidence::Low,
            "missing",
            "EPC rating is missing, so running-cost confidence is low",
        ),
    }
}

fn bedrooms_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    match bundle.rightmove.bedrooms {
        Some(bedrooms) => {
            let target = scorecard.thresholds.target_bedrooms;
            let delta = (bedrooms as f64 - target).abs();
            let score = if bedrooms as f64 >= target {
                (88.0 + (bedrooms as f64 - target) * 4.0).min(100.0)
            } else {
                (82.0 - delta * 24.0).max(25.0)
            };
            criterion(
                "bedrooms",
                "Bedrooms",
                score,
                0.30,
                ScoreConfidence::High,
                bedrooms.to_string(),
                "bedroom count is compared with the scorecard target",
            )
        }
        None => missing("bedrooms", "Bedrooms", 0.30),
    }
}

fn floor_area_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    let bedrooms = bundle.rightmove.bedrooms.unwrap_or(1).max(1) as f64;
    match bundle.epc.as_ref().and_then(|epc| epc.floor_area_sqm) {
        Some(area) => {
            let per_bed = area / bedrooms;
            let score = linear_range(
                per_bed,
                scorecard.thresholds.spacious_bedroom_sqm * 0.55,
                scorecard.thresholds.spacious_bedroom_sqm,
            );
            criterion(
                "floorAreaPerBedroom",
                "Space",
                score,
                0.30,
                ScoreConfidence::High,
                format!("{area:.0} sqm / {per_bed:.0} sqm per bedroom"),
                "floor area is normalized by bedroom count",
            )
        }
        None => criterion(
            "floorAreaPerBedroom",
            "Space",
            58.0,
            0.30,
            ScoreConfidence::Low,
            "missing",
            "floor area is missing, so spaciousness is uncertain",
        ),
    }
}

fn media_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    let photo_count = bundle.media.photos.len();
    let score = linear_range(
        photo_count as f64,
        1.0,
        scorecard.thresholds.min_photo_count,
    );
    criterion(
        "photoCoverage",
        "Photo coverage",
        score,
        0.25,
        if photo_count > 0 {
            ScoreConfidence::Medium
        } else {
            ScoreConfidence::Low
        },
        format!("{photo_count} photos"),
        "more cached/listed photos improve property-condition confidence",
    )
}

fn features_criterion(bundle: &EvidenceBundle) -> CriterionScore {
    let text = bundle
        .rightmove
        .description
        .normalized_text
        .to_ascii_lowercase();
    let positives = [
        "garden",
        "balcony",
        "parking",
        "garage",
        "storage",
        "study",
        "office",
        "renovated",
    ];
    let hits = positives
        .iter()
        .filter(|needle| text.contains(**needle))
        .count();
    let score = (58.0 + hits as f64 * 8.0).min(96.0);
    criterion(
        "featureSignals",
        "Feature signals",
        score,
        0.15,
        ScoreConfidence::Medium,
        format!("{hits} positive text signal(s)"),
        "description feature keywords provide a light deterministic uplift",
    )
}

fn imd_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    match fact_f64(bundle, "deprivation", &["imdDecile", "decile"]) {
        Some(decile) => {
            let score = linear_range(decile, 1.0, scorecard.thresholds.imd_good_decile);
            criterion(
                "imdDecile",
                "Area deprivation",
                score,
                0.40,
                ScoreConfidence::Medium,
                format!("decile {decile:.0}"),
                "higher IMD decile improves the macro-location score",
            )
        }
        None => missing("imdDecile", "Area deprivation", 0.40),
    }
}

fn crime_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    match fact_f64(bundle, "crime", &["ratePer1k", "crimeRatePer1k"]) {
        Some(rate) => {
            let score = inverse_range(
                rate,
                scorecard.thresholds.crime_low_per_1k,
                scorecard.thresholds.crime_high_per_1k,
            );
            criterion(
                "crimeRatePer1k",
                "Crime",
                score,
                0.40,
                ScoreConfidence::Medium,
                format!("{rate:.1} per 1k"),
                "lower recorded crime rate improves the location score",
            )
        }
        None => missing("crimeRatePer1k", "Crime", 0.40),
    }
}

fn flood_criterion(bundle: &EvidenceBundle) -> CriterionScore {
    match fact_text(bundle, "flood", &["risk"]) {
        Some(risk) => {
            let normalized = risk.to_ascii_lowercase();
            let score = if normalized.contains("very low") || normalized == "low" {
                88.0
            } else if normalized.contains("medium") {
                58.0
            } else if normalized.contains("high") {
                32.0
            } else {
                62.0
            };
            criterion(
                "floodRisk",
                "Flood risk",
                score,
                0.20,
                ScoreConfidence::Medium,
                risk,
                "flood source text is mapped to a risk score",
            )
        }
        None => criterion(
            "floodRisk",
            "Flood risk",
            62.0,
            0.20,
            ScoreConfidence::Low,
            "missing",
            "flood source is missing, so a neutral-low score is used",
        ),
    }
}

fn station_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    match bundle
        .rightmove
        .nearest_stations
        .first()
        .map(|station| station.distance)
    {
        Some(distance) => {
            let score = inverse_range(
                distance,
                scorecard.thresholds.station_good_miles,
                scorecard.thresholds.station_high_miles,
            );
            criterion(
                "nearestStationMiles",
                "Station access",
                score,
                0.38,
                ScoreConfidence::Medium,
                format!("{distance:.1} miles"),
                "nearest station distance is scored against scorecard thresholds",
            )
        }
        None => missing("nearestStationMiles", "Station access", 0.38),
    }
}

fn broadband_criterion(bundle: &EvidenceBundle, scorecard: &ScorecardConfig) -> CriterionScore {
    match bundle
        .broadband
        .as_ref()
        .and_then(|broadband| broadband.gigabit_availability)
    {
        Some(availability) => {
            let score = linear_range(
                availability,
                scorecard.thresholds.broadband_low_pct,
                scorecard.thresholds.broadband_good_pct,
            );
            criterion(
                "gigabitAvailability",
                "Broadband",
                score,
                0.42,
                ScoreConfidence::High,
                format!("{availability:.0}% gigabit"),
                "postcode gigabit availability is scored against scorecard thresholds",
            )
        }
        None => missing("gigabitAvailability", "Broadband", 0.42),
    }
}

fn amenity_signal_criterion(bundle: &EvidenceBundle) -> CriterionScore {
    let text = bundle
        .rightmove
        .description
        .normalized_text
        .to_ascii_lowercase();
    let hits = [
        "school",
        "park",
        "shops",
        "station",
        "high street",
        "nursery",
    ]
    .iter()
    .filter(|needle| text.contains(**needle))
    .count();
    criterion(
        "amenitySignals",
        "Amenities",
        (55.0 + hits as f64 * 9.0).min(92.0),
        0.20,
        ScoreConfidence::Low,
        format!("{hits} text signal(s)"),
        "amenity mentions are weak evidence and should be verified manually",
    )
}

fn page_status_criterion(bundle: &EvidenceBundle) -> CriterionScore {
    let status = bundle.rightmove.page_status.trim();
    let score = if status.eq_ignore_ascii_case("active") {
        92.0
    } else if status.eq_ignore_ascii_case("letAgreed") || status.eq_ignore_ascii_case("let_agreed")
    {
        40.0
    } else {
        25.0
    };
    criterion(
        "pageStatus",
        "Availability risk",
        score,
        0.30,
        ScoreConfidence::High,
        status.to_owned(),
        "inactive or let-agreed listings are capped as availability risks",
    )
}

fn verification_criterion(bundle: &EvidenceBundle) -> CriterionScore {
    if bundle.verifications.is_empty() {
        return criterion(
            "claimVerifications",
            "Claim verification",
            62.0,
            0.25,
            ScoreConfidence::Low,
            "no verifications",
            "no extracted claims were verified",
        );
    }
    let contradicted = bundle
        .verifications
        .iter()
        .filter(|item| item.status == VerificationStatus::Contradicted)
        .count();
    let supported = bundle
        .verifications
        .iter()
        .filter(|item| item.status == VerificationStatus::Supported)
        .count();
    let score = if contradicted > 0 {
        35.0
    } else {
        (70.0 + supported as f64 * 8.0).min(95.0)
    };
    criterion(
        "claimVerifications",
        "Claim verification",
        score,
        0.25,
        ScoreConfidence::Medium,
        format!("{supported} supported / {contradicted} contradicted"),
        "supported claims reduce risk; contradicted claims create risk",
    )
}

fn evidence_coverage_criterion(bundle: &EvidenceBundle) -> CriterionScore {
    let tracked = [
        "rightmove",
        "address",
        "broadband",
        "media",
        "verifications",
    ];
    let ok_count = tracked
        .iter()
        .filter(|name| {
            bundle.sections.get(**name).is_some_and(|section| {
                matches!(section.status, SectionStatus::Ok | SectionStatus::Partial)
            })
        })
        .count();
    let score = linear_range(ok_count as f64, 1.0, tracked.len() as f64);
    criterion(
        "evidenceCoverage",
        "Evidence coverage",
        score,
        0.25,
        ScoreConfidence::Medium,
        format!("{ok_count}/{} core sections", tracked.len()),
        "more complete evidence lowers uncertainty risk",
    )
}

fn assessment_risk_criterion(
    bundle: &EvidenceBundle,
    _scorecard: &ScorecardConfig,
) -> CriterionScore {
    let Some(record) = bundle.assessment.as_ref() else {
        return criterion(
            "assessmentRisk",
            "Assessment risk",
            62.0,
            0.20,
            ScoreConfidence::Low,
            "no saved assessment",
            "agent assessment has not been saved yet",
        );
    };
    let normalized = &record.normalized_assessment;
    let risk_count = normalized.risks.len() + normalized.evidence_gaps.len();
    let score = (88.0 - risk_count as f64 * 7.0).max(35.0);
    criterion(
        "assessmentRisk",
        "Assessment risk",
        score,
        0.20,
        ScoreConfidence::Medium,
        format!("{risk_count} risk/gap item(s)"),
        "saved assessment risks and gaps reduce the deterministic risk score",
    )
}

fn missing(key: &str, label: &str, weight: f64) -> CriterionScore {
    criterion(
        key,
        label,
        50.0,
        weight,
        ScoreConfidence::Low,
        "missing",
        "evidence is missing, so a neutral-low score is used",
    )
}

fn linear_range(value: f64, low: f64, high: f64) -> f64 {
    if value <= low {
        return 25.0;
    }
    if value >= high {
        return 100.0;
    }
    25.0 + (value - low) / (high - low).max(f64::EPSILON) * 75.0
}

fn inverse_range(value: f64, good: f64, high: f64) -> f64 {
    if value <= good {
        return 100.0;
    }
    if value >= high {
        return 25.0;
    }
    100.0 - (value - good) / (high - good).max(f64::EPSILON) * 75.0
}

fn fact_f64(bundle: &EvidenceBundle, category: &str, names: &[&str]) -> Option<f64> {
    bundle
        .facts
        .iter()
        .find(|fact| {
            fact.category == category
                && names
                    .iter()
                    .any(|name| fact.name.eq_ignore_ascii_case(name))
        })
        .and_then(|fact| {
            fact.value
                .as_f64()
                .or_else(|| fact.value.as_i64().map(|value| value as f64))
        })
}

fn fact_text(bundle: &EvidenceBundle, category: &str, names: &[&str]) -> Option<String> {
    bundle
        .facts
        .iter()
        .find(|fact| {
            fact.category == category
                && names
                    .iter()
                    .any(|name| fact.name.eq_ignore_ascii_case(name))
        })
        .and_then(|fact| fact.value.as_str().map(ToOwned::to_owned))
}

fn blockers(bundle: &EvidenceBundle) -> Vec<ScoreBlocker> {
    let mut blockers = Vec::new();
    let status = bundle.rightmove.page_status.to_ascii_lowercase();
    if status.contains("removed") {
        blockers.push(ScoreBlocker {
            code: "listing_removed".to_owned(),
            summary: "listing is no longer available".to_owned(),
        });
    }
    if bundle
        .verifications
        .iter()
        .any(|item| item.status == VerificationStatus::Contradicted)
    {
        blockers.push(ScoreBlocker {
            code: "contradicted_claim".to_owned(),
            summary: "one or more listing claims are contradicted by evidence".to_owned(),
        });
    }
    blockers
}

fn caps(bundle: &EvidenceBundle, scorecard: &ScorecardConfig, no_blockers: bool) -> Vec<ScoreCap> {
    let mut caps = Vec::new();
    if !no_blockers {
        caps.push(ScoreCap {
            cap: scorecard.caps.blocker_cap,
            reason: "hard blocker present".to_owned(),
        });
    }
    let degraded_sections = bundle
        .sections
        .values()
        .filter(|section| {
            matches!(
                section.status,
                SectionStatus::Degraded | SectionStatus::Blocked
            )
        })
        .count();
    if degraded_sections >= 2 {
        caps.push(ScoreCap {
            cap: scorecard.caps.partial_evidence_cap,
            reason: format!("{degraded_sections} evidence sections are degraded or blocked"),
        });
    }
    if bundle.broadband.is_none() && bundle.epc.is_none() && bundle.media.photos.is_empty() {
        caps.push(ScoreCap {
            cap: scorecard.caps.low_evidence_cap,
            reason: "broadband, EPC, and photo evidence are all missing".to_owned(),
        });
    }
    caps
}

fn score_next_actions(bundle: &EvidenceBundle) -> Vec<String> {
    let mut actions = Vec::new();
    if bundle.broadband.is_none() {
        actions.push("build broadband sources or verify address-level broadband".to_owned());
    }
    if bundle.epc.is_none() {
        actions.push(
            "capture or correct EPC evidence before relying on running-cost score".to_owned(),
        );
    }
    if bundle.assessment.is_none() {
        actions.push("save an agent assessment with judgment calibration if the base score needs human-fit adjustment".to_owned());
    }
    actions
}

fn confidence_from_criteria(criteria: &[CriterionScore]) -> ScoreConfidence {
    let low = criteria
        .iter()
        .filter(|item| item.confidence == ScoreConfidence::Low)
        .count();
    if low >= criteria.len().saturating_sub(1).max(1) {
        ScoreConfidence::Low
    } else if low > 0 {
        ScoreConfidence::Medium
    } else {
        ScoreConfidence::High
    }
}

fn result_confidence(domains: &[DomainScore], caps: &[ScoreCap]) -> ScoreConfidence {
    let low_domains = domains
        .iter()
        .filter(|domain| domain.confidence == ScoreConfidence::Low)
        .count();
    if !caps.is_empty() || low_domains >= 2 {
        ScoreConfidence::Low
    } else if domains.iter().any(|domain| {
        domain.confidence == ScoreConfidence::Low || domain.confidence == ScoreConfidence::Medium
    }) {
        ScoreConfidence::Medium
    } else {
        ScoreConfidence::High
    }
}

fn domain_summary(label: &str, score: f64, confidence: ScoreConfidence) -> String {
    format!(
        "{label} score {:.0} with {} confidence",
        rounded_score(score),
        confidence.as_str()
    )
}

fn score_summary(
    base_overall: f64,
    overall: f64,
    band: ScoreBand,
    confidence: ScoreConfidence,
    domains: &[DomainScore],
) -> String {
    let strongest = domains
        .iter()
        .max_by(|left, right| left.score.total_cmp(&right.score))
        .map(|domain| domain.label.as_str())
        .unwrap_or("n/a");
    let weakest = domains
        .iter()
        .min_by(|left, right| left.score.total_cmp(&right.score))
        .map(|domain| domain.label.as_str())
        .unwrap_or("n/a");
    let calibration = if rounded_score(base_overall) == rounded_score(overall) {
        String::new()
    } else {
        format!("; base score {:.0}", base_overall)
    };
    format!(
        "{:.0}/100 {} score with {} confidence; strongest domain: {}; weakest domain: {}",
        overall,
        band.as_str(),
        confidence.as_str(),
        strongest,
        weakest
    ) + &calibration
}

pub(crate) fn rounded_score(value: f64) -> f64 {
    (value.clamp(0.0, 100.0) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests;
