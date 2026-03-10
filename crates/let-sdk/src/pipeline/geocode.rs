#![forbid(unsafe_code)]

use serde::Deserialize;

use crate::errors::{ErrorCode, LetError, Result};

#[derive(Debug, Clone)]
pub struct GeocodedCoordinates {
    pub lat: f64,
    pub lng: f64,
    pub source: GeocodeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeocodeSource {
    PostcodesDb,
    MapboxGeocode,
    FallbackOriginal,
}

impl GeocodeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PostcodesDb => "postcodes_db",
            Self::MapboxGeocode => "mapbox_geocode",
            Self::FallbackOriginal => "fallback_original",
        }
    }
}

#[derive(Debug, Deserialize)]
struct MapboxGeocodeResponse {
    features: Vec<MapboxFeature>,
}

#[derive(Debug, Deserialize)]
struct MapboxFeature {
    geometry: MapboxGeometry,
}

#[derive(Debug, Deserialize)]
struct MapboxGeometry {
    coordinates: Vec<f64>,
}

pub async fn mapbox_forward_geocode(
    client: &reqwest::Client,
    query: &str,
    access_token: &str,
) -> Result<Option<GeocodedCoordinates>> {
    let encoded_query: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();

    let url = format!(
        "https://api.mapbox.com/search/geocode/v6/forward?q={encoded_query}&country=gb&types=postcode,address&limit=1&access_token={access_token}",
    );

    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|err| {
            LetError::new(
                ErrorCode::Network,
                format!("mapbox geocode request failed: {err}"),
                "check network and MAPBOX_ACCESS_TOKEN",
            )
        })?;

    if !response.status().is_success() {
        eprintln!(
            "[geocode] mapbox returned status {} for query {:?}",
            response.status(),
            query,
        );
        return Ok(None);
    }

    let body: MapboxGeocodeResponse = response.json().await.map_err(|err| {
        LetError::new(
            ErrorCode::Parse,
            format!("failed to parse mapbox geocode response: {err}"),
            "check mapbox geocode API response format",
        )
    })?;

    let coords = body
        .features
        .first()
        .filter(|f| f.geometry.coordinates.len() >= 2)
        .map(|f| GeocodedCoordinates {
            lng: f.geometry.coordinates[0],
            lat: f.geometry.coordinates[1],
            source: GeocodeSource::MapboxGeocode,
        });

    Ok(coords)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geocode_source_as_str_values() {
        assert_eq!(GeocodeSource::PostcodesDb.as_str(), "postcodes_db");
        assert_eq!(GeocodeSource::MapboxGeocode.as_str(), "mapbox_geocode");
        assert_eq!(
            GeocodeSource::FallbackOriginal.as_str(),
            "fallback_original"
        );
    }

    #[test]
    fn parses_mapbox_response_json() {
        let json = r#"{
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [-0.1278, 51.5074]
                },
                "properties": {}
            }]
        }"#;

        let response: MapboxGeocodeResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(response.features.len(), 1);
        let coords = &response.features[0].geometry.coordinates;
        assert!((coords[0] - (-0.1278)).abs() < 0.001);
        assert!((coords[1] - 51.5074).abs() < 0.001);
    }

    #[test]
    fn parses_empty_features() {
        let json = r#"{"type": "FeatureCollection", "features": []}"#;
        let response: MapboxGeocodeResponse = serde_json::from_str(json).expect("parse");
        assert!(response.features.is_empty());
    }
}
