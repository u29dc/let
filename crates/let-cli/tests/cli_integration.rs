use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;

use let_sdk::intelligence::{
    AddressCandidateEvidence, AddressEvidence, AssessmentRecord, BroadbandEvidence, ClaimEvidence,
    ConfidenceLevel, DescriptionEvidence, EvidenceBundle, FactEvidence, FactProvider, InspectDepth,
    IntelligenceDb, MediaEvidence, MediaItemEvidence, RefreshPolicy, RightmoveEvidence,
    SectionState, SectionStatus, SourceRef, VerificationEvidence, VerificationStatus,
};
use serde_json::{Value, json};
use tempfile::TempDir;

const RIGHTMOVE_SEARCH_API_HEALTH_URL_ENV: &str = "LET_RIGHTMOVE_SEARCH_API_HEALTH_URL";

fn bin_path() -> PathBuf {
    PathBuf::from(assert_cmd::cargo::cargo_bin!("let"))
}

struct Fixture {
    _temp: TempDir,
    data_dir: PathBuf,
    config_dir: PathBuf,
    cache_dir: PathBuf,
    sources_dir: PathBuf,
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
        fs::write(config_dir.join("let.config.toml"), sample_config()).expect("write config");

        Self {
            _temp: temp,
            data_dir,
            config_dir,
            cache_dir,
            sources_dir,
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
        command.env(
            RIGHTMOVE_SEARCH_API_HEALTH_URL_ENV,
            "http://127.0.0.1:9/api/_search",
        );
        command
    }

    fn db_path(&self) -> PathBuf {
        self.data_dir.join("let.db")
    }

    fn seed_bundle(&self) {
        self.seed_bundle_record(&sample_bundle());
    }

    fn seed_bundle_record(&self, bundle: &EvidenceBundle) {
        let mut db = IntelligenceDb::open(self.db_path()).expect("open intelligence db");
        db.save_bundle(bundle).expect("save bundle");
    }

    fn save_assessment(&self, id: &str, assessment: Value) {
        let db = IntelligenceDb::open(self.db_path()).expect("open intelligence db");
        db.save_assessment(id, assessment).expect("save assessment");
    }

    fn write_profile(&self, name: &str, config: &str) {
        let profile_dir = self.config_dir.join("profiles");
        fs::create_dir_all(&profile_dir).expect("create profile dir");
        fs::write(profile_dir.join(format!("{name}.toml")), config).expect("write profile");
    }

    fn seed_area_sources(&self) {
        seed_source_db(
            &self.sources_dir.join("postcodes.db"),
            "
            CREATE TABLE postcodes (
                postcode TEXT PRIMARY KEY,
                postcode_display TEXT,
                lat REAL,
                lng REAL,
                lsoa_code TEXT,
                lsoa_name TEXT,
                msoa_code TEXT,
                msoa_name TEXT
            );
            INSERT INTO postcodes (postcode, postcode_display, lat, lng, lsoa_code, lsoa_name, msoa_code, msoa_name)
            VALUES ('AA11AA', 'AA1 1AA', 51.5074, -0.1278, 'LSOA001', 'LSOA One', 'MSOA001', 'MSOA One');
            ",
        );
        seed_source_db(
            &self.sources_dir.join("broadband.db"),
            "
            CREATE TABLE postcodes (
                postcode TEXT PRIMARY KEY,
                postcode_display TEXT,
                outward TEXT,
                area TEXT,
                gigabit_availability REAL,
                pct_over_300mbps REAL,
                ufbb_availability REAL,
                sfbb_availability REAL
            );
            INSERT INTO postcodes (
                postcode, postcode_display, outward, area, gigabit_availability,
                pct_over_300mbps, ufbb_availability, sfbb_availability
            )
            VALUES ('AA11AA', 'AA1 1AA', 'AA1', 'Example Area', 87.4, 91.2, 96.0, 99.1);
            ",
        );
        seed_source_db(
            &self.sources_dir.join("deprivation.db"),
            "
            CREATE TABLE imd (
                lsoa_code TEXT PRIMARY KEY,
                rank INTEGER,
                decile INTEGER,
                score REAL
            );
            INSERT INTO imd (lsoa_code, rank, decile, score)
            VALUES ('LSOA001', 222, 9, 7.4);
            ",
        );
        seed_source_db(
            &self.sources_dir.join("census.db"),
            "
            CREATE TABLE tenure (
                lsoa_code TEXT PRIMARY KEY,
                social_housing_pct REAL
            );
            INSERT INTO tenure (lsoa_code, social_housing_pct)
            VALUES ('LSOA001', 14.5);
            ",
        );
        seed_source_db(
            &self.sources_dir.join("population.db"),
            "
            CREATE TABLE population (
                lsoa_code TEXT PRIMARY KEY,
                population INTEGER
            );
            INSERT INTO population (lsoa_code, population)
            VALUES ('LSOA001', 12000);
            ",
        );
        seed_source_db(
            &self.sources_dir.join("income.db"),
            "
            CREATE TABLE income (
                msoa_code TEXT PRIMARY KEY,
                income_bhc REAL,
                income_ahc REAL
            );
            INSERT INTO income (msoa_code, income_bhc, income_ahc)
            VALUES ('MSOA001', 611.2, 503.0);
            ",
        );
        seed_source_db(
            &self.sources_dir.join("flood.db"),
            "
            CREATE TABLE flood (
                postcode TEXT PRIMARY KEY,
                risk TEXT,
                source TEXT
            );
            INSERT INTO flood (postcode, risk, source)
            VALUES ('AA11AA', 'low', 'ea-postcode-risk');
            ",
        );
        seed_source_db(
            &self.sources_dir.join("crime.db"),
            "
            CREATE TABLE crime_12m (
                lsoa_code TEXT PRIMARY KEY,
                total INTEGER,
                violent INTEGER,
                burglary INTEGER,
                robbery INTEGER,
                month_end TEXT
            );
            INSERT INTO crime_12m (lsoa_code, total, violent, burglary, robbery, month_end)
            VALUES ('LSOA001', 120, 18, 9, 3, '2026-02');
            ",
        );
        seed_source_db(
            &self.sources_dir.join("naptan.db"),
            "
            CREATE TABLE stops (
                atco_code TEXT PRIMARY KEY,
                naptan_code TEXT,
                common_name TEXT,
                stop_type TEXT,
                lat REAL,
                lng REAL
            );
            INSERT INTO stops (atco_code, naptan_code, common_name, stop_type, lat, lng)
            VALUES ('stop-1', 'nap-1', 'Central Rail Station', 'RLY', 51.50745, -0.12775);
            ",
        );
        seed_source_db(
            &self.sources_dir.join("uprn.db"),
            "
            CREATE TABLE uprn (
                uprn TEXT PRIMARY KEY,
                lat REAL,
                lng REAL,
                x REAL,
                y REAL
            );
            INSERT INTO uprn (uprn, lat, lng, x, y)
            VALUES ('100021234567', 51.50741, -0.12779, NULL, NULL);
            ",
        );
    }
}

