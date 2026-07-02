#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Listing {
    pub id: String,
    pub portal_ids: PortalIds,
    pub uprn: Option<String>,
    pub uprn_source: Option<UprnSource>,
    pub uprn_confidence: Option<UprnConfidence>,
    pub url: String,
    pub location: GeoLocation,
    pub postcode: String,
    pub address: String,
    pub region: Option<String>,
    pub google_maps_url: String,
    pub google_maps_street_view_url: String,
    pub area: AreaMetrics,
    pub price: i64,
    pub price_display: String,
    pub bedrooms: i64,
    pub bathrooms: i64,
    pub property_type: String,
    pub description: String,
    pub notes: Vec<String>,
    pub images: Vec<ListingImage>,
    pub floorplan: RemoteLocalAsset,
    pub epc: RemoteLocalAsset,
    pub map_views: MapViews,
    pub epc_rating: Option<EpcBand>,
    pub floor_area_sqm: Option<f64>,
    pub epc_lodgement_date: Option<String>,
    pub epc_address_match: Option<bool>,
    pub epc_search_url: Option<String>,
    pub nearest_stations: Vec<StationDistance>,
    pub gigabit_availability: Option<f64>,
    pub listed_date: Option<String>,
    pub lettings: Lettings,
    pub agent: Agent,
    pub fetched_at: String,
    pub extraction_status: ExtractionStatus,
    pub status: ListingStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PortalIds {
    pub rightmove: Option<String>,
    pub zoopla: Option<String>,
    pub onthemarket: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UprnSource {
    Epc,
    OsOpen,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UprnConfidence {
    Exact,
    Probable,
    Heuristic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeoLocation {
    pub lat: f64,
    pub lng: f64,
    pub pin_type: Option<PinType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PinType {
    #[serde(rename = "ACCURATE_POINT")]
    AccuratePoint,
    #[serde(rename = "APPROXIMATE_POINT")]
    ApproximatePoint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AreaMetrics {
    pub lsoa: AreaCodeName,
    pub msoa: AreaCodeName,
    pub imd: ImdMetrics,
    pub income: IncomeMetrics,
    pub social_housing_pct: Option<f64>,
    pub population: Option<i64>,
    pub flood_risk: FloodRisk,
    pub crime: CrimeMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AreaCodeName {
    pub code: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ImdMetrics {
    pub rank: Option<i64>,
    pub decile: Option<i64>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct IncomeMetrics {
    pub bhc: Option<f64>,
    pub ahc: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FloodRisk {
    pub level: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CrimeMetrics {
    pub count_12m: Option<i64>,
    pub rate_per_1k: Option<f64>,
    pub violent_12m: Option<i64>,
    pub burglary_12m: Option<i64>,
    pub robbery_12m: Option<i64>,
    pub band: Option<CrimeBand>,
    pub trend: Option<CrimeTrend>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CrimeBand {
    Excellent,
    Good,
    Mixed,
    Concerning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CrimeTrend {
    Improving,
    Stable,
    Worsening,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListingImage {
    pub remote: String,
    pub local: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RemoteLocalAsset {
    pub remote: Option<String>,
    pub local: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MapViews {
    pub satellite: RemoteLocalAsset,
    pub street: RemoteLocalAsset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EpcBand {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StationDistance {
    pub name: String,
    pub distance: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Lettings {
    pub available_date: Option<String>,
    pub deposit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Agent {
    pub name: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExtractionStatus {
    Success,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ListingStatus {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListingsFile {
    pub updated_at: String,
    pub search_urls: Vec<String>,
    pub locations: Vec<String>,
    pub last_search_total: i64,
    pub listings: Vec<Listing>,
}
