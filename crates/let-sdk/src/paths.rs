#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Default)]
pub struct PathOverrides {
    pub data_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub sources_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPaths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub sources: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedPaths {
    pub config_file: PathBuf,
    pub template_file: PathBuf,
    pub env_file: PathBuf,
    pub database: PathBuf,
    pub backup: PathBuf,
    pub json_export: PathBuf,
}

impl DerivedPaths {
    pub fn source_db(&self, sources_root: &Path, name: &str) -> PathBuf {
        sources_root.join(format!("{name}.db"))
    }

    pub fn cache_dir(&self, cache_root: &Path, id: &str) -> PathBuf {
        cache_root.join(id)
    }

    pub fn cache_entry(&self, cache_root: &Path, id: &str) -> PathBuf {
        cache_root.join(id).join("data.json")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathBundle {
    pub resolved: ResolvedPaths,
    pub derived: DerivedPaths,
}

static CACHE: OnceLock<Mutex<Option<PathBundle>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<PathBundle>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

fn make_absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn env_or_default(key: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .map(make_absolute)
        .unwrap_or(fallback)
}

fn default_home() -> PathBuf {
    if let Some(let_home) = std::env::var_os("LET_HOME") {
        return PathBuf::from(let_home);
    }

    if let Some(tools_home) = std::env::var_os("TOOLS_HOME") {
        return PathBuf::from(tools_home).join("let");
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".tools")
        .join("let")
}

fn build_derived(resolved: &ResolvedPaths) -> DerivedPaths {
    DerivedPaths {
        config_file: resolved.config.join("let.config.toml"),
        template_file: resolved.config.join("let.config.template.toml"),
        env_file: resolved.config.join(".env"),
        database: resolved.data.join("let.db"),
        backup: resolved.data.join("let.db.bak"),
        json_export: resolved.data.join("let.db.json"),
    }
}

pub fn resolve_paths(overrides: Option<PathOverrides>) -> PathBundle {
    if overrides.is_none() {
        let guard = cache().lock().expect("path cache lock poisoned");
        if let Some(bundle) = guard.clone() {
            return bundle;
        }
    }

    let root = default_home();
    let defaults = ResolvedPaths {
        config: root.join("data"),
        data: root.join("data"),
        cache: root.join("cache"),
        sources: root.join("sources"),
    };

    let mut resolved = ResolvedPaths {
        config: env_or_default("LET_CONFIG_DIR", defaults.config),
        data: env_or_default("LET_DATA_DIR", defaults.data),
        cache: env_or_default("LET_CACHE_DIR", defaults.cache),
        sources: env_or_default("LET_SOURCES_DIR", defaults.sources),
    };

    if let Some(ovr) = overrides {
        if let Some(dir) = ovr.config_dir {
            resolved.config = make_absolute(dir);
        }
        if let Some(dir) = ovr.data_dir {
            resolved.data = make_absolute(dir);
        }
        if let Some(dir) = ovr.cache_dir {
            resolved.cache = make_absolute(dir);
        }
        if let Some(dir) = ovr.sources_dir {
            resolved.sources = make_absolute(dir);
        }
    }

    let bundle = PathBundle {
        derived: build_derived(&resolved),
        resolved,
    };

    let mut guard = cache().lock().expect("path cache lock poisoned");
    *guard = Some(bundle.clone());
    bundle
}

pub fn paths() -> PathBundle {
    resolve_paths(None)
}

pub fn reset_paths() {
    let mut guard = cache().lock().expect("path cache lock poisoned");
    *guard = None;
}

#[cfg(test)]
mod tests {
    use super::{PathOverrides, reset_paths, resolve_paths};
    use std::path::PathBuf;

    #[test]
    fn cli_override_wins() {
        reset_paths();
        let bundle = resolve_paths(Some(PathOverrides {
            data_dir: Some(PathBuf::from("/tmp/let-data")),
            ..PathOverrides::default()
        }));
        assert_eq!(bundle.resolved.data, PathBuf::from("/tmp/let-data"));
    }

    #[test]
    fn derived_paths_are_consistent() {
        reset_paths();
        let bundle = resolve_paths(Some(PathOverrides {
            data_dir: Some(PathBuf::from("/tmp/let-data")),
            config_dir: Some(PathBuf::from("/tmp/let-config")),
            ..PathOverrides::default()
        }));

        assert_eq!(
            bundle.derived.database,
            PathBuf::from("/tmp/let-data/let.db")
        );
        assert_eq!(
            bundle.derived.config_file,
            PathBuf::from("/tmp/let-config/let.config.toml")
        );
    }
}
