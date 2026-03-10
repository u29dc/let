#![forbid(unsafe_code)]

use std::path::Path;
use std::{env, path::PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    download_file, extract_zip, find_column_index, find_first_matching_file, open_source_db,
    to_i64, with_temp_dir,
};

const TS054_URL: &str = "https://www.nomisweb.co.uk/output/census/2021/census2021-ts054.zip";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let zip_path = resolve_input_zip_path(&temp)?;
    let extract_dir = temp.path().join("extract");

    extract_zip(&zip_path, &extract_dir)?;

    let csv_path = find_first_matching_file(&extract_dir, &|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("census2021-ts054-lsoa.csv"))
    })?
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::NotFound,
            "LSOA CSV not found in census TS054 archive".to_owned(),
            "verify census source archive contents",
        )
    })?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open census CSV: {error}"),
                "verify census source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read census CSV headers: {error}"),
                "verify census source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let geo_code =
        find_column_index(&headers, &["geography code", "geography"]).ok_or_else(|| {
            LetError::new(
                ErrorCode::Parse,
                "missing geography code column in census CSV".to_owned(),
                "verify census source file format",
            )
        })?;
    let total = find_column_index(
        &headers,
        &["all households", "total households", "households: total"],
    )
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing total households column in census CSV".to_owned(),
            "verify census source file format",
        )
    })?;
    let council = find_column_index(
        &headers,
        &[
            "social rented: council",
            "rents from council",
            "social rented: local authority",
            "social rented",
        ],
    )
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing council housing column in census CSV".to_owned(),
            "verify census source file format",
        )
    })?;
    let housing = find_column_index(
        &headers,
        &[
            "social rented: housing association",
            "other social rented",
            "social rented: registered social landlord",
            "housing association",
        ],
    )
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing housing association column in census CSV".to_owned(),
            "verify census source file format",
        )
    })?;
    let owned = find_column_index(&headers, &["tenure of household: owned"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing owned column in census CSV".to_owned(),
            "verify census source file format",
        )
    })?;
    let shared = find_column_index(&headers, &["shared ownership"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing shared ownership column in census CSV".to_owned(),
            "verify census source file format",
        )
    })?;
    let social_total = find_column_index(&headers, &["tenure of household: social rented"])
        .ok_or_else(|| {
            LetError::new(
                ErrorCode::Parse,
                "missing social rented total column in census CSV".to_owned(),
                "verify census source file format",
            )
        })?;
    let private_rented = find_column_index(&headers, &["tenure of household: private rented"])
        .ok_or_else(|| {
            LetError::new(
                ErrorCode::Parse,
                "missing private rented column in census CSV".to_owned(),
                "verify census source file format",
            )
        })?;
    let rent_free = find_column_index(&headers, &["tenure of household: lives rent free"])
        .ok_or_else(|| {
            LetError::new(
                ErrorCode::Parse,
                "missing rent free column in census CSV".to_owned(),
                "verify census source file format",
            )
        })?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS tenure;
        CREATE TABLE tenure (
            lsoa_code TEXT PRIMARY KEY,
            total_households INTEGER,
            owned INTEGER,
            shared_ownership INTEGER,
            social_rented_total INTEGER,
            council INTEGER,
            housing_association INTEGER,
            private_rented INTEGER,
            lives_rent_free INTEGER,
            social_housing_pct REAL,
            private_rented_pct REAL,
            owned_pct REAL
        );
        CREATE INDEX idx_tenure_social_pct ON tenure(social_housing_pct);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "INSERT OR REPLACE INTO tenure (lsoa_code, total_households, owned, shared_ownership, social_rented_total, council, housing_association, private_rented, lives_rent_free, social_housing_pct, private_rented_pct, owned_pct) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let lsoa = cell(&row, geo_code);
        let Some(lsoa_code) = lsoa else {
            continue;
        };

        let total_households = to_i64(cell(&row, total));
        let owned_households = to_i64(cell(&row, owned));
        let shared_households = to_i64(cell(&row, shared));
        let social_total_households = to_i64(cell(&row, social_total));
        let council_households = to_i64(cell(&row, council));
        let housing_association_households = to_i64(cell(&row, housing));
        let private_rented_households = to_i64(cell(&row, private_rented));
        let rent_free_households = to_i64(cell(&row, rent_free));

        let social_housing_pct = match total_households {
            Some(denom) if denom > 0 => {
                let council_value = council_households.unwrap_or(0);
                let housing_value = housing_association_households.unwrap_or(0);
                let pct = ((council_value + housing_value) as f64 / denom as f64) * 100.0;
                Some((pct * 100.0).round() / 100.0)
            }
            _ => None,
        };
        let private_rented_pct = match (total_households, private_rented_households) {
            (Some(denom), Some(private)) if denom > 0 => {
                Some(((private as f64 / denom as f64) * 10000.0).round() / 100.0)
            }
            _ => None,
        };
        let owned_pct = match (total_households, owned_households) {
            (Some(denom), Some(owned)) if denom > 0 => {
                Some(((owned as f64 / denom as f64) * 10000.0).round() / 100.0)
            }
            _ => None,
        };

        statement.execute(rusqlite::params![
            lsoa_code,
            total_households,
            owned_households,
            shared_households,
            social_total_households,
            council_households,
            housing_association_households,
            private_rented_households,
            rent_free_households,
            social_housing_pct,
            private_rented_pct,
            owned_pct,
        ])?;

        inserted += 1;
    }

    drop(statement);
    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(inserted)
}

fn resolve_input_zip_path(temp: &tempfile::TempDir) -> Result<PathBuf> {
    if let Ok(local_path) = env::var("CENSUS_TS054_ZIP_PATH") {
        return Ok(PathBuf::from(local_path));
    }

    let zip_path = temp.path().join("census-ts054.zip");
    let url = env::var("CENSUS_TS054_ZIP_URL").unwrap_or_else(|_| TS054_URL.to_owned());
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
        format!("failed to parse census CSV row: {error}"),
        "verify census source file format",
    )
}
