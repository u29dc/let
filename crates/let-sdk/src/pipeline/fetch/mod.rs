#![forbid(unsafe_code)]

use chrono::DateTime;

use crate::schema::listing::Listing;

pub mod cache;
pub mod maps;
pub mod media;
pub mod rightmove;

pub fn carry_over_persistent_fields(incoming: &mut Listing, existing: &Listing) {
    if incoming.portal_ids.rightmove != existing.portal_ids.rightmove {
        return;
    }

    incoming.id = existing.id.clone();
    if incoming.portal_ids.zoopla.is_none() {
        incoming.portal_ids.zoopla = existing.portal_ids.zoopla.clone();
    }
    if incoming.portal_ids.onthemarket.is_none() {
        incoming.portal_ids.onthemarket = existing.portal_ids.onthemarket.clone();
    }
    if incoming.notion_page_id.is_none() {
        incoming.notion_page_id = existing.notion_page_id.clone();
    }
    if incoming.assessment.is_none() {
        incoming.assessment = existing.assessment.clone();
    }
    if incoming.assessed_at.is_none() {
        incoming.assessed_at = existing.assessed_at.clone();
    }
    if incoming.assessed_score.is_none() {
        incoming.assessed_score = existing.assessed_score;
    }
    if incoming.uprn.is_none() {
        incoming.uprn = existing.uprn.clone();
    }
    if incoming.uprn_source.is_none() {
        incoming.uprn_source = existing.uprn_source.clone();
    }
    if incoming.uprn_confidence.is_none() {
        incoming.uprn_confidence = existing.uprn_confidence.clone();
    }

    preserve_media_paths(incoming, existing);
}

fn preserve_media_paths(incoming: &mut Listing, existing: &Listing) {
    for image in &mut incoming.images {
        if image.local.is_some() {
            continue;
        }
        if let Some(existing_local) = existing
            .images
            .iter()
            .find(|candidate| candidate.remote == image.remote)
            .and_then(|candidate| candidate.local.clone())
        {
            image.local = Some(existing_local);
        }
    }

    preserve_remote_local_asset(&mut incoming.floorplan, &existing.floorplan);
    preserve_remote_local_asset(&mut incoming.epc, &existing.epc);
    preserve_remote_local_asset(
        &mut incoming.map_views.satellite,
        &existing.map_views.satellite,
    );
    preserve_remote_local_asset(&mut incoming.map_views.street, &existing.map_views.street);
}

fn preserve_remote_local_asset(
    incoming: &mut crate::schema::listing::RemoteLocalAsset,
    existing: &crate::schema::listing::RemoteLocalAsset,
) {
    if incoming.local.is_some() {
        return;
    }
    if incoming.remote == existing.remote {
        incoming.local = existing.local.clone();
    }
}

pub fn is_newer_listing(left: &Listing, right: &Listing) -> bool {
    let left_ts = parse_timestamp(&left.fetched_at);
    let right_ts = parse_timestamp(&right.fetched_at);
    left_ts > right_ts
}

fn parse_timestamp(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::schema::listing::{
        Agent, AreaMetrics, ExtractionStatus, GeoLocation, Lettings, Listing, ListingImage,
        ListingStatus, MapViews, PortalIds, RemoteLocalAsset,
    };

    use super::carry_over_persistent_fields;

    #[test]
    fn preserves_existing_image_local_paths() {
        let mut incoming = sample_listing();
        incoming.images = vec![ListingImage {
            remote: "https://media.rightmove.co.uk/a.jpg".to_owned(),
            local: None,
        }];

        let mut existing = sample_listing();
        existing.images = vec![ListingImage {
            remote: "https://media.rightmove.co.uk/a.jpg".to_owned(),
            local: Some("170448131/a.jpg".to_owned()),
        }];

        carry_over_persistent_fields(&mut incoming, &existing);
        assert_eq!(incoming.images[0].local.as_deref(), Some("170448131/a.jpg"));
    }

    fn sample_listing() -> Listing {
        Listing {
            id: "uuid-1".to_owned(),
            portal_ids: PortalIds {
                rightmove: Some("170448131".to_owned()),
                zoopla: None,
                onthemarket: None,
            },
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
            location: GeoLocation {
                lat: 53.0,
                lng: -2.0,
                pin_type: None,
            },
            postcode: "M1 1AA".to_owned(),
            address: "10 Example Street".to_owned(),
            region: Some("Manchester".to_owned()),
            google_maps_url: "https://maps.example.com".to_owned(),
            google_maps_street_view_url: "https://maps.example.com/street".to_owned(),
            area: AreaMetrics::default(),
            price: 1200,
            price_display: "£1,200 pcm".to_owned(),
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
            nearest_stations: vec![],
            gigabit_availability: None,
            listed_date: None,
            lettings: Lettings::default(),
            agent: Agent::default(),
            assessment: None,
            assessed_at: None,
            assessed_score: None,
            scores: None,
            fetched_at: "2026-01-01T00:00:00.000Z".to_owned(),
            extraction_status: ExtractionStatus::Partial,
            status: ListingStatus::Active,
            notion_page_id: None,
        }
    }
}
