#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tempfile::{TempDir, tempdir};
use uuid::Uuid;
use zip::ZipArchive;

use crate::errors::{ErrorCode, LetError, Result};

const DOWNLOAD_ATTEMPTS: usize = 3;
const DOWNLOAD_RETRY_BASE_MS: u64 = 750;

#[derive(Debug, Clone)]
pub struct SourceInputDescriptor {
    pub source_id: &'static str,
    pub source_url: Option<&'static str>,
    pub override_envs: &'static [&'static str],
    pub declared_version: Option<&'static str>,
    pub notes: Option<&'static str>,
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        LetError::new(
            ErrorCode::Internal,
            format!("missing parent directory for path: {}", path.display()),
            "verify source database path configuration",
        )
    })?;

    fs::create_dir_all(parent).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to create directory {}: {error}", parent.display()),
            "ensure sources directory is writable",
        )
    })
}

pub fn recreate_file(path: &Path) -> Result<()> {
    ensure_parent_dir(path)?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!("failed to remove existing file {}: {error}", path.display()),
                "close active file handles and retry",
            )
        })?;
    }
    Ok(())
}

pub fn http_client() -> Result<Client> {
    reqwest::blocking::Client::builder()
        .user_agent("let-source-builder/0.1")
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(900))
        .build()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Network,
                format!("failed to initialize http client: {error}"),
                "check TLS/runtime environment and retry",
            )
        })
}

pub fn download_file_with_integrity(
    url: &str,
    destination: &Path,
    headers: &[(&str, &str)],
    checksum_envs: &[&str],
    source_id: &str,
) -> Result<()> {
    download_file_with_headers(url, destination, headers)?;
    verify_file_checksum_from_env(destination, checksum_envs, source_id)
}

pub fn download_file_checked(
    url: &str,
    destination: &Path,
    checksum_envs: &[&str],
    source_id: &str,
) -> Result<()> {
    download_file_with_integrity(url, destination, &[], checksum_envs, source_id)
}

pub fn download_file_with_headers(
    url: &str,
    destination: &Path,
    headers: &[(&str, &str)],
) -> Result<()> {
    ensure_parent_dir(destination)?;
    let mut last_error: Option<LetError> = None;

    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match download_file_once(url, destination, headers) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt == DOWNLOAD_ATTEMPTS {
                    break;
                }

                let backoff_ms = DOWNLOAD_RETRY_BASE_MS * (1_u64 << (attempt - 1));
                thread::sleep(Duration::from_millis(backoff_ms));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        LetError::new(
            ErrorCode::Network,
            format!("download failed for `{url}` with unknown error"),
            "retry source build command",
        )
    }))
}

fn download_file_once(url: &str, destination: &Path, headers: &[(&str, &str)]) -> Result<()> {
    let client = http_client()?;

    let mut header_map = HeaderMap::new();
    for (key, value) in headers {
        let header_name = HeaderName::from_bytes(key.as_bytes()).map_err(|error| {
            LetError::new(
                ErrorCode::InvalidInput,
                format!("invalid header name `{key}`: {error}"),
                "fix source header configuration",
            )
        })?;
        let header_value = HeaderValue::from_str(value).map_err(|error| {
            LetError::new(
                ErrorCode::InvalidInput,
                format!("invalid header value for `{key}`: {error}"),
                "fix source header configuration",
            )
        })?;
        header_map.insert(header_name, header_value);
    }

    let mut response = client
        .get(url)
        .headers(header_map)
        .send()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Network,
                format!("download request failed for `{url}`: {error}"),
                "check network access and retry",
            )
        })?;

    if !response.status().is_success() {
        return Err(LetError::new(
            ErrorCode::Network,
            format!(
                "download failed for `{url}`: {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("unknown")
            ),
            "verify source URL validity and access permissions",
        ));
    }

    let file = File::create(destination).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!(
                "failed to create download file {}: {error}",
                destination.display()
            ),
            "ensure destination path is writable",
        )
    })?;
    let mut writer = BufWriter::new(file);

    io::copy(&mut response, &mut writer).map_err(|error| {
        LetError::new(
            ErrorCode::Network,
            format!("failed to write download for `{url}`: {error}"),
            "check disk availability and retry",
        )
    })?;

    writer.flush().map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!(
                "failed to flush download file {}: {error}",
                destination.display()
            ),
            "check disk health and retry",
        )
    })?;

    Ok(())
}