fn seed_source_db(path: &PathBuf, sql: &str) {
    let connection = rusqlite::Connection::open(path).expect("open source db");
    connection.execute_batch(sql).expect("seed source db");
}

#[test]
fn tools_json_returns_agent_native_catalog() {
    let fixture = Fixture::new();
    let output = fixture.cmd().args(["tools"]).output().expect("run tools");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["meta"]["tool"], "tools");

    let tools = envelope["data"]["tools"].as_array().expect("tools array");
    for expected in [
        "inspect",
        "evidence",
        "verify",
        "correct.address",
        "correct.epc",
        "correct.media",
        "correct.clear",
        "area.postcode",
        "config.show",
        "config.profiles",
        "assess.save",
        "assess.get",
        "assess.list",
        "evidence.list",
        "sources.list",
        "sources.status",
        "sources.build",
        "start",
    ] {
        assert!(
            tools.iter().any(|tool| tool["name"] == expected),
            "expected tool {expected}"
        );
    }
    for removed in [
        "fetch",
        "view.list",
        "score.compute",
        "ops.patch",
        "export.json",
    ] {
        assert!(
            tools.iter().all(|tool| tool["name"] != removed),
            "removed tool {removed} should not be advertised"
        );
    }
    assert!(tools.iter().all(|tool| tool["inputSchema"].is_string()));
    assert!(tools.iter().all(|tool| tool["outputSchema"].is_string()));
}

#[test]
fn tools_detail_returns_inspect_contract() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["tools", "inspect"])
        .output()
        .expect("run tools inspect");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["data"]["tool"]["name"], "inspect");
    assert!(envelope["data"]["tool"]["inputSchema"].is_string());
}

#[test]
fn tools_toon_returns_decodable_catalog() {
    let fixture = Fixture::new();
    let json_output = fixture
        .cmd()
        .args(["tools"])
        .output()
        .expect("run tools json");
    assert_eq!(json_output.status.code(), Some(0));
    let json_envelope = assert_single_json_envelope(&json_output);

    let output = fixture
        .cmd()
        .args(["tools", "--toon"])
        .output()
        .expect("run tools toon");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_toon_envelope(&output);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["meta"]["tool"], "tools");
    assert_eq!(envelope["data"], json_envelope["data"]);
}

#[test]
fn bare_and_group_invocations_print_help() {
    let fixture = Fixture::new();
    let bare = fixture.cmd().output().expect("run bare let");
    assert_eq!(bare.status.code(), Some(0));
    assert!(
        String::from_utf8(bare.stdout)
            .expect("stdout")
            .contains("Commands:")
    );

    let group = fixture
        .cmd()
        .args(["sources"])
        .output()
        .expect("run sources");
    assert_eq!(group.status.code(), Some(0));
    let stdout = String::from_utf8(group.stdout).expect("stdout utf-8");
    assert!(stdout.contains("list"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("build"));
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

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "config.show");
    assert_eq!(envelope["data"]["config"]["search"]["useApi"], true);
    assert_eq!(envelope["data"]["profile"], serde_json::Value::Null);
}

#[test]
fn config_profiles_lists_available_profiles() {
    let fixture = Fixture::new();
    fixture.write_profile("zeta", sample_config());
    fixture.write_profile("alpha", sample_config());

    let output = fixture
        .cmd()
        .args(["config", "profiles"])
        .output()
        .expect("run config profiles");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "config.profiles");
    assert_eq!(envelope["meta"]["count"], 2);
    assert!(
        envelope["data"]["profileDir"]
            .as_str()
            .is_some_and(|path| path.ends_with("/profiles"))
    );
    let names = envelope["data"]["profiles"]
        .as_array()
        .expect("profiles array")
        .iter()
        .map(|profile| profile["name"].as_str().expect("profile name"))
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["alpha", "zeta"]);
}

