#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use csv::ReaderBuilder;

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    collect_matching_files, download_file, extract_zip, file_name, find_first_matching_file,
    open_source_db, with_temp_dir,
};

const OFCOM_ZIP_URL: &str = "https://www.ofcom.org.uk/siteassets/resources/documents/research-and-data/multi-sector/infrastructure-research/connected-nations-2025/202507_fixed_broadband_coverage_r01.zip";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let top_zip_path = temp.path().join("broadband.zip");
    let extract_dir = temp.path().join("extract");

    download_file(OFCOM_ZIP_URL, &top_zip_path)?;
    extract_zip(&top_zip_path, &extract_dir)?;

    let nested_zip = find_first_matching_file(&extract_dir, &|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
            && file_name(path)
                .is_some_and(|name| name.to_ascii_lowercase().contains("fixed_pc_coverage"))
    })?
    .ok_or_else(|| {
        LetError::new(
            ErrorCode::NotFound,
            "nested postcode broadband zip not found".to_owned(),
            "verify broadband source archive structure",
        )
    })?;

    let nested_extract_dir = temp.path().join("postcode_data");
    extract_zip(&nested_zip, &nested_extract_dir)?;

    let csv_files = discover_csv_files(&nested_extract_dir)?;
    if csv_files.is_empty() {
        return Err(LetError::new(
            ErrorCode::NotFound,
            "no broadband CSV files found after extraction".to_owned(),
            "verify broadband source archive structure",
        ));
    }

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS postcodes;
        DROP TABLE IF EXISTS outward_aggregates;
        DROP TABLE IF EXISTS area_aggregates;

        CREATE TABLE postcodes (
            postcode TEXT PRIMARY KEY,
            postcode_display TEXT,
            outward TEXT,
            area TEXT,
            pct_under_2mbps REAL,
            pct_2_5mbps REAL,
            pct_5_10mbps REAL,
            pct_10_30mbps REAL,
            pct_30_300mbps REAL,
            pct_over_300mbps REAL,
            sfbb_availability REAL,
            ufbb_100_availability REAL,
            ufbb_availability REAL,
            gigabit_availability REAL,
            nga_availability REAL,
            pct_below_uso REAL,
            pct_unable_2mbps REAL,
            pct_unable_30mbps REAL
        );

        CREATE INDEX idx_outward ON postcodes(outward);
        CREATE INDEX idx_area ON postcodes(area);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "
        INSERT INTO postcodes (
            postcode, postcode_display, outward, area,
            pct_under_2mbps, pct_2_5mbps, pct_5_10mbps, pct_10_30mbps,
            pct_30_300mbps, pct_over_300mbps,
            sfbb_availability, ufbb_100_availability, ufbb_availability,
            gigabit_availability, nga_availability,
            pct_below_uso, pct_unable_2mbps, pct_unable_30mbps
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
        ",
    )?;

    let mut inserted = 0usize;
    for csv_path in csv_files {
        let mut reader = ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .from_path(&csv_path)
            .map_err(|error| {
                LetError::new(
                    ErrorCode::Parse,
                    format!(
                        "failed to open broadband CSV {}: {error}",
                        csv_path.display()
                    ),
                    "verify broadband source file format",
                )
            })?;

        for row in reader.records() {
            let row = row.map_err(csv_err)?;
            if row.len() < 19 {
                continue;
            }

            let postcode = row.get(0).map(str::trim).unwrap_or("").to_uppercase();
            let postcode_display = row.get(1).map(str::trim).unwrap_or("").to_uppercase();
            if postcode.is_empty() || postcode_display.is_empty() {
                continue;
            }

            let outward = postcode_display
                .split_ascii_whitespace()
                .next()
                .unwrap_or("")
                .to_owned();
            let area = row.get(2).map(str::trim).unwrap_or("").to_uppercase();

            statement.execute(rusqlite::params![
                postcode,
                postcode_display,
                outward,
                area,
                parse_percent(row.get(5)),
                parse_percent(row.get(6)),
                parse_percent(row.get(7)),
                parse_percent(row.get(8)),
                parse_percent(row.get(3)),
                parse_percent(row.get(4)),
                parse_percent(row.get(9)),
                parse_percent(row.get(10)),
                parse_percent(row.get(11)),
                parse_percent(row.get(16)),
                parse_percent(row.get(18)),
                parse_percent(row.get(17)),
                parse_percent(row.get(12)),
                parse_percent(row.get(15)),
            ])?;

            inserted += 1;
        }
    }

    drop(statement);
    tx.commit()?;

    connection.execute_batch(
        "
        CREATE TABLE outward_aggregates AS
        SELECT
            outward,
            COUNT(*) as postcode_count,
            ROUND(AVG(pct_over_300mbps), 1) as avg_pct_over_300mbps,
            ROUND(AVG(gigabit_availability), 1) as avg_gigabit_availability,
            ROUND(AVG(sfbb_availability), 1) as avg_sfbb_availability,
            ROUND(MIN(pct_over_300mbps), 1) as min_pct_over_300mbps,
            ROUND(MAX(pct_over_300mbps), 1) as max_pct_over_300mbps
        FROM postcodes
        GROUP BY outward;

        CREATE UNIQUE INDEX idx_outward_agg ON outward_aggregates(outward);

        CREATE TABLE area_aggregates AS
        SELECT
            area,
            COUNT(*) as postcode_count,
            ROUND(AVG(pct_over_300mbps), 1) as avg_pct_over_300mbps,
            ROUND(AVG(gigabit_availability), 1) as avg_gigabit_availability,
            ROUND(AVG(sfbb_availability), 1) as avg_sfbb_availability,
            ROUND(MIN(pct_over_300mbps), 1) as min_pct_over_300mbps,
            ROUND(MAX(pct_over_300mbps), 1) as max_pct_over_300mbps
        FROM postcodes
        GROUP BY area;

        CREATE UNIQUE INDEX idx_area_agg ON area_aggregates(area);
        ",
    )?;

    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(inserted)
}

fn discover_csv_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_matching_files(
        root,
        &|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
        },
        &mut files,
    )?;

    files.sort();
    Ok(files)
}

fn parse_percent(raw: Option<&str>) -> f64 {
    raw.and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse broadband CSV row: {error}"),
        "verify broadband source file format",
    )
}
