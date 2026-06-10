use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

use let_sdk::config::{
    AppConfig, FetchConfig, Location, SearchConfig, SearchFilters, default_scoring_config,
};
use let_sdk::schema::listing::{
    Agent, AreaCodeName, AreaMetrics, CrimeMetrics, ExtractionStatus, FloodRisk, GeoLocation,
    ImdMetrics, IncomeMetrics, Lettings, Listing, ListingImage, ListingStatus, MapViews, PinType,
    PortalIds, RemoteLocalAsset,
};
use let_sdk::{DbMeta, load_listings_file, upsert_listings};
use rusqlite::Connection;
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
                use_api: true,
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
                ..FetchConfig::default()
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
    let output = fixture.cmd().args(["tools"]).output().expect("run tools");
    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "tools should not write to stderr in default JSON mode"
    );

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["meta"]["tool"], "tools");
    assert_eq!(json["data"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(json["data"]["tools"].as_array().is_some());
    assert!(json["data"]["globalFlags"].as_array().is_some());
    assert_eq!(
        json["data"]["outputFormats"],
        serde_json::json!(["json", "toon"])
    );
    assert_eq!(json["data"]["defaultOutputFormat"], "json");
    let global_flags = json["data"]["globalFlags"]
        .as_array()
        .expect("global flags array");
    assert!(global_flags.iter().any(|flag| flag["name"] == "--toon"));
    assert!(!global_flags.iter().any(|flag| flag["name"] == "--text"));
}

#[test]
fn tools_toon_returns_decodable_catalog() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["tools", "--toon"])
        .output()
        .expect("run tools toon");
    assert_eq!(output.status.code(), Some(0));

    let value = assert_single_toon_envelope(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(value["meta"]["tool"], "tools");
    assert_eq!(
        value["data"]["outputFormats"],
        serde_json::json!(["json", "toon"])
    );
    assert!(value["data"]["tools"].as_array().is_some());
}

#[test]
fn bare_invocation_prints_root_help() {
    let fixture = Fixture::new();
    let output = fixture.cmd().output().expect("run bare let");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("Commands:"));
}

#[test]
fn group_invocation_prints_group_help() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["view"])
        .output()
        .expect("run view group");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).expect("stdout utf-8");
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("detail"));
}

#[test]
fn view_list_json_reads_seeded_listing() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["view", "list", "--top", "5"])
        .output()
        .expect("run view list");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    let listings = json["data"]["listings"].as_array().expect("listings array");
    assert_eq!(listings.len(), 1);
}

#[test]
fn text_flag_is_rejected() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["view", "list", "--text"])
        .output()
        .expect("run view list with removed text flag");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(
        stderr.contains("--text"),
        "expected --text error, got: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn view_list_copy_keeps_json_stdout_and_copies_payload_json() {
    let fixture = Fixture::new();
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("clipboard-mock.sh");
    let capture_path = script_dir.path().join("clipboard.txt");
    write_clipboard_capture_script(&script_path);

    let output = fixture
        .cmd()
        .env("LET_CLIPBOARD_BIN", &script_path)
        .env("LET_CLIPBOARD_CAPTURE_PATH", &capture_path)
        .args(["view", "list", "--copy"])
        .output()
        .expect("run view list copy");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["filtered"], 1);
    assert_eq!(json["data"]["listings"].as_array().map(Vec::len), Some(1));

    let capture = fs::read_to_string(&capture_path).expect("read clipboard capture");
    let copied_json: serde_json::Value =
        serde_json::from_str(&capture).expect("parse copied list json");
    assert_eq!(copied_json["filtered"], 1);
    assert_eq!(copied_json["total"], 1);
    assert_eq!(copied_json["listings"].as_array().map(Vec::len), Some(1));
    assert!(
        copied_json.get("meta").is_none(),
        "clipboard should contain data payload, not envelope: {copied_json:?}"
    );
}