#[test]
fn config_show_loads_named_profile() {
    let fixture = Fixture::new();
    fixture.write_profile(
        "north",
        &sample_config()
            .replace("REGION^87490", "REGION^NORTH")
            .replace("Manchester", "York"),
    );

    let output = fixture
        .cmd()
        .args(["config", "show", "--profile", "north"])
        .output()
        .expect("run config show profile");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["data"]["profile"], "north");
    assert!(
        envelope["data"]["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("/profiles/north.toml"))
    );
    assert_eq!(
        envelope["data"]["config"]["search"]["locations"][0]["name"],
        "York"
    );

    let global_output = fixture
        .cmd()
        .args(["--profile", "north", "config", "show"])
        .output()
        .expect("run global profile config show");
    assert_eq!(global_output.status.code(), Some(0));
    let global_envelope = assert_single_json_envelope(&global_output);
    assert_eq!(global_envelope["data"]["profile"], "north");
    assert_eq!(
        global_envelope["data"]["config"]["search"]["locations"][0]["name"],
        "York"
    );
}

#[test]
fn config_show_rejects_invalid_profile_name() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["config", "show", "--profile", "../north"])
        .output()
        .expect("run config show invalid profile");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "invalid_input");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("invalid config profile name"))
    );
}

#[test]
fn config_profile_is_used_by_health() {
    let fixture = Fixture::new();
    fixture.write_profile(
        "html",
        &sample_config().replace("useApi = true", "useApi = false"),
    );

    let output = fixture
        .cmd()
        .args(["--profile", "html", "health"])
        .output()
        .expect("run health with profile");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["data"]["paths"]["profile"], "html");
    assert!(
        envelope["data"]["paths"]["configFile"]
            .as_str()
            .is_some_and(|path| path.ends_with("/profiles/html.toml"))
    );
    let check = find_check(&envelope, "rightmove.search_api");
    assert_eq!(check["status"], "disabled");
}

#[test]
fn health_reports_missing_and_present_intelligence_db() {
    let fixture = Fixture::new();
    let missing = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(missing.status.code(), Some(0));
    let missing_json = assert_single_json_envelope(&missing);
    let database = find_check(&missing_json, "database");
    assert_eq!(database["label"], "Intelligence Database");
    assert_eq!(database["status"], "missing");
    assert!(
        database["fix"]
            .as_array()
            .expect("fix array")
            .iter()
            .any(|item| item
                .as_str()
                .is_some_and(|text| text.contains("let inspect")))
    );

    fixture.seed_bundle();
    let present = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(present.status.code(), Some(0));
    let present_json = assert_single_json_envelope(&present);
    let database = find_check(&present_json, "database");
    assert_eq!(database["status"], "ok");
    assert!(
        database["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("1 entities")
    );
}

#[test]
fn health_marks_schema_mismatch_as_blocking() {
    let fixture = Fixture::new();
    let connection = rusqlite::Connection::open(fixture.db_path()).expect("open db");
    connection
        .pragma_update(None, "user_version", 999)
        .expect("set schema version");
    drop(connection);

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));
    let envelope = assert_single_json_envelope(&output);
    let database = find_check(&envelope, "database");
    assert_eq!(database["severity"], "blocking");
    assert!(
        database["detail"]
            .as_str()
            .is_some_and(|text| text.contains("intelligence database schema version 999"))
    );
}

#[test]
fn health_marks_invalid_config_as_blocking() {
    let fixture = Fixture::new();
    let invalid_config = sample_config().replace(
        r#"[[search.locations]]
id = "REGION^87490"
name = "Manchester"

"#,
        "",
    );
    fs::write(fixture.config_dir.join("let.config.toml"), invalid_config)
        .expect("write invalid config");

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));
    let envelope = assert_single_json_envelope(&output);
    let config = find_check(&envelope, "config");
    assert_eq!(config["status"], "error");
    assert_eq!(config["severity"], "blocking");
    assert!(
        config["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("invalid config") || detail.contains("location"))
    );
}

#[test]
fn health_write_probe_does_not_remove_existing_file() {
    let fixture = Fixture::new();
    let sentinel = fixture.data_dir.join(".let-cli-healthcheck.tmp");
    fs::write(&sentinel, "keep me").expect("write sentinel");

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));

    let contents = fs::read_to_string(&sentinel).expect("read sentinel");
    assert_eq!(contents, "keep me");
}

