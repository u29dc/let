#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::schema::listing::Listing;

pub const MEDIA_NORMALIZATION_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Photo,
    Floorplan,
    Epc,
    MapSatellite,
    MapStreet,
    ContactSheet,
}

impl AssetKind {
    pub fn slug(self) -> &'static str {
        match self {
            AssetKind::Photo => "photo",
            AssetKind::Floorplan => "floorplan",
            AssetKind::Epc => "epc",
            AssetKind::MapSatellite => "map-satellite",
            AssetKind::MapStreet => "map-street",
            AssetKind::ContactSheet => "contact-sheet",
        }
    }
}

pub fn cache_key_for_listing(listing: &Listing) -> String {
    listing
        .portal_ids
        .rightmove
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| listing.id.clone())
}

pub fn listing_cache_dir(cache_root: &Path, listing: &Listing) -> PathBuf {
    cache_root.join(cache_key_for_listing(listing))
}

pub fn ensure_listing_cache_dir(cache_root: &Path, listing: &Listing) -> std::io::Result<PathBuf> {
    let dir = listing_cache_dir(cache_root, listing);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn asset_filename(
    listing: &Listing,
    kind: AssetKind,
    seed: &str,
    profile_hash: &str,
    index: Option<usize>,
) -> String {
    let listing_key = cache_key_for_listing(listing);
    let digest = short_hash(seed);
    let profile = short_hash(profile_hash);

    match index {
        Some(position) => format!(
            "{listing_key}-{kind}-{position:02}-{digest}-{profile}-{version}.jpg",
            kind = kind.slug(),
            version = MEDIA_NORMALIZATION_VERSION
        ),
        None => format!(
            "{listing_key}-{kind}-{digest}-{profile}-{version}.jpg",
            kind = kind.slug(),
            version = MEDIA_NORMALIZATION_VERSION
        ),
    }
}

pub fn local_asset_path(listing: &Listing, filename: &str) -> String {
    format!("{}/{}", cache_key_for_listing(listing), filename)
}

pub fn short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

mod hex {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for value in bytes {
            out.push(char::from(HEX[(value >> 4) as usize]));
            out.push(char::from(HEX[(value & 0x0f) as usize]));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::listing::{
        Agent, AreaMetrics, ExtractionStatus, GeoLocation, Lettings, Listing, ListingStatus,
        MapViews, PortalIds, RemoteLocalAsset,
    };

    use super::{AssetKind, asset_filename, cache_key_for_listing, local_asset_path};

    #[test]
    fn cache_key_prefers_portal_id() {
        let listing = sample_listing(Some("170448131"));
        assert_eq!(cache_key_for_listing(&listing), "170448131");
    }

    #[test]
    fn filename_contains_profile_and_version() {
        let listing = sample_listing(Some("170448131"));
        let filename = asset_filename(
            &listing,
            AssetKind::Photo,
            "https://img.example.com/a.jpg",
            "1200x900-q82",
            Some(0),
        );
        assert!(filename.ends_with("-v1.jpg"));
        assert!(filename.contains("-photo-00-"));
    }

    #[test]
    fn local_path_prefixes_listing_key() {
        let listing = sample_listing(Some("170448131"));
        assert_eq!(
            local_asset_path(&listing, "image.jpg"),
            "170448131/image.jpg"
        );
    }

    fn sample_listing(rightmove_id: Option<&str>) -> Listing {
        Listing {
            id: "uuid-1".to_owned(),
            portal_ids: PortalIds {
                rightmove: rightmove_id.map(ToOwned::to_owned),
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
            fetched_at: "2026-01-01T00:00:00.000Z".to_owned(),
            extraction_status: ExtractionStatus::Partial,
            status: ListingStatus::Active,
        }
    }
}
