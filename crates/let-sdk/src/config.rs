#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, LetError, Result};
use crate::paths::paths;
use crate::score::{ScorecardOverrideConfig, validate_scorecard_overrides};

pub const RIGHTMOVE_SEARCH_TYPES: [&str; 4] = ["detached", "semi-detached", "terraced", "flat"];
pub const PROFILE_DIR_NAME: &str = "profiles";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProfile {
    pub name: String,
    pub path: PathBuf,
}

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
pub struct AppConfig {
    pub search: SearchConfig,
    pub fetch: FetchConfig,
    #[serde(default)]
    pub scorecards: BTreeMap<String, ScorecardOverrideConfig>,
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

        validate_scorecard_overrides(&self.scorecards)?;

        Ok(())
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

    parsed.validate()?;
    Ok(parsed)
}

pub fn load_config_profile(
    default_config_path: Option<&Path>,
    profile: Option<&str>,
) -> Result<AppConfig> {
    let default_path = default_config_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| paths().derived.config_file);
    let config_path = config_path_for_profile(&default_path, profile)?;
    load_config(Some(&config_path))
}

pub fn config_profiles_dir(default_config_path: &Path) -> PathBuf {
    default_config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(PROFILE_DIR_NAME)
}

pub fn config_path_for_profile(
    default_config_path: &Path,
    profile: Option<&str>,
) -> Result<PathBuf> {
    let Some(profile) = profile else {
        return Ok(default_config_path.to_path_buf());
    };

    validate_profile_name(profile)?;
    Ok(config_profiles_dir(default_config_path).join(format!("{profile}.toml")))
}

pub fn list_config_profiles(default_config_path: &Path) -> Result<Vec<ConfigProfile>> {
    let profile_dir = config_profiles_dir(default_config_path);
    let entries = match fs::read_dir(&profile_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                format!(
                    "failed to read config profiles directory {}: {error}",
                    profile_dir.display()
                ),
                "check config directory permissions",
            ));
        }
    };

    let mut profiles = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            LetError::new(
                ErrorCode::InvalidInput,
                format!(
                    "failed to read config profiles directory {}: {error}",
                    profile_dir.display()
                ),
                "check config directory permissions",
            )
        })?;
        let path = entry.path();
        if !entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if validate_profile_name(name).is_err() {
            continue;
        }
        profiles.push(ConfigProfile {
            name: name.to_owned(),
            path,
        });
    }

    profiles.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(profiles)
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    let valid_len = (1..=64).contains(&name.len());
    let valid_chars = name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    let starts_with_alnum = name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric());
    let reserved = name.eq_ignore_ascii_case("default");

    if valid_len && valid_chars && starts_with_alnum && !reserved {
        return Ok(());
    }

    Err(LetError::new(
        ErrorCode::InvalidInput,
        format!("invalid config profile name `{name}`"),
        "use 1-64 ASCII letters, digits, hyphens, or underscores; do not use `default`",
    ))
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::TempDir;

    use crate::ErrorCode;

    use super::{
        AppConfig, FetchConfig, Location, SearchConfig, SearchFilters, config_path_for_profile,
        list_config_profiles, load_config, load_config_profile, validate_profile_name,
    };

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
            scorecards: BTreeMap::new(),
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
    fn load_config_profile_reads_profile_file() {
        let temp = TempDir::new().expect("tempdir");
        let default_path = temp.path().join("let.config.toml");
        let profile_dir = temp.path().join("profiles");
        let profile_path = profile_dir.join("north.toml");

        fs::create_dir_all(&profile_dir).expect("create profile dir");
        fs::write(
            &default_path,
            toml::to_string(&sample_config("Default")).expect("serialize default config"),
        )
        .expect("write default config");
        fs::write(
            &profile_path,
            toml::to_string(&sample_config("North")).expect("serialize profile config"),
        )
        .expect("write profile config");

        let default_config =
            load_config_profile(Some(&default_path), None).expect("load default config");
        let profile_config =
            load_config_profile(Some(&default_path), Some("north")).expect("load profile config");

        assert_eq!(default_config.search.locations[0].name, "Default");
        assert_eq!(profile_config.search.locations[0].name, "North");
    }

    #[test]
    fn list_config_profiles_returns_valid_toml_profiles_sorted() {
        let temp = TempDir::new().expect("tempdir");
        let default_path = temp.path().join("let.config.toml");
        let profile_dir = temp.path().join("profiles");

        fs::create_dir_all(&profile_dir).expect("create profile dir");
        fs::write(profile_dir.join("zeta.toml"), "").expect("write zeta profile");
        fs::write(profile_dir.join("alpha.toml"), "").expect("write alpha profile");
        fs::write(profile_dir.join("../ignored.toml"), "").expect("write sibling file");
        fs::write(profile_dir.join("not-toml.txt"), "").expect("write text file");

        let profiles = list_config_profiles(&default_path).expect("list profiles");
        let names = profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["alpha", "zeta"]);
        assert_eq!(profiles[0].path, profile_dir.join("alpha.toml"));
    }

    #[test]
    fn profile_name_validation_rejects_path_traversal_and_ambiguous_names() {
        for invalid in [
            "",
            "../north",
            "north/east",
            ".hidden",
            "-dash",
            "default",
            "north.toml",
        ] {
            let error = validate_profile_name(invalid).expect_err("profile name should fail");
            assert_eq!(error.code, ErrorCode::InvalidInput);
        }

        assert!(validate_profile_name("north_1").is_ok());
        let temp = TempDir::new().expect("tempdir");
        let default_path = temp.path().join("let.config.toml");
        let error = config_path_for_profile(&default_path, Some("../north"))
            .expect_err("invalid profile path should fail");
        assert_eq!(error.code, ErrorCode::InvalidInput);
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
"#,
        )
        .expect("write config");

        let config = load_config(Some(&path)).expect("load config");
        assert!(config.search.use_api);
    }

    #[test]
    fn load_config_parses_scorecard_overrides() {
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

[scorecards.default]
label = "Configured baseline"

[scorecards.default.weights]
location = 0.3

[scorecards.default.thresholds]
rentGoodPcm = 1500
rentHighPcm = 2200

[scorecards.default.judgment]
enabled = true
blend = 0.65
maxAdjustment = 11
"#,
        )
        .expect("write config");

        let config = load_config(Some(&path)).expect("load config with scorecard overrides");
        let scorecard = crate::score::resolve_scorecard("default", &config.scorecards)
            .expect("resolve default scorecard");

        assert_eq!(scorecard.label, "Configured baseline");
        assert_eq!(scorecard.weights.location, 0.3);
        assert_eq!(scorecard.thresholds.rent_high_pcm, 2200.0);
        assert!(scorecard.judgment.enabled);
        assert_eq!(scorecard.judgment.blend, 0.65);
        assert_eq!(scorecard.judgment.max_adjustment, 11.0);
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
            scorecards: BTreeMap::new(),
        }
    }
}
