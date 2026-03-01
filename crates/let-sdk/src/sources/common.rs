#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rusqlite::Connection;
use tempfile::{TempDir, tempdir};
use url::Url;
use zip::ZipArchive;

use crate::errors::{ErrorCode, LetError, Result};

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
        .build()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Network,
                format!("failed to initialize http client: {error}"),
                "check TLS/runtime environment and retry",
            )
        })
}

pub fn download_file(url: &str, destination: &Path) -> Result<()> {
    download_file_with_headers(url, destination, &[])
}

pub fn download_file_with_headers(
    url: &str,
    destination: &Path,
    headers: &[(&str, &str)],
) -> Result<()> {
    ensure_parent_dir(destination)?;
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
    })
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

pub fn check_sas_expiry(url: &str, source_name: &str) -> Result<()> {
    let parsed = Url::parse(url).map_err(|error| {
        LetError::new(
            ErrorCode::InvalidInput,
            format!("invalid URL for {source_name}: {error}"),
            "verify source override URL format",
        )
    })?;

    let Some(expiry_raw) = parsed
        .query_pairs()
        .find_map(|(k, v)| (k == "se").then_some(v))
    else {
        return Ok(());
    };

    let expiry_text = expiry_raw.to_string();
    let expiry = chrono::DateTime::parse_from_rfc3339(&expiry_text).map_err(|_| {
        LetError::new(
            ErrorCode::Parse,
            format!("invalid SAS expiry value `{expiry_text}` for {source_name}"),
            "use a valid SAS URL override",
        )
    })?;

    let now = chrono::Utc::now();
    if expiry.with_timezone(&chrono::Utc) <= now {
        return Err(LetError::new(
            ErrorCode::Network,
            format!(
                "SAS URL for {source_name} has expired ({})",
                expiry.to_rfc3339()
            ),
            "set a fresh source override URL or local source path and retry",
        ));
    }

    Ok(())
}
