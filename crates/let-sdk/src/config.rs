#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, LetError, Result};
use crate::paths::paths;

pub const RIGHTMOVE_SEARCH_TYPES: [&str; 4] = ["detached", "semi-detached", "terraced", "flat"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Location {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilters {
    pub min_bedrooms: i64,
    pub max_bedrooms: i64,
    pub min_price: i64,
    pub max_price: i64,
    pub property_types: Vec<String>,
    pub include_let_agreed: bool,
    pub radius: f64,
    pub dont_show: Vec<String>,
    pub must_have: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchConfig {
    #[serde(rename = "useApi", default = "default_search_use_api")]
    pub use_api: bool,
    pub locations: Vec<Location>,
    pub filters: SearchFilters,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FetchConfig {
    pub delay_ms: u64,
    pub max_listings: usize,
    pub max_retries: usize,
    #[serde(default = "default_fetch_min_score")]
    pub min_score: u8,
    #[serde(default = "default_drop_new_below_min_score")]
    pub drop_new_below_min_score: bool,
    #[serde(default = "default_download_maps")]
    pub download_maps: bool,
    #[serde(default = "default_download_floorplan")]
    pub download_floorplan: bool,
    #[serde(default = "default_download_epc_asset")]
    pub download_epc_asset: bool,
    #[serde(default = "default_media_download_concurrency")]
    pub media_download_concurrency: usize,
    #[serde(default = "default_media_process_concurrency")]
    pub media_process_concurrency: usize,
    #[serde(default = "default_media_photo_landscape_width")]
    pub media_photo_landscape_width: u32,
    #[serde(default = "default_media_photo_landscape_height")]
    pub media_photo_landscape_height: u32,
    #[serde(default = "default_media_photo_portrait_width")]
    pub media_photo_portrait_width: u32,
    #[serde(default = "default_media_photo_portrait_height")]
    pub media_photo_portrait_height: u32,
    #[serde(default = "default_media_aux_width")]
    pub media_aux_width: u32,
    #[serde(default = "default_media_aux_height")]
    pub media_aux_height: u32,
    #[serde(default = "default_media_map_width")]
    pub media_map_width: u32,
    #[serde(default = "default_media_map_height")]
    pub media_map_height: u32,
    #[serde(default = "default_media_quality_photo")]
    pub media_quality_photo: u8,
    #[serde(default = "default_media_quality_aux")]
    pub media_quality_aux: u8,
    #[serde(default = "default_media_quality_map")]
    pub media_quality_map: u8,
    #[serde(default = "default_media_timeout_ms")]
    pub media_timeout_ms: u64,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            delay_ms: 250,
            max_listings: 100,
            max_retries: 3,
            min_score: default_fetch_min_score(),
            drop_new_below_min_score: default_drop_new_below_min_score(),
            download_maps: default_download_maps(),
            download_floorplan: default_download_floorplan(),
            download_epc_asset: default_download_epc_asset(),
            media_download_concurrency: default_media_download_concurrency(),
            media_process_concurrency: default_media_process_concurrency(),
            media_photo_landscape_width: default_media_photo_landscape_width(),
            media_photo_landscape_height: default_media_photo_landscape_height(),
            media_photo_portrait_width: default_media_photo_portrait_width(),
            media_photo_portrait_height: default_media_photo_portrait_height(),
            media_aux_width: default_media_aux_width(),
            media_aux_height: default_media_aux_height(),
            media_map_width: default_media_map_width(),
            media_map_height: default_media_map_height(),
            media_quality_photo: default_media_quality_photo(),
            media_quality_aux: default_media_quality_aux(),
            media_quality_map: default_media_quality_map(),
            media_timeout_ms: default_media_timeout_ms(),
        }
    }
}

const fn default_search_use_api() -> bool {
    true
}

const fn default_fetch_min_score() -> u8 {
    70
}

const fn default_drop_new_below_min_score() -> bool {
    true
}

const fn default_download_maps() -> bool {
    true
}

const fn default_download_floorplan() -> bool {
    true
}

const fn default_download_epc_asset() -> bool {
    true
}

const fn default_media_download_concurrency() -> usize {
    4
}

const fn default_media_process_concurrency() -> usize {
    2
}

const fn default_media_photo_landscape_width() -> u32 {
    1200
}

const fn default_media_photo_landscape_height() -> u32 {
    900
}

const fn default_media_photo_portrait_width() -> u32 {
    900
}

const fn default_media_photo_portrait_height() -> u32 {
    1200
}

const fn default_media_aux_width() -> u32 {
    1200
}

const fn default_media_aux_height() -> u32 {
    900
}

const fn default_media_map_width() -> u32 {
    1200
}

const fn default_media_map_height() -> u32 {
    1200
}

const fn default_media_quality_photo() -> u8 {
    82
}

const fn default_media_quality_aux() -> u8 {
    85
}

const fn default_media_quality_map() -> u8 {
    85
}

const fn default_media_timeout_ms() -> u64 {
    20_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompositeWeights {
    pub affordability: f64,
    pub location: f64,
    pub liveability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeatingCosts {
    #[serde(rename = "A")]
    pub a: f64,
    #[serde(rename = "B")]
    pub b: f64,
    #[serde(rename = "C")]
    pub c: f64,
    #[serde(rename = "D")]
    pub d: f64,
    #[serde(rename = "E")]
    pub e: f64,
    #[serde(rename = "F")]
    pub f: f64,
    #[serde(rename = "G")]
    pub g: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AffordabilityConfig {
    pub price_weight: f64,
    pub epc_weight: f64,
    pub heating_costs: HeatingCosts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocationScoringConfig {
    pub station_weight: f64,
    pub broadband_weight: f64,
    pub priority_weight: f64,
    pub imd_weight: f64,
    pub crime_weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GardenScores {
    pub private: f64,
    pub shared: f64,
    pub none: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeatingScores {
    pub gas: f64,
    pub electric: f64,
    pub unknown: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveabilityConfig {
    pub garden_weight: f64,
    pub heating_weight: f64,
    pub property_type_weight: f64,
    pub garden: GardenScores,
    pub heating: HeatingScores,
    pub property_type: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PenaltyConfig {
    pub epc_f: f64,
    pub epc_g: f64,
    pub no_garden: f64,
    pub no_pets: f64,
    pub deprivation: f64,
    pub deprivation_threshold: i64,
    pub high_crime: f64,
    pub high_crime_threshold: f64,
    pub missing_data_penalty: f64,
    #[serde(default = "default_garden_required")]
    pub garden_required: bool,
}

fn default_garden_required() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScoringConfig {
    pub adaptiveness: f64,
    pub adaptiveness_factor: f64,
    pub weights: CompositeWeights,
    pub affordability: AffordabilityConfig,
    pub location: LocationScoringConfig,
    pub liveability: LiveabilityConfig,
    pub penalties: PenaltyConfig,
    pub region_priority: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub search: SearchConfig,
    pub fetch: FetchConfig,
    pub scoring: ScoringConfig,
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

fn validate_weight_01(name: &str, value: f64) -> Result<()> {
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(LetError::new(
            ErrorCode::InvalidInput,
            format!("invalid weight for {name}: {value}"),
            "weights must be between 0 and 1",
        ))
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        if self.search.locations.is_empty() {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "search.locations must include at least one location",
                "add at least one location id/name pair in config",
            ));
        }

        if self.fetch.delay_ms == 0 || self.fetch.max_listings == 0 || self.fetch.max_retries == 0 {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "fetch config values must be positive",
                "set delayMs, maxListings, maxRetries to positive values",
            ));
        }
        if self.fetch.min_score > 100 {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                format!(
                    "fetch.minScore must be in range 0-100 (got {})",
                    self.fetch.min_score
                ),
                "set fetch.minScore between 0 and 100",
            ));
        }
        if self.fetch.media_download_concurrency == 0 || self.fetch.media_process_concurrency == 0 {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "fetch media concurrency must be positive",
                "set fetch.mediaDownloadConcurrency and fetch.mediaProcessConcurrency to positive values",
            ));
        }
        if self.fetch.media_photo_landscape_width == 0
            || self.fetch.media_photo_landscape_height == 0
            || self.fetch.media_photo_portrait_width == 0
            || self.fetch.media_photo_portrait_height == 0
            || self.fetch.media_aux_width == 0
            || self.fetch.media_aux_height == 0
            || self.fetch.media_map_width == 0
            || self.fetch.media_map_height == 0
        {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "fetch media dimensions must be positive",
                "set fetch media dimension fields to values greater than zero",
            ));
        }
        if !(40..=95).contains(&self.fetch.media_quality_photo)
            || !(40..=95).contains(&self.fetch.media_quality_aux)
            || !(40..=95).contains(&self.fetch.media_quality_map)
        {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "fetch media JPEG quality must be between 40 and 95",
                "set fetch.mediaQualityPhoto/mediaQualityAux/mediaQualityMap to 40-95",
            ));
        }
        if self.fetch.media_timeout_ms == 0 {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "fetch.mediaTimeoutMs must be positive",
                "set fetch.mediaTimeoutMs to a value greater than zero",
            ));
        }

        for property_type in &self.search.filters.property_types {
            if !RIGHTMOVE_SEARCH_TYPES.contains(&property_type.as_str()) {
                return Err(LetError::new(
                    ErrorCode::InvalidInput,
                    format!("unsupported property type: {property_type}"),
                    "use detached, semi-detached, terraced, or flat",
                ));
            }
        }

        let weights = &self.scoring.weights;
        validate_weight_01("scoring.weights.affordability", weights.affordability)?;
        validate_weight_01("scoring.weights.location", weights.location)?;
        validate_weight_01("scoring.weights.liveability", weights.liveability)?;
        if !approx_eq(
            weights.affordability + weights.location + weights.liveability,
            1.0,
        ) {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "composite scoring weights must sum to 1.0",
                "adjust affordability/location/liveability weights",
            ));
        }

        let affordability = &self.scoring.affordability;
        validate_weight_01(
            "scoring.affordability.priceWeight",
            affordability.price_weight,
        )?;
        validate_weight_01("scoring.affordability.epcWeight", affordability.epc_weight)?;
        if !approx_eq(affordability.price_weight + affordability.epc_weight, 1.0) {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "affordability weights must sum to 1.0",
                "adjust priceWeight and epcWeight",
            ));
        }

        let location = &self.scoring.location;
        validate_weight_01("scoring.location.stationWeight", location.station_weight)?;
        validate_weight_01(
            "scoring.location.broadbandWeight",
            location.broadband_weight,
        )?;
        validate_weight_01("scoring.location.priorityWeight", location.priority_weight)?;
        validate_weight_01("scoring.location.imdWeight", location.imd_weight)?;
        validate_weight_01("scoring.location.crimeWeight", location.crime_weight)?;
        if !approx_eq(
            location.station_weight
                + location.broadband_weight
                + location.priority_weight
                + location.imd_weight
                + location.crime_weight,
            1.0,
        ) {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "location weights must sum to 1.0",
                "adjust location station/broadband/priority/imd/crime weights",
            ));
        }

        let live = &self.scoring.liveability;
        validate_weight_01("scoring.liveability.gardenWeight", live.garden_weight)?;
        validate_weight_01("scoring.liveability.heatingWeight", live.heating_weight)?;
        validate_weight_01(
            "scoring.liveability.propertyTypeWeight",
            live.property_type_weight,
        )?;
        if !approx_eq(
            live.garden_weight + live.heating_weight + live.property_type_weight,
            1.0,
        ) {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                "liveability weights must sum to 1.0",
                "adjust garden/heating/propertyType weights",
            ));
        }

        Ok(())
    }
}

