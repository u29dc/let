#![forbid(unsafe_code)]

use std::path::Path;

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    download_file, extract_zip, find_first_matching_file, normalize_postcode, open_source_db,
    to_f64, with_temp_dir,
};

const ONSPD_ZIP_URL: &str =
    "https://www.arcgis.com/sharing/rest/content/items/3be72478d8454b59bb86ba97b4ee325b/data";

#[derive(Debug, Clone)]
struct HeaderIndex {
    postcode_display: usize,
    lat: usize,
    lng: usize,
    lsoa_code: usize,
    lsoa_name: Option<usize>,
    msoa_code: usize,
    msoa_name: Option<usize>,
    country_code: Option<usize>,
}

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let zip_path = temp.path().join("postcodes.zip");
    let extract_dir = temp.path().join("extract");

    download_file(ONSPD_ZIP_URL, &zip_path)?;
    extract_zip(&zip_path, &extract_dir)?;

    let csv_path = find_first_matching_file(&extract_dir, &|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("ONSPD_") && name.ends_with("_UK.csv"))
    })?
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::NotFound,
            "ONSPD CSV not found in archive".to_owned(),
            "verify postcodes source archive contents",
        )
    })?;

    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(&csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open postcodes CSV: {error}"),
                "verify postcodes source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read postcodes CSV headers: {error}"),
                "verify postcodes source file format",
            )
        })?
        .iter()
        .map(clean_header)
        .collect::<Vec<_>>();

    let header_index = resolve_header_indexes(&headers)?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS postcodes;
        CREATE TABLE postcodes (
            postcode TEXT PRIMARY KEY,
            postcode_display TEXT,
            lat REAL,
            lng REAL,
            lsoa_code TEXT,
            lsoa_name TEXT,
            msoa_code TEXT,
            msoa_name TEXT,
            country_code TEXT
        );
        CREATE INDEX idx_postcodes_lsoa ON postcodes(lsoa_code);
        CREATE INDEX idx_postcodes_msoa ON postcodes(msoa_code);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "INSERT INTO postcodes (postcode, postcode_display, lat, lng, lsoa_code, lsoa_name, msoa_code, msoa_name, country_code) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(display_raw) = cell(&row, header_index.postcode_display) else {
            continue;
        };

        let normalized = normalize_postcode(display_raw);
        if normalized.is_empty() {
            continue;
        }

        statement.execute(rusqlite::params![
            normalized,
            display_raw.trim().to_uppercase(),
            to_f64(cell(&row, header_index.lat)),
            to_f64(cell(&row, header_index.lng)),
            string_cell(&row, header_index.lsoa_code),
            optional_cell(&row, header_index.lsoa_name),
            string_cell(&row, header_index.msoa_code),
            optional_cell(&row, header_index.msoa_name),
            optional_cell(&row, header_index.country_code),
        ])?;

        inserted += 1;
    }

    drop(statement);
    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(inserted)
}

fn resolve_header_indexes(headers: &[String]) -> Result<HeaderIndex> {
    let postcode_display = pick_exact(headers, &["pcds", "pcd"]).or_else(|| {
        pick_contains(
            headers,
            &["postcode (8 char)", "postcode (7 char)", "postcode"],
        )
    });
    let postcode = pick_exact(headers, &["pcd2", "pcd", "pcds"]).or_else(|| {
        pick_contains(
            headers,
            &["postcode (7 char)", "postcode (8 char)", "postcode"],
        )
    });
    let lat =
        pick_exact(headers, &["lat", "latitude"]).or_else(|| pick_contains(headers, &["latitude"]));
    let lng = pick_exact(headers, &["long", "longitude"])
        .or_else(|| pick_contains(headers, &["longitude"]));

    let lsoa_code = pick_exact(
        headers,
        &[
            "lsoa21cd", "lsoa11cd", "lsoa01cd", "lsoa21", "lsoa11", "lsoa01",
        ],
    )
    .or_else(|| {
        pick_contains(
            headers,
            &[
                "lower layer super output area code (2021)",
                "lower layer super output area code (2011)",
                "lower layer super output area code",
            ],
        )
    });
    let lsoa_name = pick_exact(headers, &["lsoa21nm", "lsoa11nm", "lsoa01nm"])
        .or_else(|| pick_contains(headers, &["lower layer super output area name"]));

    let msoa_code = pick_exact(
        headers,
        &[
            "msoa21cd", "msoa11cd", "msoa01cd", "msoa21", "msoa11", "msoa01",
        ],
    )
    .or_else(|| {
        pick_contains(
            headers,
            &[
                "middle layer super output area code (2021)",
                "middle layer super output area code (2011)",
                "middle layer super output area code",
            ],
        )
    });
    let msoa_name = pick_exact(headers, &["msoa21nm", "msoa11nm", "msoa01nm"])
        .or_else(|| pick_contains(headers, &["middle layer super output area name"]));

    let country_code = pick_exact(headers, &["ctry", "ctry21cd", "ctry11cd"])
        .or_else(|| pick_contains(headers, &["country code"]));

    if postcode.is_none() {
        return Err(required_column_err("postcode"));
    }
    let Some(postcode_display) = postcode_display else {
        return Err(required_column_err("postcode display"));
    };
    let Some(lat) = lat else {
        return Err(required_column_err("latitude"));
    };
    let Some(lng) = lng else {
        return Err(required_column_err("longitude"));
    };
    let Some(lsoa_code) = lsoa_code else {
        return Err(required_column_err("lsoa code"));
    };
    let Some(msoa_code) = msoa_code else {
        return Err(required_column_err("msoa code"));
    };

    Ok(HeaderIndex {
        postcode_display,
        lat,
        lng,
        lsoa_code,
        lsoa_name,
        msoa_code,
        msoa_name,
        country_code,
    })
}

fn required_column_err(column: &str) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("required `{column}` column not found in postcodes CSV"),
        "verify postcodes source file format",
    )
}

fn clean_header(value: &str) -> String {
    value.trim().trim_matches('"').to_ascii_lowercase()
}

fn pick_exact(headers: &[String], names: &[&str]) -> Option<usize> {
    for name in names {
        if let Some(idx) = headers.iter().position(|header| header == *name) {
            return Some(idx);
        }
    }
    None
}

fn pick_contains(headers: &[String], patterns: &[&str]) -> Option<usize> {
    for pattern in patterns {
        if let Some(idx) = headers.iter().position(|header| header.contains(pattern)) {
            return Some(idx);
        }
    }
    None
}

fn cell(row: &StringRecord, idx: usize) -> Option<&str> {
    row.get(idx)
        .map(str::trim)
        .and_then(|value| (!value.is_empty()).then_some(value))
}

fn string_cell(row: &StringRecord, idx: usize) -> Option<String> {
    cell(row, idx).map(|value| value.trim().to_owned())
}

fn optional_cell(row: &StringRecord, idx: Option<usize>) -> Option<String> {
    idx.and_then(|index| string_cell(row, index))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse postcodes CSV row: {error}"),
        "verify postcodes source file format",
    )
}
