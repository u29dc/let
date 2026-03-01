#![forbid(unsafe_code)]

use std::env;
use std::path::{Path, PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    check_sas_expiry, download_file, find_column_index, normalize_postcode, open_source_db,
    with_temp_dir,
};

const FLOOD_CSV_URL: &str = "https://agrilake2live.file.core.windows.net/gms-datasets/fb921496-1788-4fc2-b469-7b51e2a45553/Postcodes_Risk_Assessment_All.csv?sv=2022-11-02&se=2026-02-09T12%3A34%3A08Z&sr=f&sp=r&sig=ZqHp87BTmcoetaCQ7aVNxBx0Sb5fVjoJEq50vFG0zZY%3D";

pub fn build(db_path: &Path) -> Result<usize> {
    let mut temp_guard = None;
    let csv_path: PathBuf = if let Ok(local_path) = env::var("FLOOD_CSV_PATH") {
        Path::new(&local_path).to_path_buf()
    } else {
        let temp = with_temp_dir()?;
        let temp_csv_path = temp.path().join("flood.csv");

        let download_url = env::var("FLOOD_CSV_URL").unwrap_or_else(|_| FLOOD_CSV_URL.to_owned());
        check_sas_expiry(&download_url, "flood")?;
        download_file(&download_url, &temp_csv_path)?;

        temp_guard = Some(temp);
        temp_csv_path
    };

    let _guard = temp_guard;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open flood CSV: {error}"),
                "verify flood source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read flood CSV headers: {error}"),
                "verify flood source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let postcode = find_column_index(&headers, &["postcode"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing postcode column in flood CSV".to_owned(),
            "verify flood source file format",
        )
    })?;

    let risk = find_column_index(
        &headers,
        &["overall risk", "risk overall", "risk category", "risk"],
    )
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing risk column in flood CSV".to_owned(),
            "verify flood source file format",
        )
    })?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS flood;
        CREATE TABLE flood (
            postcode TEXT PRIMARY KEY,
            risk TEXT,
            source TEXT
        );
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement =
        tx.prepare("INSERT INTO flood (postcode, risk, source) VALUES (?1, ?2, ?3)")?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(raw_postcode) = cell(&row, postcode) else {
            continue;
        };

        let normalized = normalize_postcode(raw_postcode);
        if normalized.is_empty() {
            continue;
        }

        statement.execute(rusqlite::params![
            normalized,
            cell(&row, risk),
            "ea-postcode-risk"
        ])?;
        inserted += 1;
    }

    drop(statement);
    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(inserted)
}

fn cell(row: &StringRecord, idx: usize) -> Option<&str> {
    row.get(idx)
        .map(str::trim)
        .and_then(|value| (!value.is_empty()).then_some(value))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse flood CSV row: {error}"),
        "verify flood source file format",
    )
}