#[cfg(unix)]
#[test]
fn view_list_copy_with_toon_copies_payload_toon() {
    let fixture = Fixture::new();
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("clipboard-mock.sh");
    let capture_path = script_dir.path().join("clipboard.txt");
    write_clipboard_capture_script(&script_path);

    let output = fixture
        .cmd()
        .env("LET_CLIPBOARD_BIN", &script_path)
        .env("LET_CLIPBOARD_CAPTURE_PATH", &capture_path)
        .args(["view", "list", "--copy", "--toon"])
        .output()
        .expect("run view list copy toon");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_toon_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["filtered"], 1);

    let capture = fs::read_to_string(&capture_path).expect("read clipboard capture");
    assert!(
        serde_json::from_str::<serde_json::Value>(&capture).is_err(),
        "toon clipboard payload should not be raw JSON"
    );
    let copied: serde_json::Value =
        toon_format::decode_default(&capture).expect("decode copied Toon payload");
    assert_eq!(copied["filtered"], 1);
    assert_eq!(copied["total"], 1);
    assert_eq!(copied["listings"].as_array().map(Vec::len), Some(1));
    assert!(
        copied.get("meta").is_none(),
        "clipboard should contain data payload, not envelope: {copied:?}"
    );
}

#[cfg(unix)]
#[test]
fn view_list_copy_empty_result_copies_valid_empty_payload() {
    let fixture = Fixture::new();
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("clipboard-mock.sh");
    let capture_path = script_dir.path().join("clipboard.txt");
    write_clipboard_capture_script(&script_path);

    let output = fixture
        .cmd()
        .env("LET_CLIPBOARD_BIN", &script_path)
        .env("LET_CLIPBOARD_CAPTURE_PATH", &capture_path)
        .args(["view", "list", "--region", "Leeds", "--copy"])
        .output()
        .expect("run view list empty copy");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["filtered"], 0);

    let capture = fs::read_to_string(&capture_path).expect("read clipboard capture");
    let copied_json: serde_json::Value =
        serde_json::from_str(&capture).expect("parse copied empty list json");
    assert_eq!(copied_json["filtered"], 0);
    assert_eq!(copied_json["total"], 1);
    assert_eq!(copied_json["listings"].as_array().map(Vec::len), Some(0));
}

#[test]
fn view_detail_json_renders_listing_payload() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["view", "detail", "165432101"])
        .output()
        .expect("run view detail");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(
        json["data"]["listing"]["address"],
        "10 Example Street, Manchester"
    );
}

#[test]
fn tools_json_includes_view_list_copy_parameter() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["tools", "view.list"])
        .output()
        .expect("run tools view.list");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["tool"]["name"], "view.list");

    let parameters = json["data"]["tool"]["parameters"]
        .as_array()
        .expect("tool parameters array");
    assert!(
        parameters.iter().any(|value| {
            value["name"] == "--copy"
                && value["type"] == "bool"
                && value["description"]
                    .as_str()
                    .is_some_and(|description| description.contains("clipboard"))
        }),
        "expected --copy parameter in view.list metadata, got: {parameters:?}"
    );
}

#[cfg(unix)]
#[test]
fn view_detail_copy_keeps_json_stdout_and_copies_listing_json() {
    let fixture = Fixture::new();
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("clipboard-mock.sh");
    let capture_path = script_dir.path().join("clipboard.txt");
    write_clipboard_capture_script(&script_path);

    let output = fixture
        .cmd()
        .env("LET_CLIPBOARD_BIN", &script_path)
        .env("LET_CLIPBOARD_CAPTURE_PATH", &capture_path)
        .args(["view", "detail", "165432101", "--copy"])
        .output()
        .expect("run view detail copy");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(
        json["data"]["listing"]["address"],
        "10 Example Street, Manchester"
    );

    let capture = fs::read_to_string(&capture_path).expect("read clipboard capture");
    let copied_json: serde_json::Value =
        serde_json::from_str(&capture).expect("parse copied listing json");
    assert_eq!(copied_json["address"], "10 Example Street, Manchester");
    assert!(
        copied_json.get("meta").is_none(),
        "clipboard should contain listing payload, not envelope: {copied_json:?}"
    );
}

#[cfg(unix)]
#[test]
fn view_detail_copy_not_found_preserves_error_and_skips_clipboard_write() {
    let fixture = Fixture::new();
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("clipboard-mock.sh");
    let capture_path = script_dir.path().join("clipboard.txt");
    write_clipboard_capture_script(&script_path);

    let output = fixture
        .cmd()
        .env("LET_CLIPBOARD_BIN", &script_path)
        .env("LET_CLIPBOARD_CAPTURE_PATH", &capture_path)
        .args(["view", "detail", "missing-id", "--copy"])
        .output()
        .expect("run missing view detail copy");
    assert_eq!(output.status.code(), Some(1));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "not_found");
    assert!(
        !capture_path.exists(),
        "clipboard helper should not run when detail lookup fails"
    );
}

