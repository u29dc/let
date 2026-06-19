#![forbid(unsafe_code)]

use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::Command;

use let_sdk::intelligence::EvidenceSection;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone)]
pub struct StartParams {
    pub id: Option<String>,
    pub sections: Vec<EvidenceSection>,
}

pub fn run(shared: &SharedArgs, params: StartParams) -> CommandResult {
    let binary = resolve_tui_binary().ok_or_else(|| {
        CommandError::runtime(
            "TUI_NOT_FOUND",
            "could not locate `let-tui` binary",
            "build with `bun run build` or set LET_TUI_BIN",
        )
    })?;
    run_resolved(
        shared,
        params,
        binary,
        TerminalStdio {
            stdin: io::stdin().is_terminal(),
            stdout: io::stdout().is_terminal(),
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct TerminalStdio {
    stdin: bool,
    stdout: bool,
}

fn run_resolved(
    shared: &SharedArgs,
    params: StartParams,
    binary: PathBuf,
    stdio: TerminalStdio,
) -> CommandResult {
    if !stdio.stdin || !stdio.stdout {
        return Err(CommandError::runtime(
            "START_REQUIRES_TTY",
            "`let start` requires terminal stdin and stdout",
            "run it from an interactive terminal so TUI control output cannot mix with captured JSON",
        ));
    }

    let paths = shared.resolved_paths();
    let section_names = params
        .sections
        .iter()
        .map(|section| section.as_str())
        .collect::<Vec<_>>()
        .join(",");

    let mut command = Command::new(&binary);
    command
        .env("LET_DATA_DIR", &paths.resolved.data)
        .env("LET_CONFIG_DIR", &paths.resolved.config)
        .env("LET_CACHE_DIR", &paths.resolved.cache)
        .env("LET_SOURCES_DIR", &paths.resolved.sources);

    if let Some(id) = &params.id {
        command.env("LET_START_ID", id);
    }

    if let Some(profile) = &shared.profile {
        let_sdk::config::validate_profile_name(profile)?;
        command.env("LET_PROFILE", profile);
    }

    if !section_names.is_empty() {
        command.env("LET_START_SECTIONS", &section_names);
    }

    let status = command.status().map_err(|error| {
        CommandError::runtime(
            "START_ERROR",
            format!("failed to start TUI at {}: {error}", binary.display()),
            "ensure the terminal supports crossterm and the binary is executable",
        )
    })?;

    if status.success() {
        Ok(CommandOutput::new(json!({
            "status": "exited",
            "code": status.code(),
            "binary": binary,
            "id": params.id,
            "sections": section_names,
        })))
    } else {
        Err(CommandError::new(
            "START_ERROR",
            format!("TUI exited with code {}", status.code().unwrap_or(1)),
            "check terminal output for crash details",
            status.code().unwrap_or(1),
        ))
    }
}

fn resolve_tui_binary() -> Option<PathBuf> {
    if let Ok(path) = env::var("LET_TUI_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let current_exe = env::current_exe().ok()?;
    let parent = current_exe.parent()?;

    let sibling = parent.join("let-tui");
    if sibling.is_file() {
        return Some(sibling);
    }

    let windows_sibling = parent.join("let-tui.exe");
    if windows_sibling.is_file() {
        return Some(windows_sibling);
    }

    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use let_sdk::paths::PathOverrides;
    use tempfile::TempDir;

    use super::{StartParams, TerminalStdio, run_resolved};
    use crate::commands::SharedArgs;

    #[test]
    fn start_rejects_captured_stdio() {
        let result = run_resolved(
            &SharedArgs {
                overrides: PathOverrides::default(),
                profile: None,
            },
            StartParams {
                id: None,
                sections: Vec::new(),
            },
            PathBuf::from("unused"),
            TerminalStdio {
                stdin: true,
                stdout: false,
            },
        );

        let error = result.expect_err("captured stdout should be rejected");
        assert_eq!(error.code, "START_REQUIRES_TTY");
    }

    #[test]
    fn start_rejects_invalid_profile_name() {
        let result = run_resolved(
            &SharedArgs {
                overrides: PathOverrides::default(),
                profile: Some("../bad".to_owned()),
            },
            StartParams {
                id: None,
                sections: Vec::new(),
            },
            PathBuf::from("unused"),
            TerminalStdio {
                stdin: true,
                stdout: true,
            },
        );

        let error = result.expect_err("invalid profile should be rejected before launch");
        assert_eq!(error.code, "INVALID_INPUT");
        assert!(error.message.contains("invalid config profile name"));
    }

    #[cfg(unix)]
    #[test]
    fn start_launches_tui_binary_with_runtime_paths() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("temp dir");
        let fake_tui = temp.path().join("fake-let-tui");
        let capture = temp.path().join("start-env.txt");
        fs::write(
            &fake_tui,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$LET_DATA_DIR\" \"$LET_CONFIG_DIR\" \"$LET_CACHE_DIR\" \"$LET_SOURCES_DIR\" \"$LET_START_ID\" \"$LET_START_SECTIONS\" > {}\nexit 0\n",
                shell_quote(capture.to_str().expect("capture path"))
            ),
        )
        .expect("write fake tui");
        let mut permissions = fs::metadata(&fake_tui)
            .expect("fake tui metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_tui, permissions).expect("chmod fake tui");

        let overrides = PathOverrides {
            data_dir: Some(temp.path().join("data")),
            config_dir: Some(temp.path().join("config")),
            cache_dir: Some(temp.path().join("cache")),
            sources_dir: Some(temp.path().join("sources")),
        };
        let output = run_resolved(
            &SharedArgs {
                overrides: overrides.clone(),
                profile: None,
            },
            StartParams {
                id: Some("170448131".to_owned()),
                sections: vec![let_sdk::intelligence::EvidenceSection::Media],
            },
            fake_tui,
            TerminalStdio {
                stdin: true,
                stdout: true,
            },
        )
        .expect("run start");

        assert_eq!(output.data["status"], "exited");
        assert_eq!(output.data["id"], "170448131");
        assert_eq!(output.data["sections"], "media");

        let captured = fs::read_to_string(capture).expect("capture env");
        let lines = captured.lines().collect::<Vec<_>>();
        assert_eq!(
            lines[0],
            overrides
                .data_dir
                .as_ref()
                .expect("data override")
                .to_str()
                .expect("data dir str")
        );
        assert_eq!(
            lines[1],
            overrides
                .config_dir
                .as_ref()
                .expect("config override")
                .to_str()
                .expect("config dir str")
        );
        assert_eq!(
            lines[2],
            overrides
                .cache_dir
                .as_ref()
                .expect("cache override")
                .to_str()
                .expect("cache dir str")
        );
        assert_eq!(
            lines[3],
            overrides
                .sources_dir
                .as_ref()
                .expect("sources override")
                .to_str()
                .expect("sources dir str")
        );
        assert_eq!(lines[4], "170448131");
        assert_eq!(lines[5], "media");
    }

    #[cfg(unix)]
    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