pub fn default_scoring_config() -> ScoringConfig {
    ScoringConfig {
        adaptiveness: 2.0,
        adaptiveness_factor: 10.0,
        weights: CompositeWeights {
            affordability: 0.4,
            location: 0.3,
            liveability: 0.3,
        },
        affordability: AffordabilityConfig {
            price_weight: 1.0,
            epc_weight: 0.0,
            heating_costs: HeatingCosts {
                a: 30.0,
                b: 45.0,
                c: 70.0,
                d: 100.0,
                e: 400.0,
                f: 450.0,
                g: 500.0,
            },
        },
        location: LocationScoringConfig {
            station_weight: 0.2,
            broadband_weight: 0.2,
            priority_weight: 0.2,
            imd_weight: 0.2,
            crime_weight: 0.2,
        },
        liveability: LiveabilityConfig {
            garden_weight: 0.45,
            heating_weight: 0.3,
            property_type_weight: 0.25,
            garden: GardenScores {
                private: 100.0,
                shared: 40.0,
                none: 0.0,
            },
            heating: HeatingScores {
                gas: 100.0,
                electric: 60.0,
                unknown: 30.0,
            },
            property_type: BTreeMap::from([
                ("detached".to_string(), 95.0),
                ("house".to_string(), 95.0),
                ("semi-detached".to_string(), 90.0),
                ("terraced".to_string(), 85.0),
                ("cottage".to_string(), 85.0),
                ("bungalow".to_string(), 80.0),
                ("flat".to_string(), 65.0),
                ("apartment".to_string(), 65.0),
                ("studio".to_string(), 40.0),
            ]),
        },
        penalties: PenaltyConfig {
            epc_f: 0.0,
            epc_g: 0.0,
            no_garden: 0.5,
            no_pets: 0.4,
            deprivation: 0.75,
            deprivation_threshold: 2,
            high_crime: 0.8,
            high_crime_threshold: 120.0,
            missing_data_penalty: 0.95,
            garden_required: false,
        },
        region_priority: BTreeMap::from([
            ("York".to_string(), 95.0),
            ("Durham".to_string(), 90.0),
            ("Stamford".to_string(), 90.0),
            ("Brighton".to_string(), 85.0),
            ("Harrogate".to_string(), 85.0),
            ("Newcastle".to_string(), 80.0),
            ("Liverpool".to_string(), 80.0),
            ("Morpeth".to_string(), 80.0),
            ("Lancaster".to_string(), 75.0),
            ("Folkestone".to_string(), 75.0),
            ("Leicester".to_string(), 75.0),
            ("Nottingham".to_string(), 70.0),
            ("Sheffield".to_string(), 70.0),
            ("Swansea".to_string(), 70.0),
            ("Leeds".to_string(), 65.0),
            ("Manchester".to_string(), 65.0),
        ]),
    }
}

