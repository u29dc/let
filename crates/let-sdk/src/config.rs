#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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
    pub locations: Vec<Location>,
    pub filters: SearchFilters,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FetchConfig {
    pub delay_ms: u64,
    pub max_listings: usize,
    pub max_retries: usize,
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
    pub garden_required: bool,
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

static CONFIG_CACHE: OnceLock<Mutex<Option<AppConfig>>> = OnceLock::new();

fn config_cache() -> &'static Mutex<Option<AppConfig>> {
    CONFIG_CACHE.get_or_init(|| Mutex::new(None))
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

pub fn reset_config_cache() {
    let mut guard = config_cache().lock().expect("config cache lock poisoned");
    *guard = None;
}

pub fn load_config(config_path: Option<&Path>) -> Result<AppConfig> {
    let mut guard = config_cache().lock().expect("config cache lock poisoned");
    if let Some(cfg) = guard.clone() {
        return Ok(cfg);
    }

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

    let parsed: AppConfig = toml::from_str(&text).map_err(|err| {
        LetError::new(
            ErrorCode::InvalidInput,
            format!("invalid config TOML {}: {err}", path.display()),
            "fix config syntax and required fields",
        )
    })?;

    let parsed = apply_search_scoring(parsed);
    parsed.validate()?;

    *guard = Some(parsed.clone());
    Ok(parsed)
}

pub fn load_scoring_config(config_path: Option<&Path>) -> ScoringConfig {
    let path: PathBuf = config_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths().derived.config_file);

    match std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.parse::<toml::Value>().ok())
    {
        Some(raw) => parse_scoring_config(&raw),
        None => default_scoring_config(),
    }
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, default_scoring_config, load_scoring_config, parse_scoring_config};

    #[test]
    fn default_weights_sum_to_one() {
        let cfg = default_scoring_config();
        let total = cfg.weights.affordability + cfg.weights.location + cfg.weights.liveability;
        assert!((total - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_scoring_falls_back_to_default() {
        let raw = "search = {}".parse().expect("valid toml value");
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
}
