use serde_json::json;

use crate::intelligence::types::{
    AddressEvidence, AssessmentRecord, BroadbandEvidence, DescriptionEvidence, EvidenceBundle,
    FactEvidence, FactProvider, InspectDepth, MediaEvidence, MediaItemEvidence, RefreshPolicy,
    RightmoveEvidence, SectionState, VerificationEvidence, VerificationStatus,
};
use crate::score::default_scorecard;

use super::compute_score;

#[test]
fn score_engine_rewards_complete_good_evidence() {
    let mut bundle = test_bundle();
    bundle.epc = Some(crate::intelligence::types::EpcEvidence {
        lmk_key: "lmk".to_owned(),
        rating: Some("B".to_owned()),
        floor_area_sqm: Some(82.0),
        lodgement_date: None,
        address_match: true,
        matched_address: "1 Example Street".to_owned(),
        uprn: None,
        uprn_source: None,
    });
    bundle.rightmove.nearest_stations = vec![crate::intelligence::types::NearestStationEvidence {
        name: "Example".to_owned(),
        distance: 0.3,
        unit: "miles".to_owned(),
    }];
    bundle.facts.push(FactEvidence {
        provider: FactProvider::CrimeDb,
        category: "crime".to_owned(),
        name: "ratePer1k".to_owned(),
        value: json!(35.0),
        confidence: crate::intelligence::types::ConfidenceLevel::Probable,
        sources: Vec::new(),
    });
    bundle.facts.push(FactEvidence {
        provider: FactProvider::DeprivationDb,
        category: "deprivation".to_owned(),
        name: "imdDecile".to_owned(),
        value: json!(9),
        confidence: crate::intelligence::types::ConfidenceLevel::Probable,
        sources: Vec::new(),
    });

    let score = compute_score(&bundle, &default_scorecard());
    assert!(score.overall > 70.0, "score was {}", score.overall);
    assert_eq!(score.base_overall, score.overall);
    assert!(
        score
            .domains
            .iter()
            .any(|domain| domain.domain == "dailyLife")
    );
}

#[test]
fn score_engine_caps_low_evidence_bundles() {
    let mut bundle = test_bundle();
    bundle.broadband = None;
    bundle.epc = None;
    bundle.media.photos.clear();

    let score = compute_score(&bundle, &default_scorecard());
    assert!(score.overall <= 62.0, "score was {}", score.overall);
    assert_eq!(score.base_overall, score.overall);
    assert!(!score.caps.is_empty());
}

#[test]
fn score_engine_applies_positive_judgment_adjustment() {
    let mut bundle = test_bundle();
    bundle.assessment = Some(AssessmentRecord::new(
        bundle.entity_id.clone(),
        json!({
            "scoreAdjustment": 8,
            "judgmentRationale": "Human fit is stronger than the base evidence proxy."
        }),
        "2026-06-18T00:00:00.000Z".to_owned(),
    ));

    let score = compute_score(&bundle, &default_scorecard());
    assert_eq!(score.overall, score.base_overall + 8.0);
    assert_eq!(score.judgment.applied_adjustment, 8.0);
}

#[test]
fn score_engine_applies_negative_judgment_adjustment() {
    let mut bundle = test_bundle();
    bundle.assessment = Some(AssessmentRecord::new(
        bundle.entity_id.clone(),
        json!({"scoreAdjustment": -6}),
        "2026-06-18T00:00:00.000Z".to_owned(),
    ));

    let score = compute_score(&bundle, &default_scorecard());
    assert_eq!(score.overall, score.base_overall - 6.0);
}

#[test]
fn score_engine_keeps_hard_blocker_cap_after_positive_judgment() {
    let mut bundle = test_bundle();
    bundle.verifications = vec![VerificationEvidence {
        id: "verification-1".to_owned(),
        claim_id: None,
        claim_type: "broadband".to_owned(),
        status: VerificationStatus::Contradicted,
        confidence: crate::intelligence::types::ConfidenceLevel::Probable,
        explanation: "contradicted".to_owned(),
        evidence: Vec::new(),
    }];
    bundle.assessment = Some(AssessmentRecord::new(
        bundle.entity_id.clone(),
        json!({"scoreAdjustment": 15}),
        "2026-06-18T00:00:00.000Z".to_owned(),
    ));

    let score = compute_score(&bundle, &default_scorecard());
    assert!(score.overall <= default_scorecard().caps.blocker_cap);
    assert_eq!(score.band, crate::score::types::ScoreBand::Reject);
}