#[test]
fn health_degrades_when_rightmove_search_api_returns_non_json() {
    let fixture = Fixture::new();
    let api_url = serve_search_api_response_once(
        "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: 21\r\nconnection: close\r\n\r\n<html>not json</html>",
    );

    let mut command = fixture.cmd();
    command.env(RIGHTMOVE_SEARCH_API_HEALTH_URL_ENV, api_url);
    let output = command.args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    let check = find_check(&envelope, "rightmove.search_api");
    assert_eq!(check["label"], "Rightmove Search API");
    assert_eq!(check["status"], "degraded");
    assert_eq!(check["severity"], "degraded");
    assert!(
        check["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("text/html"))
    );
    assert!(
        check["fix"]
            .as_array()
            .expect("fix array")
            .iter()
            .any(|item| item
                .as_str()
                .is_some_and(|text| text.contains("search.useApi = false")))
    );
}

#[test]
fn health_reports_rightmove_search_api_disabled_when_config_disables_api() {
    let fixture = Fixture::new();
    fs::write(
        fixture.config_dir.join("let.config.toml"),
        sample_config().replace("useApi = true", "useApi = false"),
    )
    .expect("write config");

    let output = fixture.cmd().args(["health"]).output().expect("run health");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    let check = find_check(&envelope, "rightmove.search_api");
    assert_eq!(check["status"], "disabled");
    assert_eq!(check["severity"], "info");
    assert!(
        check["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("search.useApi is false"))
    );
}

#[test]
fn sources_list_and_status_use_new_surface() {
    let fixture = Fixture::new();
    let list = fixture
        .cmd()
        .args(["sources", "list"])
        .output()
        .expect("run sources list");
    assert_eq!(list.status.code(), Some(0));
    let list_json = assert_single_json_envelope(&list);
    assert_eq!(list_json["meta"]["tool"], "sources.list");
    assert!(
        list_json["data"]["sources"]
            .as_array()
            .expect("sources")
            .iter()
            .any(|source| source == "broadband")
    );

    let status = fixture
        .cmd()
        .args(["sources", "status"])
        .output()
        .expect("run sources status");
    assert_eq!(status.status.code(), Some(0));
    let status_json = assert_single_json_envelope(&status);
    assert_eq!(status_json["meta"]["tool"], "sources.status");
    assert_eq!(status_json["data"]["present"], 0);
}

#[test]
fn area_postcode_reads_local_source_snapshot() {
    let fixture = Fixture::new();
    fixture.seed_area_sources();

    let output = fixture
        .cmd()
        .args([
            "area",
            "postcode",
            "aa1 1aa",
            "--radius-m",
            "500",
            "--limit",
            "5",
        ])
        .output()
        .expect("run area postcode");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "area.postcode");
    assert_eq!(envelope["data"]["query"]["normalizedPostcode"], "AA11AA");
    assert_eq!(envelope["data"]["joinKeys"]["lsoaCode"], "LSOA001");
    assert_eq!(envelope["data"]["broadband"]["gigabitAvailability"], 87.4);
    assert_eq!(envelope["data"]["sections"]["postcodes"]["status"], "ok");
    assert_eq!(envelope["data"]["sections"]["broadband"]["status"], "ok");
    assert_eq!(
        envelope["data"]["nearby"]["naptanStops"][0]["name"],
        "Central Rail Station"
    );
    assert_eq!(
        envelope["data"]["nearby"]["uprnCandidates"][0]["uprn"],
        "100021234567"
    );
    assert!(
        envelope["data"]["facts"]
            .as_array()
            .expect("facts")
            .iter()
            .any(|fact| fact["provider"] == "postcodesDb" && fact["name"] == "lsoaCode")
    );
}

#[test]
fn area_postcode_rejects_non_finite_radius() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["area", "postcode", "aa1 1aa", "--radius-m", "NaN"])
        .output()
        .expect("run area postcode");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "invalid_input");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("finite positive"))
    );
}

#[test]
fn evidence_reads_seeded_bundle() {
    let fixture = Fixture::new();
    fixture.seed_bundle();

    let output = fixture
        .cmd()
        .args([
            "evidence",
            "170448131",
            "--section",
            "broadband,verifications",
        ])
        .output()
        .expect("run evidence");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "evidence");
    assert_eq!(envelope["data"]["bundle"]["rightmoveId"], "170448131");
    assert_eq!(
        envelope["data"]["bundle"]["broadband"]["gigabitAvailability"],
        88.0
    );
    assert_eq!(
        envelope["data"]["requestedSections"],
        json!(["broadband", "verifications"])
    );
}

#[test]
fn evidence_reads_multiple_seeded_bundles() {
    let fixture = Fixture::new();
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448131",
        "1 Example Street, Manchester",
        "M1 1AA",
        1250,
        "2026-06-18T00:00:00.000Z",
    ));
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448132",
        "2 Example Street, Manchester",
        "M2 2BB",
        1750,
        "2026-06-18T01:00:00.000Z",
    ));

    let output = fixture
        .cmd()
        .args([
            "evidence",
            "170448131",
            "170448132",
            "--section",
            "rightmove",
        ])
        .output()
        .expect("run evidence batch");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "evidence");
    assert_eq!(envelope["meta"]["count"], 2);
    assert_eq!(envelope["data"]["count"], 2);
    assert_eq!(envelope["data"]["okCount"], 2);
    assert_eq!(envelope["data"]["errorCount"], 0);
    let items = envelope["data"]["items"].as_array().expect("items array");
    assert_eq!(items[0]["input"], "170448131");
    assert_eq!(items[0]["id"], "170448131");
    assert_eq!(items[0]["ok"], true);
    assert_eq!(items[0]["bundle"]["rightmoveId"], "170448131");
    assert_eq!(items[1]["bundle"]["rightmoveId"], "170448132");
}

#[test]
fn evidence_reads_stdin_when_no_ids_are_positional() {
    let fixture = Fixture::new();
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448131",
        "1 Example Street, Manchester",
        "M1 1AA",
        1250,
        "2026-06-18T00:00:00.000Z",
    ));
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448132",
        "2 Example Street, Manchester",
        "M2 2BB",
        1750,
        "2026-06-18T01:00:00.000Z",
    ));

    let mut command = fixture.cmd();
    command.args(["evidence", "--section", "rightmove"]);
    let output = output_with_stdin(command, "170448131, 170448132\n");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "evidence");
    assert_eq!(envelope["data"]["count"], 2);
    let items = envelope["data"]["items"].as_array().expect("items array");
    assert_eq!(items[0]["bundle"]["rightmoveId"], "170448131");
    assert_eq!(items[1]["bundle"]["rightmoveId"], "170448132");
}

#[test]
fn evidence_list_rejects_mixed_ids() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["evidence", "list", "170448131"])
        .output()
        .expect("run mixed evidence list");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "validation_error");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("sole `let evidence list`"))
    );
}

#[test]
fn evidence_rejects_list_filters_without_list_subcommand() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["evidence", "--confidence", "high"])
        .output()
        .expect("run evidence with list filter");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "validation_error");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("list filters require `let evidence list`"))
    );
}