fn apply_search_scoring(mut config: AppConfig) -> AppConfig {
    let required = config
        .search
        .filters
        .must_have
        .iter()
        .any(|feature| feature.eq_ignore_ascii_case("garden"));
    config.scoring.penalties.garden_required = required;
    config
}

pub fn parse_scoring_config(raw: &toml::Value) -> ScoringConfig {
    match raw.get("scoring").cloned() {
        Some(value) => toml::Value::try_into(value).unwrap_or_else(|_| default_scoring_config()),
        None => default_scoring_config(),
    }
}

pub fn load_config(config_path: Option<&Path>) -> Result<AppConfig> {
    let path: PathBuf = config_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths().derived.config_file);

    let text = std::fs::read_to_string(&path).map_err(|err| {
        LetError::new(
            ErrorCode::NoConfig,
            format!("failed to read config file {}: {err}", path.display()),
            "create or fix let.config.toml",
        )
    })?;

    let raw: toml::Value = toml::from_str(&text).map_err(|err| {
        LetError::new(
            ErrorCode::InvalidInput,
            format!("invalid config TOML {}: {err}", path.display()),
            "fix config syntax and required fields",
        )
    })?;

    reject_legacy_fetch_use_api(&raw, &path)?;

    let parsed: AppConfig = raw.try_into().map_err(|err| {
        LetError::new(
            ErrorCode::InvalidInput,
            format!("invalid config TOML {}: {err}", path.display()),
            "fix config syntax and required fields",
        )
    })?;

    let parsed = apply_search_scoring(parsed);
    parsed.validate()?;
    Ok(parsed)
}