pub fn verify_file_checksum_from_env(
    path: &Path,
    checksum_envs: &[&str],
    source_id: &str,
) -> Result<()> {
    let Some(expected_hash) = resolve_expected_sha256(checksum_envs)? else {
        return Ok(());
    };

    verify_file_checksum(path, &expected_hash, source_id)
}

fn resolve_expected_sha256(checksum_envs: &[&str]) -> Result<Option<String>> {
    let mut configured = checksum_envs
        .iter()
        .filter_map(|key| env::var(key).ok().map(|value| (key, value)))
        .map(|(key, value)| (key, value.trim().to_owned()))
        .filter(|(_key, value)| !value.is_empty())
        .collect::<Vec<_>>();

    if configured.is_empty() {
        return Ok(None);
    }

    let (first_key, first_value) = configured.remove(0);
    let normalized = normalize_sha256_value(first_key, &first_value)?;

    for (key, value) in configured {
        let candidate = normalize_sha256_value(key, &value)?;
        if candidate != normalized {
            return Err(LetError::new(
                ErrorCode::InvalidInput,
                format!("conflicting checksum values configured in `{first_key}` and `{key}`"),
                "set one checksum env variable per source input",
            ));
        }
    }

    Ok(Some(normalized))
}

fn normalize_sha256_value(env_key: &str, value: &str) -> Result<String> {
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(LetError::new(
            ErrorCode::InvalidInput,
            format!("invalid SHA-256 value in `{env_key}`"),
            "set a 64-character lowercase or uppercase hex SHA-256 value",
        ));
    }

    Ok(value.to_ascii_lowercase())
}

fn verify_file_checksum(path: &Path, expected_sha256: &str, source_id: &str) -> Result<()> {
    if !path.exists() {
        return Err(LetError::new(
            ErrorCode::NotFound,
            format!(
                "cannot verify checksum for `{source_id}` because file is missing: {}",
                path.display()
            ),
            "verify source path configuration and retry",
        ));
    }

    let file = File::open(path).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!(
                "failed to open source file for checksum verification {}: {error}",
                path.display()
            ),
            "ensure source file is readable and retry",
        )
    })?;

    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let bytes = reader.read(&mut buffer).map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!(
                    "failed to read source file for checksum verification {}: {error}",
                    path.display()
                ),
                "ensure source file is readable and retry",
            )
        })?;

        if bytes == 0 {
            break;
        }

        hasher.update(&buffer[..bytes]);
    }

    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != expected_sha256 {
        return Err(LetError::new(
            ErrorCode::Parse,
            format!(
                "checksum verification failed for `{source_id}` (expected {expected_sha256}, got {actual_sha256})"
            ),
            "refresh source input or update checksum env value",
        ));
    }

    Ok(())
}

pub fn extract_zip(zip_path: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!(
                "failed to create extraction directory {}: {error}",
                destination.display()
            ),
            "ensure extraction directory is writable",
        )
    })?;

    let file = File::open(zip_path).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to open zip archive {}: {error}", zip_path.display()),
            "ensure the downloaded archive exists and is readable",
        )
    })?;

    let mut archive = ZipArchive::new(file).map_err(|error| {
        LetError::new(
            ErrorCode::Parse,
            format!("failed to read zip archive {}: {error}", zip_path.display()),
            "verify source archive is valid",
        )
    })?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read zip entry #{i}: {error}"),
                "verify source archive integrity",
            )
        })?;

        let out_path = destination.join(entry.mangled_name());
        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path).map_err(|error| {
                LetError::new(
                    ErrorCode::Internal,
                    format!("failed to create directory {}: {error}", out_path.display()),
                    "ensure extraction directory is writable",
                )
            })?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                LetError::new(
                    ErrorCode::Internal,
                    format!("failed to create directory {}: {error}", parent.display()),
                    "ensure extraction directory is writable",
                )
            })?;
        }

        let mut out_file = File::create(&out_path).map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!(
                    "failed to create extracted file {}: {error}",
                    out_path.display()
                ),
                "ensure extraction directory is writable",
            )
        })?;

        io::copy(&mut entry, &mut out_file).map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!("failed to extract file {}: {error}", out_path.display()),
                "verify disk space and retry",
            )
        })?;
    }

    Ok(())
}

