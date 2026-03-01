#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use let_sdk::paths::resolve_paths;
use serde::Serialize;
use serde_json::json;

use crate::commands::{CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthCheck {
    name: String,
    status: String,
    detail: String,
    fix: String,
}

pub fn run(shared: &SharedArgs) -> CommandResult {
    let bundle = resolve_paths(Some(shared.overrides.clone()));
    let mut checks = Vec::new();

    checks.push(check_config_file(&bundle.derived.config_file));
    checks.push(check_writable_dir(
        "data.dir",
        &bundle.resolved.data,
        "ensure data directory exists and is writable",
    ));
    checks.push(check_writable_dir(
        "cache.dir",
        &bundle.resolved.cache,
        "ensure cache directory exists and is writable",
    ));
    checks.push(check_sources_dir(&bundle.resolved.sources));

    let status = overall_status(&checks).to_string();
    let data = json!({
        "status": status,
        "checks": checks,
        "paths": {
            "config": bundle.resolved.config.display().to_string(),
            "data": bundle.resolved.data.display().to_string(),
            "cache": bundle.resolved.cache.display().to_string(),
            "sources": bundle.resolved.sources.display().to_string(),
            "configFile": bundle.derived.config_file.display().to_string(),
            "database": bundle.derived.database.display().to_string(),
        }
    });
    let count = data["checks"].as_array().map_or(0, |checks| checks.len());

    Ok(CommandOutput::new(data)
        .with_count(count)
        .with_total(count)
        .with_has_more(false)
        .with_text(format!("health status: {status}")))
}

fn check_config_file(path: &Path) -> HealthCheck {
    if path.exists() {
        HealthCheck {
            name: "config.file".to_string(),
            status: "ready".to_string(),
            detail: format!("found {}", path.display()),
            fix: "none".to_string(),
        }
    } else {
        HealthCheck {
            name: "config.file".to_string(),
            status: "blocked".to_string(),
            detail: format!("missing {}", path.display()),
            fix: format!("create {}", path.display()),
        }
    }
}

fn check_writable_dir(name: &str, path: &Path, fix_hint: &str) -> HealthCheck {
    let status = match fs::create_dir_all(path) {
        Ok(()) => probe_write(path).map_or("blocked", |_| "ready"),
        Err(_) => "blocked",
    };

    let detail = if status == "ready" {
        format!("writable {}", path.display())
    } else {
        format!("cannot write {}", path.display())
    };

    HealthCheck {
        name: name.to_string(),
        status: status.to_string(),
        detail,
        fix: format!("{fix_hint}: {}", path.display()),
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

fn check_sources_dir(path: &Path) -> HealthCheck {
    if path.exists() {
        HealthCheck {
            name: "sources.dir".to_string(),
            status: "ready".to_string(),
            detail: format!("found {}", path.display()),
            fix: "none".to_string(),
        }
    } else {
        HealthCheck {
            name: "sources.dir".to_string(),
            status: "degraded".to_string(),
            detail: format!("missing {}", path.display()),
            fix: "run source build commands before enrichment tasks".to_string(),
        }
    }
}

fn overall_status(checks: &[HealthCheck]) -> &'static str {
    if checks.iter().any(|check| check.status == "blocked") {
        "blocked"
    } else if checks.iter().any(|check| check.status == "degraded") {
        "degraded"
    } else {
        "ready"
    }
}