#[test]
fn evidence_list_returns_summary_and_applies_filters() {
    let fixture = Fixture::new();
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448131",
        "1 Example Street, Manchester",
        "M1 1AA",
        1250,
        "2026-06-18T00:00:00.000Z",
    ));
    fixture.save_assessment(
        "170448131",
        json!({
            "recommendation": "view",
            "confidence": "high",
            "area": "Manchester"
        }),
    );
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448132",
        "2 Example Street, Manchester",
        "M2 2BB",
        1750,
        "2026-06-18T01:00:00.000Z",
    ));
    fixture.save_assessment(
        "170448132",
        json!({
            "recommendation": "skip",
            "confidence": "medium",
            "area": "Manchester"
        }),
    );

    let output = fixture
        .cmd()
        .args([
            "evidence",
            "list",
            "--recommendation",
            "view",
            "--confidence",
            "high",
            "--area",
            "manchester",
            "--max-price",
            "1300",
            "--postcode-prefix",
            "M1",
        ])
        .output()
        .expect("run evidence list");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "evidence.list");
    assert_eq!(envelope["meta"]["count"], 1);
    assert_eq!(envelope["data"]["filters"]["recommendation"], "view");
    assert_eq!(envelope["data"]["filters"]["confidence"], "high");
    let listings = envelope["data"]["listings"]
        .as_array()
        .expect("listings array");
    assert_eq!(listings.len(), 1);
    let listing = &listings[0];
    assert_eq!(listing["id"], "170448131");
    assert_eq!(listing["entityId"], "rightmove:170448131");
    assert_eq!(
        listing["url"],
        "https://www.rightmove.co.uk/properties/170448131"
    );
    assert_eq!(listing["address"], "1 Example Street, Manchester");
    assert_eq!(listing["postcode"], "M1 1AA");
    assert_eq!(listing["pricePcm"], 1250);
    assert_eq!(listing["recommendation"], "view");
    assert_eq!(listing["confidence"], "high");
    assert_eq!(listing["inspectedAt"], "2026-06-18T00:00:00.000Z");
    assert!(listing["savedAt"].is_string());
    assert!(listing["updatedAt"].is_string());
}

#[test]
fn verify_reads_seeded_verifications_without_refresh() {
    let fixture = Fixture::new();
    fixture.seed_bundle();

    let output = fixture
        .cmd()
        .args(["verify", "170448131", "--claim", "broadband"])
        .output()
        .expect("run verify");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "verify");
    assert_eq!(envelope["data"]["verifications"][0]["status"], "supported");
}

#[test]
fn verify_rejects_unknown_claims() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["verify", "170448131", "--claim", "broadbnd"])
        .output()
        .expect("run verify");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "validation_error");
    assert!(
        envelope["error"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("broadband"))
    );
}

#[test]
fn assess_save_and_get_persist_agent_json() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args([
            "assess",
            "save",
            "170448131",
            r#"{"recommendation":"view","reasoning":"good evidence"}"#,
        ])
        .output()
        .expect("run assess save");
    assert_eq!(output.status.code(), Some(0));
    let save_json = assert_single_json_envelope(&output);
    assert_eq!(save_json["meta"]["tool"], "assess.save");
    assert_eq!(save_json["data"]["assessment"]["recommendation"], "view");
    assert_eq!(
        save_json["data"]["normalizedAssessment"]["recommendation"],
        "view"
    );

    let get = fixture
        .cmd()
        .args(["assess", "get", "170448131"])
        .output()
        .expect("run assess get");
    assert_eq!(get.status.code(), Some(0));
    let get_json = assert_single_json_envelope(&get);
    assert_eq!(get_json["meta"]["tool"], "assess.get");
    assert_eq!(get_json["data"]["assessment"]["reasoning"], "good evidence");
    assert_eq!(
        get_json["data"]["normalizedAssessment"]["recommendation"],
        "view"
    );
}

#[test]
fn assess_list_returns_saved_assessments_and_applies_filters() {
    let fixture = Fixture::new();
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448131",
        "1 Example Street, Manchester",
        "M1 1AA",
        1250,
        "2026-06-18T00:00:00.000Z",
    ));
    fixture.save_assessment(
        "170448131",
        json!({
            "recommendation": "view",
            "confidence": "high",
            "area": "Manchester",
            "reasoning": "good evidence"
        }),
    );
    fixture.seed_bundle_record(&sample_bundle_variant(
        "170448132",
        "2 Example Street, Manchester",
        "M2 2BB",
        1750,
        "2026-06-18T01:00:00.000Z",
    ));
    fixture.save_assessment(
        "170448132",
        json!({
            "recommendation": "skip",
            "confidence": "medium",
            "area": "Manchester"
        }),
    );

    let output = fixture
        .cmd()
        .args([
            "assess",
            "list",
            "--recommendation",
            "view",
            "--confidence",
            "high",
            "--area",
            "manchester",
            "--max-price",
            "1300",
            "--postcode-prefix",
            "M1",
        ])
        .output()
        .expect("run assess list");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "assess.list");
    assert_eq!(envelope["meta"]["count"], 1);
    let assessments = envelope["data"]["assessments"]
        .as_array()
        .expect("assessments array");
    assert_eq!(assessments.len(), 1);
    let assessment = &assessments[0];
    assert_eq!(assessment["id"], "170448131");
    assert_eq!(assessment["entityId"], "rightmove:170448131");
    assert_eq!(assessment["postcode"], "M1 1AA");
    assert_eq!(assessment["pricePcm"], 1250);
    assert_eq!(assessment["recommendation"], "view");
    assert_eq!(assessment["confidence"], "high");
    assert_eq!(assessment["normalizedAssessment"]["confidence"], "high");
    assert_eq!(assessment["assessment"]["reasoning"], "good evidence");
    assert!(assessment["savedAt"].is_string());
}