pub fn with_temp_dir() -> Result<TempDir> {
    tempdir().map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to create temp directory: {error}"),
            "check temporary directory permissions",
        )
    })
}

pub fn open_source_db(path: &Path) -> Result<Connection> {
    recreate_file(path)?;
    let connection = Connection::open(path).map_err(|error| {
        LetError::new(
            ErrorCode::SchemaMismatch,
            format!("failed to open source database {}: {error}", path.display()),
            "verify sqlite setup and path permissions",
        )
    })?;

    connection
        .execute_batch(
            "
            PRAGMA journal_mode = DELETE;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;
            ",
        )
        .map_err(|error| {
            LetError::new(
                ErrorCode::SchemaMismatch,
                format!("failed to initialize sqlite pragmas: {error}"),
                "ensure sqlite supports expected pragmas",
            )
        })?;

    Ok(connection)
}

pub fn temp_db_path_for(source_name: &str, final_path: &Path) -> Result<PathBuf> {
    ensure_parent_dir(final_path)?;
    let parent = final_path.parent().ok_or_else(|| {
        LetError::new(
            ErrorCode::Internal,
            format!(
                "missing parent directory for path: {}",
                final_path.display()
            ),
            "verify source database path configuration",
        )
    })?;

    Ok(parent.join(format!(".{source_name}.{}.tmp.db", Uuid::new_v4())))
}

pub fn replace_file_atomically(temp_path: &Path, final_path: &Path) -> Result<()> {
    ensure_parent_dir(final_path)?;
    if fs::rename(temp_path, final_path).is_ok() {
        return Ok(());
    }

    if !final_path.exists() {
        return fs::rename(temp_path, final_path).map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!(
                    "failed to replace {} with {}: {error}",
                    final_path.display(),
                    temp_path.display(),
                ),
                "ensure sources directory is writable and retry",
            )
        });
    }

    let backup_path = final_path.with_extension(format!("swap-{}", Uuid::new_v4()));
    fs::rename(final_path, &backup_path).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!(
                "failed to move existing file {} to backup {}: {error}",
                final_path.display(),
                backup_path.display()
            ),
            "close active file handles and retry",
        )
    })?;

    match fs::rename(temp_path, final_path) {
        Ok(()) => {
            let _ = fs::remove_file(&backup_path);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&backup_path, final_path);
            Err(LetError::new(
                ErrorCode::Internal,
                format!(
                    "failed to replace {} with {}: {error}",
                    final_path.display(),
                    temp_path.display(),
                ),
                "ensure sources directory is writable and retry",
            ))
        }
    }
}

pub fn remove_file_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to remove file {}: {error}", path.display()),
            "close active file handles and retry",
        )
    })
}

