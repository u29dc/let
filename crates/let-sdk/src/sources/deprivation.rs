#![forbid(unsafe_code)]

use std::path::Path;

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    download_file, find_column_index, open_source_db, to_f64, to_i64, with_temp_dir,
};

const IMD_CSV_URL: &str = "https://assets.publishing.service.gov.uk/media/691ded56d140bbbaa59a2a7d/File_7_IoD2025_All_Ranks_Scores_Deciles_Population_Denominators.csv";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let csv_path = temp.path().join("deprivation.csv");
    download_file(IMD_CSV_URL, &csv_path)?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open deprivation CSV: {error}"),
                "verify deprivation source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read deprivation CSV headers: {error}"),
                "verify deprivation source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let lsoa_idx = find_column_index(&headers, &["lsoa code"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing `lsoa code` column in deprivation CSV".to_owned(),
            "verify deprivation source file format",
        )
    })?;
    let rank_idx = find_column_index(
        &headers,
        &[
            "index of multiple deprivation (imd) rank",
            "imd) rank",
            "imd rank",
        ],
    )
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing IMD rank column in deprivation CSV".to_owned(),
            "verify deprivation source file format",
        )
    })?;
    let decile_idx = find_column_index(
        &headers,
        &[
            "index of multiple deprivation (imd) decile",
            "imd) decile",
            "imd decile",
        ],
    )
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing IMD decile column in deprivation CSV".to_owned(),
            "verify deprivation source file format",
        )
    })?;
    let score_idx = find_column_index(
        &headers,
        &[
            "index of multiple deprivation (imd) score",
            "imd) score",
            "imd score",
        ],
    )
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing IMD score column in deprivation CSV".to_owned(),
            "verify deprivation source file format",
        )
    })?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS imd;
        CREATE TABLE imd (
            lsoa_code TEXT PRIMARY KEY,
            rank INTEGER,
            decile INTEGER,
            score REAL
        );
        CREATE INDEX idx_imd_rank ON imd(rank);
        CREATE INDEX idx_imd_decile ON imd(decile);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement =
        tx.prepare("INSERT INTO imd (lsoa_code, rank, decile, score) VALUES (?1, ?2, ?3, ?4)")?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let lsoa = cell(&row, lsoa_idx);
        if lsoa.is_none_or(str::is_empty) {
            continue;
        }

        statement.execute(rusqlite::params![
            lsoa,
            to_i64(cell(&row, rank_idx)),
            to_i64(cell(&row, decile_idx)),
            to_f64(cell(&row, score_idx)),
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
        format!("failed to parse deprivation CSV row: {error}"),
        "verify deprivation source file format",
    )
}