#[test]
fn config_show_exposes_search_use_api() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["config", "show"])
        .output()
        .expect("run config show");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["config"]["search"]["useApi"], true);
}

#[test]
fn config_validate_rejects_legacy_fetch_use_api_key() {
    let fixture = Fixture::new();
    fs::write(
        fixture.config_dir.join("let.config.toml"),
        r#"
[search]
locations = [{ id = "REGION^87490", name = "Manchester" }]

[search.filters]
minBedrooms = 1
maxBedrooms = 4
minPrice = 600
maxPrice = 2500
propertyTypes = ["flat", "terraced"]
includeLetAgreed = false
radius = 0
dontShow = []
mustHave = ["garden"]

[fetch]
useApi = false
delayMs = 1
maxListings = 100
maxRetries = 2

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
terraced = 85

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
Manchester = 70
"#,
    )
    .expect("write legacy config");

    let output = fixture
        .cmd()
        .args(["config", "validate"])
        .output()
        .expect("run config validate");
    assert_eq!(output.status.code(), Some(1));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "invalid_input");
    assert_eq!(
        json["error"]["hint"],
        "rename fetch.useApi to search.useApi"
    );
}

#[test]
fn search_diff_marks_known_and_new_ids() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["search", "diff", "165432101,999999999"])
        .output()
        .expect("run search diff");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    let known = json["data"]["known"].as_array().expect("known array");
    let new_ids = json["data"]["new"].as_array().expect("new array");
    assert_eq!(known.len(), 1);
    assert_eq!(new_ids.len(), 1);
    assert_eq!(known[0].as_str(), Some("165432101"));
    assert_eq!(new_ids[0].as_str(), Some("999999999"));
}

#[test]
fn unsupported_group_subcommands_return_json_envelope() {
    let cases = [
        ("search", vec!["search", "nope"]),
        ("config", vec!["config", "nope"]),
        ("view", vec!["view", "nope"]),
        ("assess", vec!["assess", "nope"]),
        ("score", vec!["score", "nope"]),
        ("build", vec!["build", "nope"]),
        ("ops", vec!["ops", "nope"]),
        ("export", vec!["export", "nope"]),
    ];

    for (group, args) in cases {
        let output = Command::new(bin_path())
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("run unsupported {group} subcommand: {error}"));

        assert_eq!(output.status.code(), Some(1));
        assert!(
            output.stderr.is_empty(),
            "unsupported {group} subcommand should not emit clap usage text"
        );

        let json = assert_single_json_envelope(&output);
        assert_eq!(json["ok"], false);
        assert_eq!(json["meta"]["tool"], "external");
        assert_eq!(json["error"]["code"], "unsupported_command");
        assert_eq!(
            json["error"]["message"],
            format!("{group} command is not supported: nope")
        );
    }
}

#[test]
fn search_diff_preserves_schema_mismatch_error_contract() {
    let fixture = Fixture::new();
    drop_score_contexts_table(&fixture.db_path);

    let output = fixture
        .cmd()
        .args(["search", "diff", "165432101,999999999"])
        .output()
        .expect("run search diff with mismatched schema");
    assert_eq!(output.status.code(), Some(2));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "schema_mismatch");
    assert!(
        json["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("score_contexts")),
        "expected score_contexts schema mismatch message, got: {json:?}"
    );
}

#[test]
fn health_marks_schema_mismatch_as_blocking() {
    let fixture = Fixture::new();
    drop_score_contexts_table(&fixture.db_path);

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["status"], "blocked");

    let checks = json["data"]["checks"].as_array().expect("checks array");
    let database = checks
        .iter()
        .find(|check| check["id"] == "database")
        .expect("database check");
    assert_eq!(database["status"], "error");
    assert_eq!(database["severity"], "blocking");

    let fix = database["fix"]
        .as_array()
        .expect("database fix instructions");
    assert!(
        fix.iter().any(|item| {
            item.as_str()
                .is_some_and(|text| text.contains("run `let fetch <id>`"))
        }),
        "expected fetch recreation hint, got: {fix:?}"
    );
}

