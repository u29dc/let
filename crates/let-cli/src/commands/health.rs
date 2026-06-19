#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use let_sdk::config::{AppConfig, SearchConfig};
use let_sdk::{ErrorCode, database_overview};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::search::{SEARCH_API_BASE_URL, build_search_api_url_from_base};
use crate::commands::{CommandOutput, CommandResult, SharedArgs};
use crate::env::{EnvValueSource, resolve_env_var};

const RIGHTMOVE_SEARCH_API_HEALTH_URL_ENV: &str = "LET_RIGHTMOVE_SEARCH_API_HEALTH_URL";
const RIGHTMOVE_SEARCH_API_HEALTH_TIMEOUT: Duration = Duration::from_millis(1500);
const RIGHTMOVE_SEARCH_API_HEALTH_PAGE_SIZE: usize = 1;

const SOURCE_NAMES: [&str; 10] = [
    "postcodes",
    "broadband",
    "deprivation",
    "census",
    "population",
    "income",
    "flood",
    "crime",
    "naptan",
    "uprn",
];

#[derive(Debug, Clone, Serialize)]
struct HealthCheck {
    id: String,
    label: String,
    status: String,
    severity: String,
    detail: String,
    fix: Value,
}

pub fn run(shared: &SharedArgs) -> CommandResult {
    let bundle = shared.resolved_paths();
    let config_path = shared.config_path(&bundle)?;
    let mut checks = Vec::new();

    let (config_check, config) = check_config_file(&config_path);
    checks.push(config_check);
    if let Some(config) = config.as_ref() {
        checks.push(check_rightmove_search_api(&config.search));
    }
    checks.push(check_database(&bundle.derived.database));
    for source in SOURCE_NAMES {
        checks.push(check_source_db(
            source,
            &bundle.derived.source_db(&bundle.resolved.sources, source),
        ));
    }
    checks.push(check_epc_credentials(&bundle.derived.env_file));
    checks.push(check_env_key(
        "NOTION_API_KEY",
        "env.notion_api_key",
        "Notion API Key",
        &bundle.derived.env_file,
        "your-key",
    ));
    checks.push(check_env_key(
        "NOTION_DATABASE_ID",
        "env.notion_database_id",
        "Notion Database ID",
        &bundle.derived.env_file,
        "your-database-id",
    ));
    checks.push(check_env_key(
        "MAPBOX_ACCESS_TOKEN",
        "env.mapbox_access_token",
        "Mapbox Token",
        &bundle.derived.env_file,
        "your-token",
    ));
    checks.push(check_writable_dir(
        "dir.data",
        "Directory: data",
        &bundle.resolved.data,
    ));
    checks.push(check_writable_dir(
        "dir.cache",
        "Directory: cache",
        &bundle.resolved.cache,
    ));

    let summary = summarize_checks(&checks);
    let status = if summary.blocking > 0 {
        "blocked"
    } else if summary.degraded > 0 {
        "degraded"
    } else {
        "ready"
    };

    let data = json!({
        "status": status,
        "paths": {
            "config": bundle.resolved.config.display().to_string(),
            "configFile": config_path.display().to_string(),
            "profile": shared.profile.as_deref(),
            "data": bundle.resolved.data.display().to_string(),
            "cache": bundle.resolved.cache.display().to_string(),
            "sources": bundle.resolved.sources.display().to_string(),
        },
        "checks": checks,
        "summary": {
            "ok": summary.ok,
            "blocking": summary.blocking,
            "degraded": summary.degraded,
        }
    });

    let count = data["checks"].as_array().map_or(0, |items| items.len());
    Ok(CommandOutput::new(data)
        .with_count(count)
        .with_total(count)
        .with_has_more(false))
}

fn check_rightmove_search_api(config: &SearchConfig) -> HealthCheck {
    if !config.use_api {
        return shape_rightmove_search_api_check(false, None);
    }

    let location = config
        .locations
        .first()
        .map(|location| format!("{} ({})", location.name, location.id));
    let Some(url) = build_rightmove_search_api_probe_url(config) else {
        return HealthCheck {
            id: "rightmove.search_api".to_owned(),
            label: "Rightmove Search API".to_owned(),
            status: "unknown".to_owned(),
            severity: "degraded".to_owned(),
            detail: "search.useApi is true but no search location is configured".to_owned(),
            fix: json!(["add at least one search.locations entry or set search.useApi = false"]),
        };
    };

    shape_rightmove_search_api_check(
        true,
        Some(probe_rightmove_search_api(&url, location.as_deref())),
    )
}

