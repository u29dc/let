use std::fs;
use std::path::PathBuf;
use std::process::Command;

use let_sdk::config::{
    AppConfig, FetchConfig, Location, SearchConfig, SearchFilters, default_scoring_config,
};
use let_sdk::schema::listing::{
    Agent, AreaCodeName, AreaMetrics, CrimeMetrics, ExtractionStatus, FloodRisk, GeoLocation,
    ImdMetrics, IncomeMetrics, Lettings, Listing, ListingImage, ListingStatus, MapViews, PinType,
    PortalIds, RemoteLocalAsset,
};
use let_sdk::{DbMeta, load_listings_file, upsert_listings};
use tempfile::TempDir;

fn bin_path() -> PathBuf {
    PathBuf::from(assert_cmd::cargo::cargo_bin!("let"))
}

struct Fixture {
    _temp: TempDir,
    data_dir: PathBuf,
    config_dir: PathBuf,
    cache_dir: PathBuf,
    sources_dir: PathBuf,
    db_path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().to_path_buf();
        let data_dir = root.join("data");
        let config_dir = root.join("config");
        let cache_dir = root.join("cache");
        let sources_dir = root.join("sources");

        fs::create_dir_all(&data_dir).expect("create data dir");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::create_dir_all(&cache_dir).expect("create cache dir");
        fs::create_dir_all(&sources_dir).expect("create sources dir");

        let config = AppConfig {
            search: SearchConfig {
                locations: vec![Location {
                    id: "REGION^87490".to_owned(),
                    name: "Manchester".to_owned(),
                }],
                filters: SearchFilters {
                    min_bedrooms: 1,
                    max_bedrooms: 4,
                    min_price: 600,
                    max_price: 2500,
                    property_types: vec!["flat".to_owned(), "terraced".to_owned()],
                    include_let_agreed: false,
                    radius: 0.0,
                    dont_show: vec![],
                    must_have: vec!["garden".to_owned()],
                },
            },
            fetch: FetchConfig {
                delay_ms: 1,
                max_listings: 100,
                max_retries: 2,
            },
            scoring: default_scoring_config(),
        };
        let config_text = toml::to_string(&config).expect("serialize config");
        fs::write(config_dir.join("let.config.toml"), config_text).expect("write config");

        let db_path = data_dir.join("let.db");
        let listing = sample_listing();
        upsert_listings(
            &db_path,
            std::slice::from_ref(&listing),
            &[],
            std::slice::from_ref(&listing),
            &DbMeta {
                updated_at: "2026-03-01T00:00:00.000Z".to_owned(),
                last_search_total: 1,
            },
            &["https://www.rightmove.co.uk/property-to-rent/find.html".to_owned()],
            &["Manchester".to_owned()],
        )
        .expect("seed listings db");

        Self {
            _temp: temp,
            data_dir,
            config_dir,
            cache_dir,
            sources_dir,
            db_path,
        }
    }

    fn cmd(&self) -> Command {
        let mut command = Command::new(bin_path());
        command.args([
            "--data-dir",
            self.data_dir.to_str().expect("data dir str"),
            "--config-dir",
            self.config_dir.to_str().expect("config dir str"),
            "--cache-dir",
            self.cache_dir.to_str().expect("cache dir str"),
            "--sources-dir",
            self.sources_dir.to_str().expect("sources dir str"),
        ]);
        command
    }
}

#[test]
fn tools_json_returns_catalog() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["--json", "tools"])
        .output()
        .expect("run tools");
    assert_eq!(output.status.code(), Some(0));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse envelope");
    assert_eq!(json["ok"], true);
    assert!(json["data"]["tools"].as_array().is_some());
}

#[test]
fn view_list_json_reads_seeded_listing() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["--json", "view", "list", "--top", "5"])
        .output()
        .expect("run view list");
    assert_eq!(output.status.code(), Some(0));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse envelope");
    let listings = json["data"]["listings"].as_array().expect("listings array");
    assert_eq!(listings.len(), 1);
}

#[test]
fn search_diff_marks_known_and_new_ids() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["--json", "search", "diff", "165432101,999999999"])
        .output()
        .expect("run search diff");
    assert_eq!(output.status.code(), Some(0));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse envelope");
    let known = json["data"]["known"].as_array().expect("known array");
    let new_ids = json["data"]["new"].as_array().expect("new array");
    assert_eq!(known.len(), 1);
    assert_eq!(new_ids.len(), 1);
}