pub fn write_source_metadata(
    db_path: &Path,
    source_name: &str,
    rows: usize,
    inputs: &[SourceInputDescriptor],
) -> Result<()> {
    let connection = Connection::open(db_path).map_err(|error| {
        LetError::new(
            ErrorCode::SchemaMismatch,
            format!(
                "failed to open source database {} for metadata write: {error}",
                db_path.display()
            ),
            "verify sqlite setup and path permissions",
        )
    })?;

    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS source_runs (
            run_id TEXT PRIMARY KEY,
            source_name TEXT NOT NULL,
            built_at TEXT NOT NULL,
            rows_written INTEGER NOT NULL,
            tool_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS source_inputs (
            run_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            source_url TEXT,
            local_path TEXT,
            resolved_input TEXT,
            declared_version TEXT,
            downloaded_at TEXT NOT NULL,
            notes TEXT,
            PRIMARY KEY (run_id, source_id),
            FOREIGN KEY (run_id) REFERENCES source_runs(run_id)
        );

        CREATE INDEX IF NOT EXISTS idx_source_inputs_source_id ON source_inputs(source_id);
        ",
    )?;

    let run_id = Uuid::new_v4().to_string();
    let built_at = Utc::now().to_rfc3339();
    connection.execute(
        "
        INSERT INTO source_runs (run_id, source_name, built_at, rows_written, tool_version)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        rusqlite::params![
            run_id,
            source_name,
            built_at,
            rows as i64,
            env!("CARGO_PKG_VERSION"),
        ],
    )?;

    let mut insert_input = connection.prepare(
        "
        INSERT INTO source_inputs (
            run_id, source_id, source_url, local_path, resolved_input,
            declared_version, downloaded_at, notes
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
    )?;

    for descriptor in inputs {
        let (resolved_input, source_url, local_path) = resolve_source_input(descriptor);
        insert_input.execute(rusqlite::params![
            run_id,
            descriptor.source_id,
            source_url,
            local_path,
            resolved_input,
            descriptor.declared_version,
            built_at,
            descriptor.notes,
        ])?;
    }

    Ok(())
}

fn resolve_source_input(
    descriptor: &SourceInputDescriptor,
) -> (Option<String>, Option<String>, Option<String>) {
    let override_value = descriptor
        .override_envs
        .iter()
        .find_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    let resolved_input = override_value
        .clone()
        .or_else(|| descriptor.source_url.map(str::to_owned));
    let source_url = resolved_input
        .as_ref()
        .filter(|value| is_http_url(value))
        .cloned();
    let local_path = resolved_input
        .as_ref()
        .filter(|value| !is_http_url(value))
        .cloned();

    (resolved_input, source_url, local_path)
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub fn find_first_matching_file<F>(root: &Path, predicate: &F) -> Result<Option<PathBuf>>
where
    F: Fn(&Path) -> bool,
{
    if !root.exists() {
        return Ok(None);
    }
    if root.is_file() {
        return Ok(predicate(root).then(|| root.to_path_buf()));
    }

    let entries = fs::read_dir(root).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to read directory {}: {error}", root.display()),
            "verify source extraction output",
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!(
                    "failed to read directory entry in {}: {error}",
                    root.display()
                ),
                "verify source extraction output",
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_first_matching_file(&path, predicate)? {
                return Ok(Some(found));
            }
            continue;
        }
        if predicate(&path) {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

pub fn collect_matching_files<F>(root: &Path, predicate: &F, acc: &mut Vec<PathBuf>) -> Result<()>
where
    F: Fn(&Path) -> bool,
{
    if !root.exists() {
        return Ok(());
    }
    if root.is_file() {
        if predicate(root) {
            acc.push(root.to_path_buf());
        }
        return Ok(());
    }

    for entry in fs::read_dir(root).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to read directory {}: {error}", root.display()),
            "verify source extraction output",
        )
    })? {
        let entry = entry.map_err(|error| {
            LetError::new(
                ErrorCode::Internal,
                format!(
                    "failed to read directory entry in {}: {error}",
                    root.display()
                ),
                "verify source extraction output",
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_matching_files(&path, predicate, acc)?;
        } else if predicate(&path) {
            acc.push(path);
        }
    }

    Ok(())
}

pub fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(OsStr::to_str)
}

pub fn normalize_postcode(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>()
        .to_uppercase()
}

pub fn to_f64(value: Option<&str>) -> Option<f64> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        trimmed.parse::<f64>().ok()
    })
}

pub fn to_i64(value: Option<&str>) -> Option<i64> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        trimmed.parse::<i64>().ok()
    })
}

pub fn find_column_index(headers: &[String], patterns: &[&str]) -> Option<usize> {
    let lowered = headers
        .iter()
        .map(|header| header.to_lowercase())
        .collect::<Vec<_>>();

    for pattern in patterns {
        if let Some(idx) = lowered.iter().position(|header| header.contains(pattern)) {
            return Some(idx);
        }
    }
    None
}