fn build_rightmove_search_api_probe_url(config: &SearchConfig) -> Option<String> {
    let location = config.locations.first()?;
    let base_url = std::env::var(RIGHTMOVE_SEARCH_API_HEALTH_URL_ENV)
        .unwrap_or_else(|_| SEARCH_API_BASE_URL.to_owned());
    Some(build_search_api_url_from_base(
        &base_url,
        &location.id,
        &config.filters,
        RIGHTMOVE_SEARCH_API_HEALTH_PAGE_SIZE,
        0,
    ))
}

fn probe_rightmove_search_api(url: &str, location: Option<&str>) -> RightmoveSearchApiProbe {
    let client = match reqwest::blocking::Client::builder()
        .timeout(RIGHTMOVE_SEARCH_API_HEALTH_TIMEOUT)
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
        .default_headers(default_json_headers())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return RightmoveSearchApiProbe::RequestFailed {
                location: location.map(ToOwned::to_owned),
                message: format!("failed to build HTTP client: {error}"),
            };
        }
    };

    let response = match client.get(url).send() {
        Ok(response) => response,
        Err(error) => {
            return RightmoveSearchApiProbe::RequestFailed {
                location: location.map(ToOwned::to_owned),
                message: error.to_string(),
            };
        }
    };

    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = match response.text() {
        Ok(body) => body,
        Err(error) => {
            return RightmoveSearchApiProbe::RequestFailed {
                location: location.map(ToOwned::to_owned),
                message: format!("failed to read response body: {error}"),
            };
        }
    };

    classify_rightmove_search_api_response(location, status, &content_type, &body)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RightmoveSearchApiProbe {
    Usable {
        location: Option<String>,
        status: u16,
        content_type: String,
        property_count: usize,
    },
    HttpError {
        location: Option<String>,
        status: u16,
        content_type: String,
    },
    NonJson {
        location: Option<String>,
        status: u16,
        content_type: String,
    },
    InvalidJson {
        location: Option<String>,
        status: u16,
        content_type: String,
        error: String,
    },
    MissingProperties {
        location: Option<String>,
        status: u16,
        content_type: String,
    },
    RequestFailed {
        location: Option<String>,
        message: String,
    },
}

fn classify_rightmove_search_api_response(
    location: Option<&str>,
    status: u16,
    content_type: &str,
    body: &str,
) -> RightmoveSearchApiProbe {
    let location = location.map(ToOwned::to_owned);
    let content_type = content_type.to_owned();
    if !(200..300).contains(&status) {
        return RightmoveSearchApiProbe::HttpError {
            location,
            status,
            content_type,
        };
    }
    if !content_type.to_ascii_lowercase().contains("json") {
        return RightmoveSearchApiProbe::NonJson {
            location,
            status,
            content_type,
        };
    }

    let payload = match serde_json::from_str::<Value>(body) {
        Ok(payload) => payload,
        Err(error) => {
            return RightmoveSearchApiProbe::InvalidJson {
                location,
                status,
                content_type,
                error: error.to_string(),
            };
        }
    };

    let Some(properties) = payload.get("properties").and_then(Value::as_array) else {
        return RightmoveSearchApiProbe::MissingProperties {
            location,
            status,
            content_type,
        };
    };

    RightmoveSearchApiProbe::Usable {
        location,
        status,
        content_type,
        property_count: properties.len(),
    }
}

