#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::commands::CommandError;

const CLIPBOARD_BIN_ENV: &str = "LET_CLIPBOARD_BIN";

pub fn copy_text(text: &str) -> Result<(), CommandError> {
    let program = clipboard_program()?;
    let program_label = program.to_string_lossy().into_owned();

    let mut child = Command::new(&program)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CommandError::runtime(
                "CLIPBOARD_ERROR",
                format!("failed to launch clipboard command `{program_label}`: {error}"),
                format!(
                    "set {CLIPBOARD_BIN_ENV} to an executable clipboard helper or run without `--copy`"
                ),
            )
        })?;

    {
        let Some(stdin) = child.stdin.as_mut() else {
            return Err(CommandError::runtime(
                "CLIPBOARD_ERROR",
                format!("clipboard command `{program_label}` did not expose stdin"),
                "run without `--copy` or report this bug",
            ));
        };

        stdin.write_all(text.as_bytes()).map_err(|error| {
            CommandError::runtime(
                "CLIPBOARD_ERROR",
                format!("failed to write clipboard payload to `{program_label}`: {error}"),
                "run without `--copy` or report this bug",
            )
        })?;
    }

    let output = child.wait_with_output().map_err(|error| {
        CommandError::runtime(
            "CLIPBOARD_ERROR",
            format!("failed to wait for clipboard command `{program_label}`: {error}"),
            "run without `--copy` or report this bug",
        )
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let detail = if stderr.is_empty() {
        format!(
            "clipboard command `{program_label}` exited with {}",
            output.status
        )
    } else {
        format!(
            "clipboard command `{program_label}` exited with {}: {stderr}",
            output.status
        )
    };

    Err(CommandError::runtime(
        "CLIPBOARD_ERROR",
        detail,
        "run without `--copy` or set `LET_CLIPBOARD_BIN` to a working clipboard helper",
    ))
}

fn clipboard_program() -> Result<OsString, CommandError> {
    if let Ok(value) = std::env::var(CLIPBOARD_BIN_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(OsString::from(trimmed));
        }
    }

    #[cfg(target_os = "macos")]
    {
        Ok(OsString::from("/usr/bin/pbcopy"))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(CommandError::runtime(
            "UNSUPPORTED_PLATFORM",
            "clipboard copy is only supported on macOS",
            "run without `--copy` or set `LET_CLIPBOARD_BIN` to a test helper",
        ))
    }
}
