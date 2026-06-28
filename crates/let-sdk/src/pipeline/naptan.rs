#![forbid(unsafe_code)]

use crate::errors::Result;
use crate::pipeline::enrich::SourceEnricher;
use crate::schema::listing::{Listing, StationDistance};
use serde::Serialize;

const MAX_STATION_DISTANCE_M: f64 = 5_000.0;
const LOOKUP_LIMIT: usize = 12;
const OUTPUT_LIMIT: usize = 3;
const METERS_PER_MILE: f64 = 1_609.344;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NaptanStopCandidate {
    pub name: String,
    pub stop_type: Option<String>,
    pub distance_m: f64,
}

pub fn resolve_listing_stations(
    enricher: &SourceEnricher,
    listing: &Listing,
) -> Result<Option<Vec<StationDistance>>> {
    if !listing.nearest_stations.is_empty() {
        return Ok(None);
    }

    let candidates = enricher.lookup_naptan_stops(
        listing.location.lat,
        listing.location.lng,
        MAX_STATION_DISTANCE_M,
        LOOKUP_LIMIT,
    )?;
    Ok(select_station_backfill(&candidates))
}

fn select_station_backfill(candidates: &[NaptanStopCandidate]) -> Option<Vec<StationDistance>> {
    let mut selected = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for candidate in candidates {
        if !looks_like_station(candidate) {
            continue;
        }

        let dedupe_key = candidate.name.trim().to_ascii_lowercase();
        if dedupe_key.is_empty() || !seen.insert(dedupe_key) {
            continue;
        }

        selected.push(StationDistance {
            name: candidate.name.trim().to_owned(),
            distance: candidate.distance_m / METERS_PER_MILE,
            unit: "miles".to_owned(),
        });

        if selected.len() == OUTPUT_LIMIT {
            break;
        }
    }

    (!selected.is_empty()).then_some(selected)
}

fn looks_like_station(candidate: &NaptanStopCandidate) -> bool {
    let normalized_name = candidate.name.to_ascii_lowercase();
    let normalized_type = candidate
        .stop_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();

    let keyword_match = [
        "station",
        "rail",
        "tram",
        "metrolink",
        "metro",
        "underground",
        "tube",
        "overground",
        "dlr",
    ]
    .iter()
    .any(|keyword| normalized_name.contains(keyword) || normalized_type.contains(keyword));
    if keyword_match {
        return true;
    }

    ["rly", "rail", "met", "tram", "plt", "tmu", "dlr"]
        .iter()
        .any(|code| normalized_type.contains(code))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::pipeline::enrich::SourceEnricher;
    use crate::schema::listing::{
        Agent, AreaMetrics, ExtractionStatus, GeoLocation, Lettings, Listing, ListingStatus,
        MapViews, PortalIds, RemoteLocalAsset, StationDistance,
    };

    use super::{NaptanStopCandidate, resolve_listing_stations, select_station_backfill};

    #[test]
    fn backfill_filters_to_station_like_candidates() {
        let result = select_station_backfill(&[
            NaptanStopCandidate {
                name: "Market Street Bus Stop".to_owned(),
                stop_type: Some("BCT".to_owned()),
                distance_m: 100.0,
            },
            NaptanStopCandidate {
                name: "Central Rail Station".to_owned(),
                stop_type: Some("RLY".to_owned()),
                distance_m: 300.0,
            },
            NaptanStopCandidate {
                name: "Town Tram Stop".to_owned(),
                stop_type: Some("MKD".to_owned()),
                distance_m: 450.0,
            },
        ])
        .expect("backfill");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "Central Rail Station");
        assert_eq!(result[1].name, "Town Tram Stop");
    }

    #[test]
    fn backfill_deduplicates_names_and_limits_results() {
        let result = select_station_backfill(&[
            NaptanStopCandidate {
                name: "North Station".to_owned(),
                stop_type: Some("RLY".to_owned()),
                distance_m: 200.0,
            },
            NaptanStopCandidate {
                name: "North Station".to_owned(),
                stop_type: Some("RLY".to_owned()),
                distance_m: 250.0,
            },
            NaptanStopCandidate {
                name: "East Tram Stop".to_owned(),
                stop_type: Some("MKD".to_owned()),
                distance_m: 400.0,
            },
            NaptanStopCandidate {
                name: "West Underground Station".to_owned(),
                stop_type: Some("MET".to_owned()),
                distance_m: 800.0,
            },
            NaptanStopCandidate {
                name: "South Rail Station".to_owned(),
                stop_type: Some("RLY".to_owned()),
                distance_m: 1_000.0,
            },
        ])
        .expect("backfill");

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].name, "North Station");
        assert_eq!(result[1].name, "East Tram Stop");
        assert_eq!(result[2].name, "West Underground Station");
    }

    #[test]
    fn existing_scraped_stations_are_preserved() {
        let temp = TempDir::new().expect("temp dir");
        let enricher = SourceEnricher::open(temp.path()).expect("open enricher");
        let listing = Listing {
            id: "listing-1".to_owned(),
            portal_ids: PortalIds::default(),
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: "https://example.com/listing".to_owned(),
            location: GeoLocation {
                lat: 51.5074,
                lng: -0.1278,
                pin_type: None,
            },
            postcode: "SW1A 1AA".to_owned(),
            address: "1 Example Street".to_owned(),
            region: None,
            google_maps_url: "https://maps.example.com".to_owned(),
            google_maps_street_view_url: "https://maps.example.com/street".to_owned(),
            area: AreaMetrics::default(),
            price: 1500,
            price_display: "£1,500 pcm".to_owned(),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "Flat".to_owned(),
            description: "desc".to_owned(),
            notes: vec![],
            images: vec![],
            floorplan: RemoteLocalAsset::default(),
            epc: RemoteLocalAsset::default(),
            map_views: MapViews::default(),
            epc_rating: None,
            floor_area_sqm: None,
            epc_lodgement_date: None,
            epc_address_match: None,
            epc_search_url: None,
            nearest_stations: vec![StationDistance {
                name: "Existing Station".to_owned(),
                distance: 0.4,
                unit: "miles".to_owned(),
            }],
            gigabit_availability: None,
            listed_date: None,
            lettings: Lettings::default(),
            agent: Agent::default(),
            fetched_at: "2026-03-10T00:00:00.000Z".to_owned(),
            extraction_status: ExtractionStatus::Success,
            status: ListingStatus::Active,
            notion_page_id: None,
        };

        let result = resolve_listing_stations(&enricher, &listing).expect("resolve stations");
        assert!(result.is_none());
    }
}
