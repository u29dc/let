#![forbid(unsafe_code)]

use std::path::Path;

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    download_file, extract_zip, find_column_index, find_first_matching_file, open_source_db,
    to_f64, with_temp_dir,
};

const UPRN_ZIP_URL: &str =
    "https://api.os.uk/downloads/v1/products/OpenUPRN/downloads?area=GB&format=CSV&redirect";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let zip_path = temp.path().join("uprn.zip");
    let extract_dir = temp.path().join("extract");

    download_file(UPRN_ZIP_URL, &zip_path)?;
    extract_zip(&zip_path, &extract_dir)?;

    let csv_path = find_first_matching_file(&extract_dir, &|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
    })?
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::NotFound,
            "UPRN CSV not found in archive".to_owned(),
            "verify UPRN source archive contents",
        )
    })?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open UPRN CSV: {error}"),
                "verify UPRN source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read UPRN CSV headers: {error}"),
                "verify UPRN source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let uprn = find_column_index(&headers, &["uprn"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing UPRN column in UPRN CSV".to_owned(),
            "verify UPRN source file format",
        )
    })?;

    let lat = find_column_index(&headers, &["lat"]);
    let lng = find_column_index(&headers, &["long", "lng"]);
    let x = find_column_index(&headers, &["x"]);
    let y = find_column_index(&headers, &["y"]);

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS uprn;
        CREATE TABLE uprn (
            uprn TEXT PRIMARY KEY,
            lat REAL,
            lng REAL,
            x REAL,
            y REAL
        );
        CREATE INDEX idx_uprn_lat_lng ON uprn(lat, lng);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement =
        tx.prepare("INSERT INTO uprn (uprn, lat, lng, x, y) VALUES (?1, ?2, ?3, ?4, ?5)")?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(uprn_value) = cell(&row, uprn) else {
            continue;
        };

        statement.execute(rusqlite::params![
            uprn_value,
            optional_number(&row, lat),
            optional_number(&row, lng),
            optional_number(&row, x),
            optional_number(&row, y),
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

fn optional_number(row: &StringRecord, idx: Option<usize>) -> Option<f64> {
    idx.and_then(|index| to_f64(cell(row, index)))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse UPRN CSV row: {error}"),
        "verify UPRN source file format",
    )
}
