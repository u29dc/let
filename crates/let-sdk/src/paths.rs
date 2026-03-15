#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

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

    PathBundle {
        derived: build_derived(&resolved),
        resolved,
    }
}

pub fn paths() -> PathBundle {
    resolve_paths(None)
}

#[cfg(test)]
mod tests {
    use super::{PathOverrides, resolve_paths};
    use std::path::PathBuf;

    #[test]
    fn cli_override_wins() {
        let bundle = resolve_paths(Some(PathOverrides {
            data_dir: Some(PathBuf::from("/tmp/let-data")),
            ..PathOverrides::default()
        }));
        assert_eq!(bundle.resolved.data, PathBuf::from("/tmp/let-data"));
    }

    #[test]
    fn derived_paths_are_consistent() {
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

    #[test]
    fn consecutive_override_calls_do_not_reuse_previous_values() {
        let first = resolve_paths(Some(PathOverrides {
            data_dir: Some(PathBuf::from("/tmp/let-data-a")),
            ..PathOverrides::default()
        }));
        let second = resolve_paths(Some(PathOverrides {
            data_dir: Some(PathBuf::from("/tmp/let-data-b")),
            ..PathOverrides::default()
        }));

        assert_eq!(first.resolved.data, PathBuf::from("/tmp/let-data-a"));
        assert_eq!(second.resolved.data, PathBuf::from("/tmp/let-data-b"));
    }
}