fn shape_rightmove_search_api_check(
    use_api: bool,
    probe: Option<RightmoveSearchApiProbe>,
) -> HealthCheck {
    if !use_api {
        return HealthCheck {
            id: "rightmove.search_api".to_owned(),
            label: "Rightmove Search API".to_owned(),
            status: "disabled".to_owned(),
            severity: "info".to_owned(),
            detail: "search.useApi is false; discovery uses Rightmove HTML search pages directly"
                .to_owned(),
            fix: Value::Null,
        };
    }

    match probe.expect("enabled Rightmove search API check needs a probe result") {
        RightmoveSearchApiProbe::Usable {
            location,
            status,
            content_type,
            property_count,
        } => HealthCheck {
            id: "rightmove.search_api".to_owned(),
            label: "Rightmove Search API".to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: format!(
                "search.useApi is true; {}returned http {status} {} with a properties array ({property_count} item probe)",
                location_prefix(location.as_deref()),
                display_content_type(&content_type)
            ),
            fix: Value::Null,
        },
        RightmoveSearchApiProbe::HttpError {
            location,
            status,
            content_type,
        } => degraded_rightmove_search_api_check(format!(
            "search.useApi is true; {}returned http {status} {}",
            location_prefix(location.as_deref()),
            display_content_type(&content_type)
        )),
        RightmoveSearchApiProbe::NonJson {
            location,
            status,
            content_type,
        } => degraded_rightmove_search_api_check(format!(
            "search.useApi is true; {}returned http {status} {} instead of JSON",
            location_prefix(location.as_deref()),
            display_content_type(&content_type)
        )),
        RightmoveSearchApiProbe::InvalidJson {
            location,
            status,
            content_type,
            error,
        } => degraded_rightmove_search_api_check(format!(
            "search.useApi is true; {}returned http {status} {} but the body was not valid JSON: {error}",
            location_prefix(location.as_deref()),
            display_content_type(&content_type)
        )),
        RightmoveSearchApiProbe::MissingProperties {
            location,
            status,
            content_type,
        } => degraded_rightmove_search_api_check(format!(
            "search.useApi is true; {}returned http {status} {} but no properties array",
            location_prefix(location.as_deref()),
            display_content_type(&content_type)
        )),
        RightmoveSearchApiProbe::RequestFailed { location, message } => HealthCheck {
            id: "rightmove.search_api".to_owned(),
            label: "Rightmove Search API".to_owned(),
            status: "unknown".to_owned(),
            severity: "degraded".to_owned(),
            detail: format!(
                "search.useApi is true; {}probe failed: {message}",
                location_prefix(location.as_deref())
            ),
            fix: rightmove_search_api_fix(),
        },
    }
}

fn degraded_rightmove_search_api_check(detail: String) -> HealthCheck {
    HealthCheck {
        id: "rightmove.search_api".to_owned(),
        label: "Rightmove Search API".to_owned(),
        status: "degraded".to_owned(),
        severity: "degraded".to_owned(),
        detail,
        fix: rightmove_search_api_fix(),
    }
}

fn rightmove_search_api_fix() -> Value {
    json!([
        "set search.useApi = false in let.config.toml to use HTML discovery",
        "update the Rightmove search API integration if the private endpoint contract changed",
        "rerun `let health` after changing config or integration"
    ])
}

fn location_prefix(location: Option<&str>) -> String {
    location.map_or_else(String::new, |location| format!("probe for {location} "))
}

fn display_content_type(content_type: &str) -> String {
    if content_type.trim().is_empty() {
        "(no content-type)".to_owned()
    } else {
        format!("({content_type})")
    }
}

fn default_json_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-GB,en;q=0.9"));
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    headers
}

fn check_epc_credentials(env_file: &Path) -> HealthCheck {
    let bearer_token = resolve_env_var("EPC_API_BEARER_TOKEN", env_file);
    let email = resolve_env_var("EPC_API_EMAIL", env_file);
    let api_key = resolve_env_var("EPC_API_KEY", env_file);
    let has_email = email.is_some();
    let has_api_key = api_key.is_some();

    if let Some((_, source)) = bearer_token {
        return HealthCheck {
            id: "env.epc_auth".to_owned(),
            label: "EPC API Auth".to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: format!(
                "EPC_API_BEARER_TOKEN is set ({})",
                env_value_source_text(source)
            ),
            fix: Value::Null,
        };
    }

    if let (Some((_, email_source)), Some((_, api_key_source))) = (&email, &api_key) {
        let source_text = if email_source == api_key_source {
            env_value_source_text(*email_source).to_owned()
        } else {
            format!(
                "{}/{}",
                env_value_source_text(*email_source),
                env_value_source_text(*api_key_source)
            )
        };
        return HealthCheck {
            id: "env.epc_auth".to_owned(),
            label: "EPC API Auth".to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: format!(
                "legacy EPC_API_EMAIL and EPC_API_KEY are set ({source_text}); prefer EPC_API_BEARER_TOKEN for the new service"
            ),
            fix: Value::Null,
        };
    }

    let detail = match (has_email, has_api_key) {
        (true, false) => "EPC_API_EMAIL is set but EPC_API_KEY is missing",
        (false, true) => "EPC_API_KEY is set but EPC_API_EMAIL is missing",
        _ => "EPC API credentials not set",
    };

    HealthCheck {
        id: "env.epc_auth".to_owned(),
        label: "EPC API Auth".to_owned(),
        status: "missing".to_owned(),
        severity: "degraded".to_owned(),
        detail: detail.to_owned(),
        fix: json!([
            format!(
                "echo 'EPC_API_BEARER_TOKEN=your-bearer-token' >> {}",
                shell_quote_path(env_file)
            ),
            format!(
                "echo 'EPC_API_EMAIL=your-email@example.com' >> {}",
                shell_quote_path(env_file)
            ),
            format!(
                "echo 'EPC_API_KEY=your-api-key' >> {}",
                shell_quote_path(env_file)
            ),
            "prefer EPC_API_BEARER_TOKEN for Get Energy Performance Data; legacy email/key only works during the Open Data Communities transition"
        ]),
    }
}