fn test_bundle() -> EvidenceBundle {
    EvidenceBundle {
        entity_id: "rightmove:170448131".to_owned(),
        rightmove_id: "170448131".to_owned(),
        url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
        generated_at: "2026-06-18T00:00:00.000Z".to_owned(),
        depth: InspectDepth::Standard,
        refresh: RefreshPolicy::Stale,
        sections: [
            (
                "rightmove".to_owned(),
                SectionState::ok(
                    "captured",
                    crate::intelligence::types::ConfidenceLevel::Probable,
                ),
            ),
            (
                "address".to_owned(),
                SectionState::ok(
                    "matched",
                    crate::intelligence::types::ConfidenceLevel::Probable,
                ),
            ),
            (
                "broadband".to_owned(),
                SectionState::ok(
                    "matched",
                    crate::intelligence::types::ConfidenceLevel::Probable,
                ),
            ),
            (
                "media".to_owned(),
                SectionState::ok(
                    "cached",
                    crate::intelligence::types::ConfidenceLevel::Probable,
                ),
            ),
        ]
        .into_iter()
        .collect(),
        source_snapshots: Vec::new(),
        rightmove: RightmoveEvidence {
            rightmove_id: "170448131".to_owned(),
            url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
            page_status: "active".to_owned(),
            fetched_at: "2026-06-18T00:00:00.000Z".to_owned(),
            content_hash: "hash".to_owned(),
            title: Some("Two bedroom flat".to_owned()),
            address: Some("1 Example Street".to_owned()),
            postcode: Some("M1 1AA".to_owned()),
            display_price: Some("£1,250 pcm".to_owned()),
            price_pcm: Some(1250),
            bedrooms: Some(2),
            bathrooms: Some(1),
            property_type: Some("Flat".to_owned()),
            agent_name: None,
            agent_phone: None,
            latitude: None,
            longitude: None,
            pin_type: None,
            listed_date: None,
            available_date: None,
            deposit: Some(1442),
            description: DescriptionEvidence {
                raw_html: String::new(),
                text: "garden near station".to_owned(),
                key_features: Vec::new(),
                normalized_text: "garden near station".to_owned(),
            },
            nearest_stations: Vec::new(),
            media: Vec::new(),
        },
        address: AddressEvidence {
            candidates: Vec::new(),
            selected: None,
            status: crate::intelligence::types::SectionStatus::Ok,
            confidence: crate::intelligence::types::ConfidenceLevel::Probable,
            warnings: Vec::new(),
        },
        facts: Vec::new(),
        broadband: Some(BroadbandEvidence {
            postcode: "M11AA".to_owned(),
            postcode_display: Some("M1 1AA".to_owned()),
            outward: Some("M1".to_owned()),
            area: Some("M".to_owned()),
            gigabit_availability: Some(88.0),
            pct_over_300mbps: Some(92.0),
            ufbb_availability: Some(95.0),
            sfbb_availability: Some(99.0),
        }),
        epc: None,
        claims: Vec::new(),
        verifications: vec![VerificationEvidence {
            id: "verification-1".to_owned(),
            claim_id: None,
            claim_type: "broadband".to_owned(),
            status: VerificationStatus::Supported,
            confidence: crate::intelligence::types::ConfidenceLevel::Probable,
            explanation: "supported".to_owned(),
            evidence: Vec::new(),
        }],
        media: MediaEvidence {
            photos: vec![MediaItemEvidence {
                kind: "photo".to_owned(),
                remote_url: "https://example.com/photo.jpg".to_owned(),
                local_path: None,
                width: None,
                height: None,
                content_hash: None,
                status: "remote".to_owned(),
            }],
            ..MediaEvidence::default()
        },
        assessment: None,
        corrections: Vec::new(),
        next_actions: Vec::new(),
        flags: Vec::new(),
    }
}