#[test]
fn assess_save_rejects_non_object_json() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["assess", "save", "170448131", "[1,2,3]"])
        .output()
        .expect("run assess save");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "validation_error");
}

#[test]
fn correct_address_persists_and_clear_disables_correction() {
    let fixture = Fixture::new();
    fixture.seed_bundle();

    let save = fixture
        .cmd()
        .args([
            "correct",
            "address",
            "170448131",
            "--postcode",
            "YO1 7HH",
            "--lat",
            "53.9590",
            "--lng",
            "-1.0815",
            "--note",
            "verified manually",
        ])
        .output()
        .expect("run correct address");
    assert_eq!(save.status.code(), Some(0));
    let save_json = assert_single_json_envelope(&save);
    assert_eq!(save_json["meta"]["tool"], "correct.address");
    assert_eq!(save_json["data"]["correction"]["kind"], "address");
    assert_eq!(
        save_json["data"]["correction"]["payload"]["postcode"],
        "YO1 7HH"
    );
    let correction_id = save_json["data"]["correction"]["id"]
        .as_str()
        .expect("correction id")
        .to_owned();

    let evidence = fixture
        .cmd()
        .args(["evidence", "170448131"])
        .output()
        .expect("run evidence");
    assert_eq!(evidence.status.code(), Some(0));
    let evidence_json = assert_single_json_envelope(&evidence);
    assert_eq!(
        evidence_json["data"]["bundle"]["corrections"][0]["id"],
        correction_id
    );

    let clear = fixture
        .cmd()
        .args([
            "correct",
            "clear",
            "170448131",
            "--kind",
            "address",
            "--correction-id",
            &correction_id,
        ])
        .output()
        .expect("run correct clear");
    assert_eq!(clear.status.code(), Some(0));
    let clear_json = assert_single_json_envelope(&clear);
    assert_eq!(clear_json["meta"]["tool"], "correct.clear");
    assert_eq!(clear_json["data"]["correction"]["active"], false);

    let evidence_after_clear = fixture
        .cmd()
        .args(["evidence", "170448131"])
        .output()
        .expect("run evidence after clear");
    let evidence_after_clear_json = assert_single_json_envelope(&evidence_after_clear);
    assert!(
        evidence_after_clear_json["data"]["bundle"]
            .get("corrections")
            .is_none()
    );
}

#[test]
fn correct_epc_requires_certificate_anchor() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["correct", "epc", "170448131", "--rating", "C"])
        .output()
        .expect("run correct epc");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "validation_error");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("--lmk-key"))
    );
}

#[test]
fn inspect_rejects_non_rightmove_ids_with_json_envelope() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["inspect", "not-a-listing"])
        .output()
        .expect("run inspect");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "inspect");
    assert_eq!(envelope["error"]["code"], "invalid_input");
}

#[test]
fn inspect_batch_returns_per_item_errors() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["inspect", "not-a-listing", "still-not-a-listing"])
        .output()
        .expect("run inspect batch");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "inspect");
    assert_eq!(envelope["meta"]["count"], 2);
    assert_eq!(envelope["data"]["count"], 2);
    assert_eq!(envelope["data"]["okCount"], 0);
    assert_eq!(envelope["data"]["errorCount"], 2);
    let items = envelope["data"]["items"].as_array().expect("items array");
    assert_eq!(items[0]["input"], "not-a-listing");
    assert_eq!(items[0]["ok"], false);
    assert_eq!(items[0]["error"]["code"], "invalid_input");
    assert_eq!(items[1]["input"], "still-not-a-listing");
    assert_eq!(items[1]["error"]["code"], "invalid_input");
}

#[test]
fn inspect_reads_stdin_when_no_ids_are_positional() {
    let fixture = Fixture::new();
    let mut command = fixture.cmd();
    command.args(["inspect", "--depth", "quick"]);
    let output = output_with_stdin(command, "not-a-listing, still-not-a-listing\n");
    assert_eq!(output.status.code(), Some(0));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "inspect");
    assert_eq!(envelope["data"]["count"], 2);
    let items = envelope["data"]["items"].as_array().expect("items array");
    assert_eq!(items[0]["input"], "not-a-listing");
    assert_eq!(items[1]["input"], "still-not-a-listing");
}

#[test]
fn unsupported_group_subcommands_return_json_envelope() {
    let fixture = Fixture::new();
    let output = fixture
        .cmd()
        .args(["sources", "legacy"])
        .output()
        .expect("run unsupported group command");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "unsupported_command");
}

#[cfg(unix)]
#[test]
fn start_rejects_captured_stdio_before_launching_tui() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let fake_tui = fixture.data_dir.join("fake-let-tui");
    let capture = fixture.data_dir.join("start-env.txt");
    fs::write(
        &fake_tui,
        "#!/bin/sh\nprintf '%s\\n' \"$LET_DATA_DIR\" \"$LET_CONFIG_DIR\" \"$LET_CACHE_DIR\" \"$LET_SOURCES_DIR\" \"$LET_START_ID\" \"$LET_START_SECTIONS\" > \"$LET_TUI_CAPTURE\"\nexit 0\n",
    )
    .expect("write fake tui");
    let mut permissions = fs::metadata(&fake_tui)
        .expect("fake tui metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_tui, permissions).expect("chmod fake tui");

    let output = fixture
        .cmd()
        .env("LET_TUI_BIN", &fake_tui)
        .env("LET_TUI_CAPTURE", &capture)
        .args(["start", "--id", "170448131", "--section", "media,address"])
        .output()
        .expect("run start");
    assert_eq!(output.status.code(), Some(1));

    let envelope = assert_single_json_envelope(&output);
    assert_eq!(envelope["meta"]["tool"], "start");
    assert_eq!(envelope["error"]["code"], "start_requires_tty");
    assert!(
        !capture.exists(),
        "TUI subprocess must not run when stdio is captured"
    );
}

