#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use let_sdk::paths::resolve_paths;
use let_sdk::{ErrorCode, load_listings_file};
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandOutput, CommandResult, SharedArgs};
use crate::env::{EnvValueSource, resolve_env_var};

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
    let bundle = resolve_paths(Some(shared.overrides.clone()));
    let mut checks = Vec::new();

    checks.push(check_config_file(&bundle.derived.config_file));
    checks.push(check_database(&bundle.derived.database));
    for source in SOURCE_NAMES {
        checks.push(check_source_db(
            source,
            &bundle.derived.source_db(&bundle.resolved.sources, source),
        ));
    }
    checks.push(check_env_key(
        "EPC_API_EMAIL",
        "env.epc_api_email",
        "EPC API Email",
        &bundle.derived.env_file,
        "your-email@example.com",
    ));
    checks.push(check_env_key(
        "EPC_API_KEY",
        "env.epc_api_key",
        "EPC API Key",
        &bundle.derived.env_file,
        "your-api-key",
    ));
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
        .with_has_more(false)
        .with_text(format!("health status: {status}")))
}

fn check_config_file(path: &Path) -> HealthCheck {
    if path.exists() {
        HealthCheck {
            id: "config".to_owned(),
            label: "Configuration".to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: path.display().to_string(),
            fix: Value::Null,
        }
    } else {
        HealthCheck {
            id: "config".to_owned(),
            label: "Configuration".to_owned(),
            status: "missing".to_owned(),
            severity: "blocking".to_owned(),
            detail: format!("missing {}", path.display()),
            fix: json!([format!("create {}", path.display())]),
        }
    }
}

fn check_database(path: &Path) -> HealthCheck {
    if !path.exists() {
        return HealthCheck {
            id: "database".to_owned(),
            label: "Listings Database".to_owned(),
            status: "missing".to_owned(),
            severity: "degraded".to_owned(),
            detail: format!("missing {}", path.display()),
            fix: json!(["run `let fetch <id>` to create and populate the listings database"]),
        };
    }

    match load_listings_file(path) {
        Ok(data) => HealthCheck {
            id: "database".to_owned(),
            label: "Listings Database".to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: format!("{} ({} listings)", path.display(), data.listings.len()),
            fix: Value::Null,
        },
        Err(error) if error.code == ErrorCode::SchemaMismatch => {
            let backup = format!("{}.bak", path.display());
            HealthCheck {
                id: "database".to_owned(),
                label: "Listings Database".to_owned(),
                status: "error".to_owned(),
                severity: "blocking".to_owned(),
                detail: format!("{} ({})", path.display(), error.message),
                fix: json!([
                    format!(
                        "restore backup database from {backup} or delete {}",
                        path.display()
                    ),
                    "run `let fetch <id>` to recreate the listings database"
                ]),
            }
        }
        Err(error) if error.code == ErrorCode::Conflict => HealthCheck {
            id: "database".to_owned(),
            label: "Listings Database".to_owned(),
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
            label: "Listings Database".to_owned(),
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
            fix: json!([format!("run `let build sources {name}`")]),
        }
    }
}

fn check_env_key(key: &str, id: &str, label: &str, env_file: &Path, example: &str) -> HealthCheck {
    if let Some((_, source)) = resolve_env_var(key, env_file) {
        let source_text = match source {
            EnvValueSource::Process => "process env",
            EnvValueSource::EnvFile => ".env file",
        };
        return HealthCheck {
            id: id.to_owned(),
            label: label.to_owned(),
            status: "ok".to_owned(),
            severity: "info".to_owned(),
            detail: format!("{key} is set ({source_text})"),
            fix: Value::Null,
        };
    }

    HealthCheck {
        id: id.to_owned(),
        label: label.to_owned(),
        status: "missing".to_owned(),
        severity: "degraded".to_owned(),
        detail: format!("{key} not set"),
        fix: json!([format!("echo '{key}={example}' >> {}", env_file.display())]),
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
    let probe = path.join(".let-cli-healthcheck.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe)?;
    file.write_all(b"ok")?;
    drop(file);
    let _ = fs::remove_file(&probe);
    Ok(())
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