fn check_config_file(path: &Path) -> (HealthCheck, Option<AppConfig>) {
    if !path.exists() {
        return (
            HealthCheck {
                id: "config".to_owned(),
                label: "Configuration".to_owned(),
                status: "missing".to_owned(),
                severity: "blocking".to_owned(),
                detail: format!("missing {}", path.display()),
                fix: json!([format!("create {}", path.display())]),
            },
            None,
        );
    }

    match let_sdk::config::load_config(Some(path)) {
        Ok(config) => (
            HealthCheck {
                id: "config".to_owned(),
                label: "Configuration".to_owned(),
                status: "ok".to_owned(),
                severity: "info".to_owned(),
                detail: path.display().to_string(),
                fix: Value::Null,
            },
            Some(config),
        ),
        Err(error) => (
            HealthCheck {
                id: "config".to_owned(),
                label: "Configuration".to_owned(),
                status: "error".to_owned(),
                severity: "blocking".to_owned(),
                detail: format!("{} ({})", path.display(), error.message),
                fix: json!([format!("edit {}", path.display())]),
            },
            None,
        ),
    }
}

fn check_database(path: &Path) -> HealthCheck {
    if !path.exists() {
        return HealthCheck {
            id: "database".to_owned(),
            label: "Intelligence Database".to_owned(),
            status: "missing".to_owned(),
            severity: "degraded".to_owned(),
            detail: format!("missing {}", path.display()),
            fix: json!(["run `let inspect <rightmove-id>` to create the intelligence database"]),
        };
    }

    match database_overview(path) {
        Ok(overview) => HealthCheck {
            id: "database".to_owned(),
            label: "Intelligence Database".to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: format!(
                "{} ({} entities, {} bundles, {} assessments)",
                path.display(),
                overview.entity_count,
                overview.bundle_count,
                overview.assessment_count
            ),
            fix: Value::Null,
        },
        Err(error) if error.code == ErrorCode::SchemaMismatch => HealthCheck {
            id: "database".to_owned(),
            label: "Intelligence Database".to_owned(),
            status: "error".to_owned(),
            severity: "blocking".to_owned(),
            detail: format!("{} ({})", path.display(), error.message),
            fix: json!([format!(
                "delete {} and run `let inspect <rightmove-id>` to recreate the intelligence database",
                path.display()
            )]),
        },
        Err(error) if error.code == ErrorCode::Conflict => HealthCheck {
            id: "database".to_owned(),
            label: "Intelligence Database".to_owned(),
            status: "error".to_owned(),
            severity: "degraded".to_owned(),
            detail: format!("{} ({})", path.display(), error.message),
            fix: json!([
                "close other processes using the database and retry",
                "rerun `let health` once the lock clears"
            ]),
        },
        Err(error) => HealthCheck {
            id: "database".to_owned(),
            label: "Intelligence Database".to_owned(),
            status: "error".to_owned(),
            severity: "degraded".to_owned(),
            detail: format!("{} ({})", path.display(), error.message),
            fix: json!([
                "check database path, permissions, and disk state",
                "restore from backup only if the database file is damaged"
            ]),
        },
    }
}

fn check_source_db(name: &str, path: &Path) -> HealthCheck {
    if path.exists() {
        HealthCheck {
            id: format!("source.{name}"),
            label: format!("Source: {name}"),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: path.display().to_string(),
            fix: Value::Null,
        }
    } else {
        HealthCheck {
            id: format!("source.{name}"),
            label: format!("Source: {name}"),
            status: "missing".to_owned(),
            severity: "degraded".to_owned(),
            detail: format!("missing {}", path.display()),
            fix: json!([format!("run `let sources build {name}`")]),
        }
    }
}