#[test]
fn health_marks_unopenable_database_as_degraded() {
    let fixture = Fixture::new();
    fs::remove_file(&fixture.db_path).expect("remove seeded database file");
    fs::create_dir(&fixture.db_path).expect("replace database with directory");

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["status"], "degraded");

    let checks = json["data"]["checks"].as_array().expect("checks array");
    let database = checks
        .iter()
        .find(|check| check["id"] == "database")
        .expect("database check");
    assert_eq!(database["status"], "error");
    assert_eq!(database["severity"], "degraded");
    assert!(
        database["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("sqlite error")),
        "expected sqlite error detail, got: {database:?}"
    );
}

#[test]
fn health_marks_schema_version_mismatch_as_blocking() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.db_path).expect("open listings db");
    connection
        .pragma_update(None, "user_version", 999)
        .expect("set mismatched user_version");
    drop(connection);

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    let checks = json["data"]["checks"].as_array().expect("checks array");
    let database = checks
        .iter()
        .find(|check| check["id"] == "database")
        .expect("database check");

    assert_eq!(database["severity"], "blocking");
    assert!(
        database["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("schema version mismatch")),
        "expected version mismatch detail, got: {database:?}"
    );
}

#[test]
fn health_accepts_epc_bearer_token() {
    let fixture = Fixture::new();
    fs::write(
        fixture.config_dir.join(".env"),
        "EPC_API_BEARER_TOKEN=bearer-secret\nNOTION_API_KEY=secret_xxx\n",
    )
    .expect("write env file");

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    let checks = json["data"]["checks"].as_array().expect("checks array");
    let epc_auth = checks
        .iter()
        .find(|check| check["id"] == "env.epc_auth")
        .expect("epc auth check");

    assert_eq!(epc_auth["status"], "ok");
    assert_eq!(epc_auth["severity"], "info");
    assert!(
        epc_auth["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("EPC_API_BEARER_TOKEN")),
        "expected bearer-token detail, got: {epc_auth:?}"
    );
}

#[test]
fn prune_dry_run_does_not_mutate_database() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["ops", "prune", "--min-score", "90", "--dry-run", "--force"])
        .output()
        .expect("run prune dry-run");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
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
        .args(["fetch", ",,,"])
        .output()
        .expect("run fetch");
    assert_eq!(output.status.code(), Some(1));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "validation_error");
}

#[test]
fn assess_submit_invalid_payload_returns_error_envelope() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args([
            "assess",
            "submit",
            "165432101",
            r#"{"maintenance":"invalid","scoreAdjustment":99}"#,
        ])
        .output()
        .expect("run assess submit");

    assert_eq!(output.status.code(), Some(1));

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "validation_error");

    let details = json["error"]["details"]
        .as_array()
        .expect("error details array");
    assert!(
        details.iter().any(|item| item["path"] == "maintenance"),
        "expected maintenance validation detail, got: {details:?}"
    );
    assert!(
        details.iter().any(|item| item["path"] == "scoreAdjustment"),
        "expected scoreAdjustment validation detail, got: {details:?}"
    );
}

#[test]
fn assess_candidates_returns_unassessed_active_listing() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["assess", "candidates", "--top", "5"])
        .output()
        .expect("run assess candidates");

    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    let candidates = json["data"]["candidates"]
        .as_array()
        .expect("candidates array");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["portalId"], "165432101");
}

#[test]
fn ops_patch_re_enriches_from_sources_by_default() {
    let fixture = Fixture::new();
    seed_minimal_sources(&fixture.sources_dir);
    clear_listing_enrichment(&fixture);

    let output = fixture
        .cmd()
        .args([
            "ops",
            "patch",
            "165432101",
            "--address",
            "10 Example Street, Manchester (updated)",
        ])
        .output()
        .expect("run ops patch");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    let re_enriched = json["data"]["reEnriched"]
        .as_array()
        .expect("re-enriched array");
    assert!(
        re_enriched.iter().any(|item| item == "gigabitAvailability"),
        "expected gigabitAvailability in re-enriched fields, got: {re_enriched:?}"
    );
    assert!(
        re_enriched.iter().any(|item| item == "lsoaCode"),
        "expected lsoaCode in re-enriched fields, got: {re_enriched:?}"
    );

    let after = load_listings_file(&fixture.db_path).expect("reload listings");
    let listing = after.listings.first().expect("listing exists");
    assert_eq!(listing.gigabit_availability, Some(88.0));
    assert_eq!(listing.area.lsoa.code.as_deref(), Some("E01005236"));
    assert_eq!(listing.area.msoa.code.as_deref(), Some("E02001052"));
}

