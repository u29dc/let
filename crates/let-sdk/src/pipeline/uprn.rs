#![forbid(unsafe_code)]

use crate::errors::Result;
use crate::pipeline::enrich::SourceEnricher;
use crate::schema::listing::{Listing, PinType, UprnConfidence, UprnSource};

const MAX_FALLBACK_DISTANCE_M: f64 = 25.0;
const LOOKUP_LIMIT: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct UprnDistanceCandidate {
    pub uprn: String,
    pub distance_m: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UprnResolution {
    pub uprn: String,
    pub source: UprnSource,
    pub confidence: UprnConfidence,
}

pub fn resolve_listing_uprn(
    enricher: &SourceEnricher,
    listing: &Listing,
) -> Result<Option<UprnResolution>> {
    let candidates = enricher.lookup_uprn_candidates(
        listing.location.lat,
        listing.location.lng,
        MAX_FALLBACK_DISTANCE_M,
        LOOKUP_LIMIT,
    )?;
    Ok(resolve_from_distance_candidates(
        listing.location.pin_type.as_ref(),
        &candidates,
    ))
}

fn resolve_from_distance_candidates(
    pin_type: Option<&PinType>,
    candidates: &[UprnDistanceCandidate],
) -> Option<UprnResolution> {
    let nearest = candidates.first()?;
    let second_distance = candidates.get(1).map(|candidate| candidate.distance_m);
    let confidence = classify_confidence(pin_type, nearest.distance_m, second_distance)?;

    Some(UprnResolution {
        uprn: nearest.uprn.clone(),
        source: UprnSource::OsOpen,
        confidence,
    })
}

fn classify_confidence(
    pin_type: Option<&PinType>,
    nearest_distance_m: f64,
    second_distance_m: Option<f64>,
) -> Option<UprnConfidence> {
    let unique_margin = second_distance_m.map(|distance| distance - nearest_distance_m);

    match pin_type {
        Some(PinType::AccuratePoint) => {
            if nearest_distance_m <= 2.0 && unique_margin.is_none_or(|margin| margin >= 12.0) {
                return Some(UprnConfidence::Exact);
            }
            if nearest_distance_m <= 8.0 && unique_margin.is_none_or(|margin| margin >= 8.0) {
                return Some(UprnConfidence::Probable);
            }
            if nearest_distance_m <= 15.0 && unique_margin.is_none_or(|margin| margin >= 12.0) {
                return Some(UprnConfidence::Heuristic);
            }
            None
        }
        Some(PinType::ApproximatePoint) | None => {
            if nearest_distance_m <= 6.0 && unique_margin.is_none_or(|margin| margin >= 12.0) {
                return Some(UprnConfidence::Heuristic);
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::listing::{PinType, UprnConfidence};

    use super::{UprnDistanceCandidate, resolve_from_distance_candidates};

    #[test]
    fn exact_requires_accurate_pin_and_strong_unique_match() {
        let resolved = resolve_from_distance_candidates(
            Some(&PinType::AccuratePoint),
            &[
                UprnDistanceCandidate {
                    uprn: "1001".to_owned(),
                    distance_m: 1.2,
                },
                UprnDistanceCandidate {
                    uprn: "1002".to_owned(),
                    distance_m: 18.0,
                },
            ],
        )
        .expect("resolution");

        assert_eq!(resolved.confidence, UprnConfidence::Exact);
    }

    #[test]
    fn approximate_pin_only_returns_heuristic() {
        let resolved = resolve_from_distance_candidates(
            Some(&PinType::ApproximatePoint),
            &[UprnDistanceCandidate {
                uprn: "1001".to_owned(),
                distance_m: 4.0,
            }],
        )
        .expect("resolution");

        assert_eq!(resolved.confidence, UprnConfidence::Heuristic);
    }

    #[test]
    fn ambiguous_candidates_return_none() {
        assert!(
            resolve_from_distance_candidates(
                Some(&PinType::AccuratePoint),
                &[
                    UprnDistanceCandidate {
                        uprn: "1001".to_owned(),
                        distance_m: 5.0,
                    },
                    UprnDistanceCandidate {
                        uprn: "1002".to_owned(),
                        distance_m: 7.0,
                    },
                ],
            )
            .is_none()
        );
    }
}