fn check_env_key(key: &str, id: &str, label: &str, env_file: &Path, example: &str) -> HealthCheck {
    if let Some((_, source)) = resolve_env_var(key, env_file) {
        return HealthCheck {
            id: id.to_owned(),
            label: label.to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: format!("{key} is set ({})", env_value_source_text(source)),
            fix: Value::Null,
        };
    }

    HealthCheck {
        id: id.to_owned(),
        label: label.to_owned(),
        status: "missing".to_owned(),
        severity: "degraded".to_owned(),
        detail: format!("{key} not set"),
        fix: json!([format!(
            "echo '{key}={example}' >> {}",
            shell_quote_path(env_file)
        )]),
    }
}

fn env_value_source_text(source: EnvValueSource) -> &'static str {
    match source {
        EnvValueSource::Process => "process env",
        EnvValueSource::EnvFile => ".env file",
    }
}

fn check_writable_dir(id: &str, label: &str, path: &Path) -> HealthCheck {
    let status = match fs::create_dir_all(path) {
        Ok(()) => probe_write(path).map_or("error", |_| "ok"),
        Err(_) => "error",
    };

    if status == "ok" {
        HealthCheck {
            id: id.to_owned(),
            label: label.to_owned(),
            status: status.to_owned(),
            severity: "info".to_owned(),
            detail: path.display().to_string(),
            fix: Value::Null,
        }
    } else {
        HealthCheck {
            id: id.to_owned(),
            label: label.to_owned(),
            status: status.to_owned(),
            severity: "blocking".to_owned(),
            detail: format!("cannot write {}", path.display()),
            fix: json!([format!(
                "ensure directory exists and is writable: {}",
                path.display()
            )]),
        }
    }
}

fn probe_write(path: &Path) -> std::io::Result<()> {
    let probe = path.join(format!(
        ".let-cli-healthcheck-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&probe)?;
        file.write_all(b"ok")?;
        file.flush()
    })();
    let _ = fs::remove_file(&probe);
    result
}

fn shell_quote_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    format!("'{}'", raw.replace('\'', "'\\''"))
}

struct Summary {
    ok: usize,
    blocking: usize,
    degraded: usize,
}

fn summarize_checks(checks: &[HealthCheck]) -> Summary {
    let mut summary = Summary {
        ok: 0,
        blocking: 0,
        degraded: 0,
    };
    for check in checks {
        match check.severity.as_str() {
            "blocking" => summary.blocking += 1,
            "degraded" => summary.degraded += 1,
            _ => summary.ok += 1,
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::{
        RightmoveSearchApiProbe, classify_rightmove_search_api_response,
        shape_rightmove_search_api_check,
    };

    #[test]
    fn rightmove_search_api_classifier_accepts_properties_array() {
        let probe = classify_rightmove_search_api_response(
            Some("York (REGION^904)"),
            200,
            "application/json",
            r#"{"properties":[{"id":1701}]}"#,
        );

        assert_eq!(
            probe,
            RightmoveSearchApiProbe::Usable {
                location: Some("York (REGION^904)".to_owned()),
                status: 200,
                content_type: "application/json".to_owned(),
                property_count: 1,
            }
        );
    }

    #[test]
    fn rightmove_search_api_classifier_rejects_html() {
        let check = shape_rightmove_search_api_check(
            true,
            Some(classify_rightmove_search_api_response(
                Some("York (REGION^904)"),
                200,
                "text/html",
                "<html></html>",
            )),
        );

        assert_eq!(check.id, "rightmove.search_api");
        assert_eq!(check.status, "degraded");
        assert_eq!(check.severity, "degraded");
        assert!(check.detail.contains("text/html"));
        assert!(check.fix.as_array().expect("fix array").iter().any(|item| {
            item.as_str()
                .is_some_and(|text| text.contains("search.useApi = false"))
        }));
    }

    #[test]
    fn rightmove_search_api_classifier_rejects_json_without_properties() {
        let check = shape_rightmove_search_api_check(
            true,
            Some(classify_rightmove_search_api_response(
                Some("York (REGION^904)"),
                200,
                "application/json; charset=utf-8",
                r#"{"error":"upstream changed"}"#,
            )),
        );

        assert_eq!(check.status, "degraded");
        assert_eq!(check.severity, "degraded");
        assert!(check.detail.contains("no properties array"));
    }

    #[test]
    fn rightmove_search_api_disabled_is_not_degraded() {
        let check = shape_rightmove_search_api_check(false, None);

        assert_eq!(check.id, "rightmove.search_api");
        assert_eq!(check.status, "disabled");
        assert_eq!(check.severity, "info");
        assert!(check.detail.contains("search.useApi is false"));
        assert!(check.fix.is_null());
    }
}