#[test]
fn ops_patch_skip_re_enrich_keeps_source_fields_untouched() {
    let fixture = Fixture::new();
    seed_minimal_sources(&fixture.sources_dir);
    clear_listing_enrichment(&fixture);

    let output = fixture
        .cmd()
        .args([
            "ops",
            "patch",
            "165432101",
            "--address",
            "10 Example Street, Manchester (skip)",
            "--skip-re-enrich",
        ])
        .output()
        .expect("run ops patch");
    assert_eq!(output.status.code(), Some(0));

    let json = assert_single_json_envelope(&output);
    let re_enriched = json["data"]["reEnriched"]
        .as_array()
        .expect("re-enriched array");
    assert!(re_enriched.is_empty());

    let after = load_listings_file(&fixture.db_path).expect("reload listings");
    let listing = after.listings.first().expect("listing exists");
    assert_eq!(listing.gigabit_availability, None);
    assert_eq!(listing.area.lsoa.code, None);
    assert_eq!(listing.area.msoa.code, None);
}

#[test]
fn global_json_flag_is_rejected() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["--json", "tools"])
        .output()
        .expect("run tools with removed flag");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '--json'"),
        "expected clap to reject removed flag, got: {stderr}"
    );
}

#[test]
fn build_sources_list_returns_single_line_json_envelope_by_default() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["build", "sources", "list"])
        .output()
        .expect("run build sources list");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "build sources list should not write to stderr in default JSON mode"
    );

    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["meta"]["tool"], "build.sources");
    assert!(json["data"]["sources"].as_array().is_some());
    assert_eq!(json["data"]["defaultJobs"], 3);
}

#[test]
fn toon_flag_switches_to_toon_envelope() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["--toon", "build", "sources", "list"])
        .output()
        .expect("run build sources list with toon mode");

    assert_eq!(output.status.code(), Some(0));

    let value = assert_single_toon_envelope(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(value["meta"]["tool"], "build.sources");
    assert!(value["data"]["sources"].as_array().is_some());
}

#[cfg(unix)]
#[test]
fn start_passes_resolved_paths_to_child_tui() {
    let fixture = Fixture::new();
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("let-tui-mock.sh");
    let capture_path = script_dir.path().join("captured-env.txt");

    fs::write(
        &script_path,
        r#"#!/bin/sh
{
  printf 'LET_DATA_DIR=%s\n' "$LET_DATA_DIR"
  printf 'LET_CONFIG_DIR=%s\n' "$LET_CONFIG_DIR"
  printf 'LET_CACHE_DIR=%s\n' "$LET_CACHE_DIR"
  printf 'LET_SOURCES_DIR=%s\n' "$LET_SOURCES_DIR"
} > "$LET_TUI_CAPTURE_PATH"
"#,
    )
    .expect("write mock tui script");

    let chmod = Command::new("chmod")
        .args(["+x", script_path.to_str().expect("script path str")])
        .status()
        .expect("chmod script");
    assert!(chmod.success(), "chmod should succeed");

    let output = fixture
        .cmd()
        .env("LET_TUI_BIN", &script_path)
        .env("LET_TUI_CAPTURE_PATH", &capture_path)
        .args(["start"])
        .output()
        .expect("run start");

    assert_eq!(output.status.code(), Some(0));
    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], true);
    assert_eq!(json["meta"]["tool"], "start");

    let captured = fs::read_to_string(&capture_path).expect("read captured env");
    assert!(
        captured.contains(&format!("LET_DATA_DIR={}", fixture.data_dir.display())),
        "expected data dir in child env, got: {captured}"
    );
    assert!(
        captured.contains(&format!("LET_CONFIG_DIR={}", fixture.config_dir.display())),
        "expected config dir in child env, got: {captured}"
    );
    assert!(
        captured.contains(&format!("LET_CACHE_DIR={}", fixture.cache_dir.display())),
        "expected cache dir in child env, got: {captured}"
    );
    assert!(
        captured.contains(&format!(
            "LET_SOURCES_DIR={}",
            fixture.sources_dir.display()
        )),
        "expected sources dir in child env, got: {captured}"
    );
}

#[test]
fn prune_requires_force_in_non_interactive_mode() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["ops", "prune", "--min-score", "90"])
        .output()
        .expect("run prune without force");

    assert_eq!(output.status.code(), Some(1));
    let json = assert_single_json_envelope(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["error"]["code"], "validation_error");
}