#[test]
fn json_envelope_helper_accepts_single_stdout_line() {
    let envelope = assert_single_json_envelope_stdout("{\"ok\":true}\n");
    assert_eq!(envelope["ok"], true);
}

#[test]
#[should_panic(expected = "expected exactly one stdout line")]
fn json_envelope_helper_rejects_extra_blank_stdout_lines() {
    let _ = assert_single_json_envelope_stdout("{\"ok\":true}\n\n");
}

fn assert_single_json_envelope(output: &Output) -> serde_json::Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");
    assert_single_json_envelope_stdout(&stdout)
}

fn assert_single_toon_envelope(output: &Output) -> serde_json::Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");
    let Some(document) = stdout.strip_suffix('\n') else {
        panic!("expected Toon envelope stdout to end with one newline, got: {stdout:?}");
    };
    toon_format::decode_default(document).expect("decode Toon envelope")
}

fn output_with_stdin(mut command: Command, stdin: &str) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn command with stdin");
    {
        let mut child_stdin = child.stdin.take().expect("stdin pipe");
        child_stdin
            .write_all(stdin.as_bytes())
            .expect("write stdin");
    }
    child.wait_with_output().expect("wait for command output")
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

fn find_check<'a>(envelope: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    envelope["data"]["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["id"] == id)
        .expect("check present")
}

fn serve_search_api_response_once(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let address = listener.local_addr().expect("read test server address");
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    format!("http://{address}/api/_search")
}

fn sample_config() -> &'static str {
    r#"
[search]
useApi = true

[[search.locations]]
id = "REGION^87490"
name = "Manchester"

[search.filters]
minBedrooms = 1
maxBedrooms = 4
minPrice = 600
maxPrice = 2500
propertyTypes = ["flat"]
includeLetAgreed = false
radius = 0.0
dontShow = []
mustHave = []

[fetch]
delayMs = 1
maxListings = 100
maxRetries = 2
minScore = 0
dropNewBelowMinScore = false
downloadMaps = false
downloadFloorplan = false
downloadEpcAsset = false
mediaDownloadConcurrency = 1
mediaProcessConcurrency = 1
mediaPhotoLandscapeWidth = 1200
mediaPhotoLandscapeHeight = 800
mediaPhotoPortraitWidth = 900
mediaPhotoPortraitHeight = 1200
mediaAuxWidth = 1200
mediaAuxHeight = 800
mediaMapWidth = 1200
mediaMapHeight = 800
mediaQualityPhoto = 82
mediaQualityAux = 82
mediaQualityMap = 82
mediaTimeoutMs = 15000

[scoring]
adaptiveness = 2.0
adaptivenessFactor = 0.5
regionPriority = {}

[scoring.weights]
affordability = 0.25
location = 0.30
liveability = 0.45

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
stationWeight = 0.45
broadbandWeight = 0.20
priorityWeight = 0.20
imdWeight = 0.15
crimeWeight = 0.0

[scoring.liveability]
heatingWeight = 0.2
gardenWeight = 0.2
propertyTypeWeight = 0.6

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
"#
}

fn sample_bundle_variant(
    rightmove_id: &str,
    address: &str,
    postcode: &str,
    price_pcm: i64,
    generated_at: &str,
) -> EvidenceBundle {
    let mut bundle = sample_bundle();
    let entity_id = format!("rightmove:{rightmove_id}");
    let url = format!("https://www.rightmove.co.uk/properties/{rightmove_id}");

    bundle.entity_id = entity_id.clone();
    bundle.rightmove_id = rightmove_id.to_owned();
    bundle.url = url.clone();
    bundle.generated_at = generated_at.to_owned();
    bundle.rightmove.rightmove_id = rightmove_id.to_owned();
    bundle.rightmove.url = url;
    bundle.rightmove.address = Some(address.to_owned());
    bundle.rightmove.postcode = Some(postcode.to_owned());
    bundle.rightmove.display_price = Some(format!("£{price_pcm} pcm"));
    bundle.rightmove.price_pcm = Some(price_pcm);
    bundle.address.candidates = vec![AddressCandidateEvidence {
        source: "rightmove".to_owned(),
        label: address.to_owned(),
        postcode: Some(postcode.to_owned()),
        latitude: Some(53.4808),
        longitude: Some(-2.2426),
        confidence: ConfidenceLevel::Exact,
        raw: None,
    }];
    if let Some(broadband) = bundle.broadband.as_mut() {
        broadband.postcode = postcode.replace(' ', "");
        broadband.postcode_display = Some(postcode.to_owned());
        broadband.outward = postcode.split_whitespace().next().map(ToOwned::to_owned);
        broadband.area = Some("M".to_owned());
    }
    bundle.assessment = None;
    bundle.claims.clear();
    bundle.verifications.clear();
    bundle
}