fn reject_legacy_fetch_use_api(raw: &toml::Value, path: &Path) -> Result<()> {
    let has_legacy_key = raw
        .get("fetch")
        .and_then(toml::Value::as_table)
        .map(|fetch| fetch.contains_key("useApi"))
        .unwrap_or(false);

    if has_legacy_key {
        return Err(LetError::new(
            ErrorCode::InvalidInput,
            format!(
                "invalid config TOML {}: fetch.useApi is no longer supported",
                path.display()
            ),
            "rename fetch.useApi to search.useApi",
        ));
    }

    Ok(())
}

pub fn load_scoring_config(config_path: Option<&Path>) -> ScoringConfig {
    let path: PathBuf = config_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths().derived.config_file);

    match std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
    {
        Some(raw) => parse_scoring_config(&raw),
        None => default_scoring_config(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::ErrorCode;

    use super::{
        AppConfig, FetchConfig, Location, SearchConfig, SearchFilters, default_scoring_config,
        load_config, load_scoring_config, parse_scoring_config,
    };

    #[test]
    fn default_weights_sum_to_one() {
        let cfg = default_scoring_config();
        let total = cfg.weights.affordability + cfg.weights.location + cfg.weights.liveability;
        assert!((total - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_scoring_falls_back_to_default() {
        let raw = toml::from_str("search = {}").expect("valid toml value");
        let cfg = parse_scoring_config(&raw);
        assert_eq!(cfg.adaptiveness, default_scoring_config().adaptiveness);
    }

    #[test]
    fn scoring_loader_falls_back_without_file() {
        let cfg = load_scoring_config(Some(std::path::Path::new("/tmp/missing-let-config.toml")));
        assert_eq!(cfg.adaptiveness_factor, 10.0);
    }

    #[test]
    fn config_validation_requires_locations() {
        let mut config = AppConfig {
            search: super::SearchConfig {
                use_api: true,
                locations: vec![],
                filters: super::SearchFilters {
                    min_bedrooms: 1,
                    max_bedrooms: 4,
                    min_price: 1000,
                    max_price: 4000,
                    property_types: vec!["flat".to_string()],
                    include_let_agreed: false,
                    radius: 3.0,
                    dont_show: vec![],
                    must_have: vec![],
                },
            },
            fetch: super::FetchConfig {
                delay_ms: 250,
                max_listings: 100,
                max_retries: 3,
                ..super::FetchConfig::default()
            },
            scoring: default_scoring_config(),
        };

        assert!(config.validate().is_err());
        config.search.locations.push(super::Location {
            id: "REGION^87490".to_string(),
            name: "Manchester".to_string(),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn load_config_reads_requested_path_each_time() {
        let temp = TempDir::new().expect("tempdir");
        let path_a = temp.path().join("a.toml");
        let path_b = temp.path().join("b.toml");

        fs::write(
            &path_a,
            toml::to_string(&sample_config("Alpha")).expect("serialize config"),
        )
        .expect("write config a");
        fs::write(
            &path_b,
            toml::to_string(&sample_config("Beta")).expect("serialize config"),
        )
        .expect("write config b");

        let config_a = load_config(Some(&path_a)).expect("load config a");
        let config_b = load_config(Some(&path_b)).expect("load config b");

        assert_eq!(config_a.search.locations[0].name, "Alpha");
        assert_eq!(config_b.search.locations[0].name, "Beta");
    }

    #[test]
    fn load_config_defaults_search_use_api_to_true() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("config.toml");

        fs::write(
            &path,
            r#"
[search]
locations = [{ id = "REGION^1", name = "Alpha" }]

[search.filters]
minBedrooms = 1
maxBedrooms = 2
minPrice = 700
maxPrice = 1400
propertyTypes = ["flat"]
includeLetAgreed = false
radius = 1
dontShow = []
mustHave = []

[fetch]
delayMs = 250
maxListings = 100
maxRetries = 3

[scoring]
adaptiveness = 2.0
adaptivenessFactor = 10

[scoring.weights]
affordability = 0.3
location = 0.4
liveability = 0.3

[scoring.affordability]
priceWeight = 1.0
epcWeight = 0.0

[scoring.affordability.heatingCosts]
A = 30
B = 45
C = 70
D = 100
E = 400
F = 450
G = 500

[scoring.location]
stationWeight = 0.2
broadbandWeight = 0.2
priorityWeight = 0.2
imdWeight = 0.2
crimeWeight = 0.2

[scoring.liveability]
gardenWeight = 0.4
heatingWeight = 0.3
propertyTypeWeight = 0.3

[scoring.liveability.garden]
private = 100
shared = 40
none = 0

[scoring.liveability.heating]
gas = 100
electric = 60
unknown = 30

[scoring.liveability.propertyType]
flat = 80

[scoring.penalties]
epcF = 0.0
epcG = 0.0
noGarden = 0.5
noPets = 0.9
deprivation = 0.75
deprivationThreshold = 2
highCrime = 0.8
highCrimeThreshold = 120
missingDataPenalty = 0.95

[scoring.regionPriority]
Alpha = 80
"#,
        )
        .expect("write config");

        let config = load_config(Some(&path)).expect("load config");
        assert!(config.search.use_api);
    }

    #[test]
    fn load_config_parses_search_use_api_false() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("config.toml");

        let mut config = sample_config("Alpha");
        config.search.use_api = false;
        fs::write(&path, toml::to_string(&config).expect("serialize config"))
            .expect("write config");

        let parsed = load_config(Some(&path)).expect("load config");
        assert!(!parsed.search.use_api);
    }

    #[test]
    fn load_config_rejects_legacy_fetch_use_api() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("config.toml");

        fs::write(
            &path,
            r#"
[search]
locations = [{ id = "REGION^1", name = "Alpha" }]

[search.filters]
minBedrooms = 1
maxBedrooms = 2
minPrice = 700
maxPrice = 1400
propertyTypes = ["flat"]
includeLetAgreed = false
radius = 1
dontShow = []
mustHave = []

[fetch]
useApi = false
delayMs = 250
maxListings = 100
maxRetries = 3

[scoring]
adaptiveness = 2.0
adaptivenessFactor = 10

[scoring.weights]
affordability = 0.3
location = 0.4
liveability = 0.3

[scoring.affordability]
priceWeight = 1.0
epcWeight = 0.0

[scoring.affordability.heatingCosts]
A = 30
B = 45
C = 70
D = 100
E = 400
F = 450
G = 500

[scoring.location]
stationWeight = 0.2
broadbandWeight = 0.2
priorityWeight = 0.2
imdWeight = 0.2
crimeWeight = 0.2

[scoring.liveability]
gardenWeight = 0.4
heatingWeight = 0.3
propertyTypeWeight = 0.3

[scoring.liveability.garden]
private = 100
shared = 40
none = 0

[scoring.liveability.heating]
gas = 100
electric = 60
unknown = 30

[scoring.liveability.propertyType]
flat = 80

[scoring.penalties]
epcF = 0.0
epcG = 0.0
noGarden = 0.5
noPets = 0.9
deprivation = 0.75
deprivationThreshold = 2
highCrime = 0.8
highCrimeThreshold = 120
missingDataPenalty = 0.95

[scoring.regionPriority]
Alpha = 80
"#,
        )
        .expect("write config");

        let error = load_config(Some(&path)).expect_err("legacy key should fail");
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("fetch.useApi"));
        assert_eq!(error.hint, "rename fetch.useApi to search.useApi");
    }

    fn sample_config(name: &str) -> AppConfig {
        AppConfig {
            search: SearchConfig {
                use_api: true,
                locations: vec![Location {
                    id: format!("REGION^{name}"),
                    name: name.to_owned(),
                }],
                filters: SearchFilters {
                    min_bedrooms: 1,
                    max_bedrooms: 2,
                    min_price: 700,
                    max_price: 1400,
                    property_types: vec!["flat".to_owned()],
                    include_let_agreed: false,
                    radius: 1.0,
                    dont_show: Vec::new(),
                    must_have: Vec::new(),
                },
            },
            fetch: FetchConfig::default(),
            scoring: default_scoring_config(),
        }
    }
}
