#![forbid(unsafe_code)]

use std::path::Path;
use std::{env, path::PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    download_file, extract_zip, find_column_index, find_first_matching_file, open_source_db,
    to_i64, with_temp_dir,
};

const TS001_URL: &str = "https://www.nomisweb.co.uk/output/census/2021/census2021-ts001.zip";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let zip_path = resolve_input_zip_path(&temp)?;
    let extract_dir = temp.path().join("extract");

    extract_zip(&zip_path, &extract_dir)?;

    let csv_path = find_first_matching_file(&extract_dir, &|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("census2021-ts001-lsoa.csv"))
    })?
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::NotFound,
            "LSOA CSV not found in census TS001 archive".to_owned(),
            "verify population source archive contents",
        )
    })?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open population CSV: {error}"),
                "verify population source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read population CSV headers: {error}"),
                "verify population source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let geo_code = find_column_index(&headers, &["geography code"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing geography code column in population CSV".to_owned(),
            "verify population source file format",
        )
    })?;

    let total =
        find_column_index(&headers, &["all usual residents", "total"]).ok_or_else(|| {
            LetError::new(
                ErrorCode::Parse,
                "missing total population column in population CSV".to_owned(),
                "verify population source file format",
            )
        })?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS population;
        CREATE TABLE population (
            lsoa_code TEXT PRIMARY KEY,
            population INTEGER
        );
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement =
        tx.prepare("INSERT OR REPLACE INTO population (lsoa_code, population) VALUES (?1, ?2)")?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(lsoa_code) = cell(&row, geo_code) else {
            continue;
        };

        statement.execute(rusqlite::params![lsoa_code, to_i64(cell(&row, total))])?;
        inserted += 1;
    }

    drop(statement);
    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(inserted)
}

fn resolve_input_zip_path(temp: &tempfile::TempDir) -> Result<PathBuf> {
    if let Ok(local_path) = env::var("POPULATION_TS001_ZIP_PATH") {
        return Ok(PathBuf::from(local_path));
    }

    let zip_path = temp.path().join("population-ts001.zip");
    let url = env::var("POPULATION_TS001_ZIP_URL").unwrap_or_else(|_| TS001_URL.to_owned());
    download_file(&url, &zip_path)?;
    Ok(zip_path)
}

fn cell(row: &StringRecord, idx: usize) -> Option<&str> {
    row.get(idx)
        .map(str::trim)
        .and_then(|value| (!value.is_empty()).then_some(value))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse population CSV row: {error}"),
        "verify population source file format",
    )
}
