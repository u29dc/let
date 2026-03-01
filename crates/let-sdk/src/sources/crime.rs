#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashMap};
use std::env;
use std::path::{Path, PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    collect_matching_files, download_file, extract_zip, find_column_index, open_source_db,
    with_temp_dir,
};

const CRIME_ZIP_URL: &str = "https://data.police.uk/data/archive/latest.zip";

#[derive(Debug, Clone, Copy, Default)]
struct CrimeCounts {
    total: i64,
    violent: i64,
    burglary: i64,
    robbery: i64,
}

pub fn build(db_path: &Path) -> Result<usize> {
    let (zip_path, _temp_guard) = resolve_archive_path()?;

    let temp = with_temp_dir()?;
    let extract_dir = temp.path().join("extract");
    extract_zip(&zip_path, &extract_dir)?;

    let mut crime_files = Vec::new();
    collect_matching_files(
        &extract_dir,
        &|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with("-street.csv"))
        },
        &mut crime_files,
    )?;
    crime_files.sort();

    if crime_files.is_empty() {
        return Err(LetError::new(
            ErrorCode::NotFound,
            "no street crime CSV files found in archive".to_owned(),
            "verify crime source archive contents",
        ));
    }

    let mut monthly_counts: HashMap<(String, String), CrimeCounts> = HashMap::new();
    let mut months = BTreeSet::new();

    for file in crime_files {
        process_crime_file(&file, &mut monthly_counts, &mut months)?;
    }

    let month_vec = months.into_iter().collect::<Vec<_>>();
    let last_12_months = month_vec
        .iter()
        .rev()
        .take(12)
        .cloned()
        .collect::<BTreeSet<_>>();

    let month_start = last_12_months.iter().next().cloned();
    let month_end = last_12_months.iter().next_back().cloned();

    let mut totals_12m: HashMap<String, CrimeCounts> = HashMap::new();
    for ((lsoa, month), counts) in &monthly_counts {
        if !last_12_months.contains(month) {
            continue;
        }
        let aggregate = totals_12m.entry(lsoa.clone()).or_default();
        aggregate.total += counts.total;
        aggregate.violent += counts.violent;
        aggregate.burglary += counts.burglary;
        aggregate.robbery += counts.robbery;
    }

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS crime_monthly;
        DROP TABLE IF EXISTS crime_12m;

        CREATE TABLE crime_monthly (
            lsoa_code TEXT NOT NULL,
            month TEXT NOT NULL,
            total INTEGER,
            violent INTEGER,
            burglary INTEGER,
            robbery INTEGER,
            PRIMARY KEY (lsoa_code, month)
        );
        CREATE INDEX idx_crime_monthly_lsoa ON crime_monthly(lsoa_code);
        CREATE INDEX idx_crime_monthly_month ON crime_monthly(month);

        CREATE TABLE crime_12m (
            lsoa_code TEXT PRIMARY KEY,
            total INTEGER,
            violent INTEGER,
            burglary INTEGER,
            robbery INTEGER,
            month_start TEXT,
            month_end TEXT
        );
        ",
    )?;

    let tx = connection.transaction()?;

    {
        let mut monthly_statement = tx.prepare(
            "INSERT OR REPLACE INTO crime_monthly (lsoa_code, month, total, violent, burglary, robbery) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for ((lsoa, month), counts) in &monthly_counts {
            monthly_statement.execute(rusqlite::params![
                lsoa,
                month,
                counts.total,
                counts.violent,
                counts.burglary,
                counts.robbery,
            ])?;
        }
    }

    {
        let mut summary_statement = tx.prepare(
            "INSERT OR REPLACE INTO crime_12m (lsoa_code, total, violent, burglary, robbery, month_start, month_end) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;

        for (lsoa, counts) in &totals_12m {
            summary_statement.execute(rusqlite::params![
                lsoa,
                counts.total,
                counts.violent,
                counts.burglary,
                counts.robbery,
                month_start,
                month_end,
            ])?;
        }
    }

    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(totals_12m.len())
}

fn resolve_archive_path() -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    if let Ok(local_path) = env::var("CRIME_ARCHIVE_PATH") {
        return Ok((PathBuf::from(local_path), None));
    }

    let temp = with_temp_dir()?;
    let zip_path = temp.path().join("crime-latest.zip");
    download_file(CRIME_ZIP_URL, &zip_path)?;
    Ok((zip_path, Some(temp)))
}

fn process_crime_file(
    file_path: &Path,
    monthly_counts: &mut HashMap<(String, String), CrimeCounts>,
    months: &mut BTreeSet<String>,
) -> Result<()> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(file_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open crime CSV {}: {error}", file_path.display()),
                "verify crime source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!(
                    "failed to read crime CSV headers {}: {error}",
                    file_path.display()
                ),
                "verify crime source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let month_idx = find_column_index(&headers, &["month"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            format!("missing month column in {}", file_path.display()),
            "verify crime source file format",
        )
    })?;

    let lsoa_idx = find_column_index(&headers, &["lsoa code"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            format!("missing lsoa code column in {}", file_path.display()),
            "verify crime source file format",
        )
    })?;

    let crime_type_idx = find_column_index(&headers, &["crime type"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            format!("missing crime type column in {}", file_path.display()),
            "verify crime source file format",
        )
    })?;

    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(month) = cell(&row, month_idx) else {
            continue;
        };
        let Some(lsoa) = cell(&row, lsoa_idx) else {
            continue;
        };
        let crime_type = cell(&row, crime_type_idx)
            .unwrap_or_default()
            .to_ascii_lowercase();

        months.insert(month.to_owned());
        let entry = monthly_counts
            .entry((lsoa.to_owned(), month.to_owned()))
            .or_default();
        entry.total += 1;

        if crime_type.contains("violence") {
            entry.violent += 1;
        }
        if crime_type.contains("burglary") {
            entry.burglary += 1;
        }
        if crime_type.contains("robbery") {
            entry.robbery += 1;
        }
    }

    Ok(())
}

fn cell(row: &StringRecord, idx: usize) -> Option<&str> {
    row.get(idx)
        .map(str::trim)
        .and_then(|value| (!value.is_empty()).then_some(value))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse crime CSV row: {error}"),
        "verify crime source file format",
    )
}