#[test]
fn json_envelope_helper_accepts_single_stdout_line() {
    let json = assert_single_json_envelope_stdout("{\"ok\":true}\n");
    assert_eq!(json["ok"], true);
}

#[test]
fn json_envelope_helper_rejects_extra_blank_stdout_lines() {
    let result = std::panic::catch_unwind(|| {
        assert_single_json_envelope_stdout("{\"ok\":true}\n\n");
    });

    assert!(
        result.is_err(),
        "helper should reject trailing blank stdout lines"
    );
}

fn assert_single_json_envelope(output: &Output) -> serde_json::Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");
    assert_single_json_envelope_stdout(&stdout)
}

fn assert_single_toon_envelope(output: &Output) -> serde_json::Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");
    let Some(line) = stdout.strip_suffix('\n') else {
        panic!("expected Toon envelope stdout to end with one newline, got: {stdout:?}");
    };
    assert!(!line.is_empty(), "expected Toon envelope on stdout");
    toon_format::decode_default(line).expect("decode Toon envelope")
}

fn assert_single_json_envelope_stdout(stdout: &str) -> serde_json::Value {
    let Some(line) = stdout.strip_suffix('\n') else {
        panic!("expected JSON envelope stdout to end with one newline, got: {stdout:?}");
    };

    assert!(!line.is_empty(), "expected JSON envelope on stdout");
    assert_eq!(
        line.find('\n'),
        None,
        "expected exactly one stdout line in JSON mode, got: {stdout:?}"
    );

    serde_json::from_str(line).expect("parse envelope")
}

#[cfg(unix)]
fn write_clipboard_capture_script(path: &Path) {
    fs::write(path, "#!/bin/sh\ncat > \"$LET_CLIPBOARD_CAPTURE_PATH\"\n")
        .expect("write clipboard capture script");
    make_executable(path);
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    let chmod = Command::new("chmod")
        .args(["+x", path.to_str().expect("path str")])
        .status()
        .expect("chmod path");
    assert!(
        chmod.success(),
        "chmod should succeed for {}",
        path.display()
    );
}

fn drop_score_contexts_table(db_path: &Path) {
    let connection = Connection::open(db_path).expect("open listings db");
    connection
        .execute_batch("DROP TABLE IF EXISTS score_contexts;")
        .expect("drop score_contexts table");
}

fn seed_minimal_sources(sources_dir: &Path) {
    let postcodes_db = Connection::open(sources_dir.join("postcodes.db")).expect("postcodes db");
    postcodes_db
        .execute_batch(
            "
            CREATE TABLE postcodes (
                postcode TEXT PRIMARY KEY,
                postcode_display TEXT,
                lsoa_code TEXT,
                lsoa_name TEXT,
                msoa_code TEXT,
                msoa_name TEXT
            );
            INSERT INTO postcodes (postcode, postcode_display, lsoa_code, lsoa_name, msoa_code, msoa_name)
            VALUES ('M11AA', 'M1 1AA', 'E01005236', 'Manchester 001A', 'E02001052', 'Manchester 001');
            ",
        )
        .expect("seed postcodes");

    let broadband_db = Connection::open(sources_dir.join("broadband.db")).expect("broadband db");
    broadband_db
        .execute_batch(
            "
            CREATE TABLE postcodes (
                postcode TEXT PRIMARY KEY,
                postcode_display TEXT,
                gigabit_availability REAL
            );
            INSERT INTO postcodes (postcode, postcode_display, gigabit_availability)
            VALUES ('M11AA', 'M1 1AA', 88.0);
            ",
        )
        .expect("seed broadband");
}

fn clear_listing_enrichment(fixture: &Fixture) {
    let mut data = load_listings_file(&fixture.db_path).expect("load listings");
    for listing in &mut data.listings {
        listing.gigabit_availability = None;
        listing.area.lsoa.code = None;
        listing.area.lsoa.name = None;
        listing.area.msoa.code = None;
        listing.area.msoa.name = None;
    }

    let meta = DbMeta {
        updated_at: "2026-03-02T00:00:00.000Z".to_owned(),
        last_search_total: data.last_search_total,
    };
    upsert_listings(
        &fixture.db_path,
        &[],
        &data.listings,
        &data.listings,
        &meta,
        &data.search_urls,
        &data.locations,
    )
    .expect("persist listing reset");
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