#[test]
fn prune_dry_run_does_not_mutate_database() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args([
            "--json",
            "ops",
            "prune",
            "--min-score",
            "90",
            "--dry-run",
            "--force",
        ])
        .output()
        .expect("run prune dry-run");
    assert_eq!(output.status.code(), Some(0));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse envelope");
    assert_eq!(json["data"]["dryRun"], true);
    assert_eq!(json["data"]["removed"], 1);

    let after = load_listings_file(&fixture.db_path).expect("reload listings");
    assert_eq!(after.listings.len(), 1);
}

#[test]
fn fetch_with_empty_ids_returns_validation_error() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["--json", "fetch", ",,,"])
        .output()
        .expect("run fetch");
    assert_eq!(output.status.code(), Some(1));

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse envelope");
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "VALIDATION_ERROR");
}

fn sample_listing() -> Listing {
    Listing {
        id: "2d8ab4a6-7de1-4e3f-a4aa-9408f2112377".to_owned(),
        portal_ids: PortalIds {
            rightmove: Some("165432101".to_owned()),
            zoopla: None,
            onthemarket: None,
        },
        uprn: Some("100021345678".to_owned()),
        uprn_source: None,
        uprn_confidence: None,
        url: "https://www.rightmove.co.uk/properties/165432101".to_owned(),
        location: GeoLocation {
            lat: 53.4808,
            lng: -2.2426,
            pin_type: Some(PinType::AccuratePoint),
        },
        postcode: "M1 1AA".to_owned(),
        address: "10 Example Street, Manchester".to_owned(),
        region: Some("Manchester".to_owned()),
        google_maps_url: "https://maps.google.com/?q=53.4808,-2.2426".to_owned(),
        google_maps_street_view_url: "https://maps.google.com/?layer=c&cbll=53.4808,-2.2426"
            .to_owned(),
        area: AreaMetrics {
            lsoa: AreaCodeName {
                code: Some("E01005236".to_owned()),
                name: Some("Manchester 001A".to_owned()),
            },
            msoa: AreaCodeName {
                code: Some("E02001052".to_owned()),
                name: Some("Manchester 001".to_owned()),
            },
            imd: ImdMetrics {
                rank: Some(14000),
                decile: Some(5),
                score: Some(18.4),
            },
            income: IncomeMetrics {
                bhc: Some(35_200.0),
                ahc: Some(30_100.0),
            },
            social_housing_pct: Some(14.0),
            population: Some(9800),
            flood_risk: FloodRisk {
                level: Some("low".to_owned()),
                source: Some("ea".to_owned()),
            },
            crime: CrimeMetrics {
                count_12m: Some(240),
                rate_per_1k: Some(58.3),
                violent_12m: Some(30),
                burglary_12m: Some(11),
                robbery_12m: Some(3),
                band: None,
                trend: None,
                updated_at: Some("2026-01-31".to_owned()),
            },
        },
        price: 1450,
        price_display: "£1,450 pcm".to_owned(),
        bedrooms: 2,
        bathrooms: 1,
        property_type: "Flat".to_owned(),
        description: "Two-bedroom apartment with private balcony and shared garden.".to_owned(),
        notes: vec!["balcony".to_owned(), "shared garden".to_owned()],
        images: vec![ListingImage {
            remote: "https://media.rightmove.co.uk/image-1.jpg".to_owned(),
            local: None,
        }],
        floorplan: RemoteLocalAsset {
            remote: Some("https://media.rightmove.co.uk/floorplan.jpg".to_owned()),
            local: None,
        },
        epc: RemoteLocalAsset {
            remote: Some("https://epc.service.gov.uk/energy-certificate/abcd".to_owned()),
            local: None,
        },
        map_views: MapViews::default(),
        epc_rating: Some(let_sdk::schema::listing::EpcBand::C),
        floor_area_sqm: Some(72.0),
        epc_lodgement_date: None,
        epc_address_match: None,
        epc_search_url: None,
        nearest_stations: vec![let_sdk::schema::listing::StationDistance {
            name: "Piccadilly".to_owned(),
            distance: 0.4,
            unit: "miles".to_owned(),
        }],
        gigabit_availability: Some(95.0),
        listed_date: Some("2026-02-10".to_owned()),
        lettings: Lettings {
            available_date: Some("2026-03-01".to_owned()),
            deposit: Some(1673),
        },
        agent: Agent {
            name: Some("Example Agent".to_owned()),
            phone: Some("0161 000 0000".to_owned()),
        },
        assessment: None,
        assessed_at: None,
        assessed_score: None,
        scores: None,
        fetched_at: "2026-03-01T00:00:00.000Z".to_owned(),
        extraction_status: ExtractionStatus::Success,
        status: ListingStatus::Active,
        notion_page_id: None,
    }
}
