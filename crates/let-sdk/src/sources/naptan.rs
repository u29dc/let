#![forbid(unsafe_code)]

use std::path::Path;

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{download_file, find_column_index, open_source_db, to_f64, with_temp_dir};

const NAPTAN_CSV_URL: &str = "https://naptan.api.dft.gov.uk/v1/access-nodes?dataFormat=csv";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let csv_path = temp.path().join("naptan.csv");
    download_file(NAPTAN_CSV_URL, &csv_path)?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open naptan CSV: {error}"),
                "verify naptan source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read naptan CSV headers: {error}"),
                "verify naptan source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let atco = find_column_index(&headers, &["atco", "code"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing ATCO code column in naptan CSV".to_owned(),
            "verify naptan source file format",
        )
    })?;

    let lat = find_column_index(&headers, &["latitude"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing latitude column in naptan CSV".to_owned(),
            "verify naptan source file format",
        )
    })?;

    let lng = find_column_index(&headers, &["longitude"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing longitude column in naptan CSV".to_owned(),
            "verify naptan source file format",
        )
    })?;

    let naptan = find_column_index(&headers, &["naptan", "code"]);
    let common_name = find_column_index(&headers, &["common", "name"]);
    let stop_type = find_column_index(&headers, &["stop type"]);

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS stops;
        CREATE TABLE stops (
            atco_code TEXT PRIMARY KEY,
            naptan_code TEXT,
            common_name TEXT,
            stop_type TEXT,
            lat REAL,
            lng REAL
        );
        CREATE INDEX idx_stops_lat_lng ON stops(lat, lng);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "INSERT INTO stops (atco_code, naptan_code, common_name, stop_type, lat, lng) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(atco_code) = cell(&row, atco) else {
            continue;
        };

        statement.execute(rusqlite::params![
            atco_code,
            optional_cell(&row, naptan),
            optional_cell(&row, common_name),
            optional_cell(&row, stop_type),
            to_f64(cell(&row, lat)),
            to_f64(cell(&row, lng)),
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

fn optional_cell(row: &StringRecord, idx: Option<usize>) -> Option<&str> {
    idx.and_then(|index| cell(row, index))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse naptan CSV row: {error}"),
        "verify naptan source file format",
    )
}