fn sample_bundle() -> EvidenceBundle {
    let source = SourceRef {
        source: "rightmove".to_owned(),
        snapshot_id: Some("snapshot-1".to_owned()),
        observation_id: None,
        url: Some("https://www.rightmove.co.uk/properties/170448131".to_owned()),
        captured_at: Some("2026-06-18T00:00:00.000Z".to_owned()),
    };
    let broadband_source = SourceRef {
        source: "broadbandDb".to_owned(),
        snapshot_id: None,
        observation_id: None,
        url: None,
        captured_at: None,
    };
    let assessment = AssessmentRecord::new(
        "rightmove:170448131".to_owned(),
        json!({"recommendation":"view"}),
        "2026-06-18T00:00:00.000Z".to_owned(),
    );

    EvidenceBundle {
        entity_id: "rightmove:170448131".to_owned(),
        rightmove_id: "170448131".to_owned(),
        url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
        generated_at: "2026-06-18T00:00:00.000Z".to_owned(),
        depth: InspectDepth::Standard,
        refresh: RefreshPolicy::Stale,
        sections: [
            (
                "rightmove".to_owned(),
                SectionState::ok("Rightmove PAGE_MODEL captured", ConfidenceLevel::Probable),
            ),
            (
                "broadband".to_owned(),
                SectionState::ok("broadband database matched", ConfidenceLevel::Probable),
            ),
            (
                "verifications".to_owned(),
                SectionState::ok("claims verified", ConfidenceLevel::Probable),
            ),
        ]
        .into_iter()
        .collect(),
        source_snapshots: Vec::new(),
        rightmove: RightmoveEvidence {
            rightmove_id: "170448131".to_owned(),
            url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
            page_status: "active".to_owned(),
            fetched_at: "2026-06-18T00:00:00.000Z".to_owned(),
            content_hash: "hash".to_owned(),
            title: Some("Two bedroom flat".to_owned()),
            address: Some("1 Example Street".to_owned()),
            postcode: Some("M1 1AA".to_owned()),
            display_price: Some("£1,250 pcm".to_owned()),
            price_pcm: Some(1250),
            bedrooms: Some(2),
            bathrooms: Some(1),
            property_type: Some("Flat".to_owned()),
            agent_name: Some("Example Agent".to_owned()),
            agent_phone: Some("0161 000 0000".to_owned()),
            latitude: Some(53.4808),
            longitude: Some(-2.2426),
            pin_type: Some("ACCURATE_POINT".to_owned()),
            listed_date: Some("2026-06-01".to_owned()),
            available_date: None,
            deposit: Some(1442),
            description: DescriptionEvidence {
                raw_html: "Gigabit broadband available".to_owned(),
                text: "Gigabit broadband available".to_owned(),
                key_features: vec!["Balcony".to_owned()],
                normalized_text: "balcony gigabit broadband available".to_owned(),
            },
            media: vec![MediaItemEvidence {
                kind: "photo".to_owned(),
                remote_url: "https://media.rightmove.co.uk/photo.jpg".to_owned(),
                local_path: None,
                width: None,
                height: None,
                content_hash: None,
                status: "remote".to_owned(),
            }],
        },
        address: AddressEvidence {
            candidates: vec![AddressCandidateEvidence {
                source: "rightmove".to_owned(),
                label: "1 Example Street".to_owned(),
                postcode: Some("M1 1AA".to_owned()),
                latitude: Some(53.4808),
                longitude: Some(-2.2426),
                confidence: ConfidenceLevel::Exact,
                raw: None,
            }],
            selected: None,
            status: SectionStatus::Ok,
            confidence: ConfidenceLevel::Exact,
            warnings: Vec::new(),
        },
        facts: vec![FactEvidence {
            provider: FactProvider::BroadbandDb,
            category: "broadband".to_owned(),
            name: "gigabitAvailability".to_owned(),
            value: json!(88.0),
            confidence: ConfidenceLevel::Probable,
            sources: vec![broadband_source.clone()],
        }],
        broadband: Some(BroadbandEvidence {
            postcode: "M11AA".to_owned(),
            postcode_display: Some("M1 1AA".to_owned()),
            outward: Some("M1".to_owned()),
            area: Some("M".to_owned()),
            gigabit_availability: Some(88.0),
            pct_over_300mbps: Some(92.0),
            ufbb_availability: Some(95.0),
            sfbb_availability: Some(99.0),
        }),
        epc: None,
        claims: vec![ClaimEvidence {
            id: "claim-1".to_owned(),
            claim_type: "broadband".to_owned(),
            claim_text: "description mentions gigabit broadband".to_owned(),
            value: json!({"claimedCapability":"gigabit"}),
            source: source.clone(),
        }],
        verifications: vec![VerificationEvidence {
            id: "verification-1".to_owned(),
            claim_id: Some("claim-1".to_owned()),
            claim_type: "broadband".to_owned(),
            status: VerificationStatus::Supported,
            confidence: ConfidenceLevel::Probable,
            explanation: "Ofcom postcode data supports the claim".to_owned(),
            evidence: vec![broadband_source],
        }],
        media: MediaEvidence {
            photos: vec![MediaItemEvidence {
                kind: "photo".to_owned(),
                remote_url: "https://media.rightmove.co.uk/photo.jpg".to_owned(),
                local_path: None,
                width: None,
                height: None,
                content_hash: None,
                status: "remote".to_owned(),
            }],
            floorplans: Vec::new(),
            epc_graphs: Vec::new(),
            maps: Vec::new(),
            contact_sheet: None,
        },
        assessment: Some(assessment),
        corrections: Vec::new(),
        next_actions: Vec::new(),
        flags: Vec::new(),
    }
}
