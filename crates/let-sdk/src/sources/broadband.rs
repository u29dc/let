#![forbid(unsafe_code)]

use std::env;
use std::path::{Path, PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    collect_matching_files, download_file, extract_zip, file_name, find_column_index,
    find_first_matching_file, open_source_db, to_f64, to_i64, with_temp_dir,
};

const OFCOM_ZIP_URL: &str = "https://www.ofcom.org.uk/siteassets/resources/documents/research-and-data/multi-sector/infrastructure-research/connected-nations-2025/202507_fixed_broadband_coverage_r01.zip";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let top_zip_path = resolve_top_level_archive(&temp)?;
    let extract_dir = temp.path().join("extract");

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

    let postcode_csvs = discover_csv_files(&nested_extract_dir)?;
    if postcode_csvs.is_empty() {
        return Err(LetError::new(
            ErrorCode::NotFound,
            "no broadband postcode CSV files found after extraction".to_owned(),
            "verify broadband source archive structure",
        ));
    }

    let laua_coverage_csv = find_first_matching_file(&extract_dir, &|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.to_ascii_lowercase().contains("fixed_laua_coverage"))
    })?;

    let full_fibre_takeup_csv = find_first_matching_file(&extract_dir, &|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| {
                    name.to_ascii_lowercase()
                        .contains("full_fibre_take-up_laua")
                })
    })?;

    let national_summary_csv = find_first_matching_file(&extract_dir, &|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| {
                    name.to_ascii_lowercase()
                        .contains("fixed_coverage_uk_and_nations")
                })
    })?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS postcodes;
        DROP TABLE IF EXISTS outward_aggregates;
        DROP TABLE IF EXISTS area_aggregates;
        DROP TABLE IF EXISTS laua_coverage;
        DROP TABLE IF EXISTS laua_full_fibre_takeup;
        DROP TABLE IF EXISTS national_summary;

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

        CREATE TABLE laua_coverage (
            laua_code TEXT PRIMARY KEY,
            laua_name TEXT,
            all_premises INTEGER,
            matched_premises INTEGER,
            sfbb_availability REAL,
            ufbb_100_availability REAL,
            ufbb_availability REAL,
            full_fibre_availability REAL,
            gigabit_availability REAL,
            pct_unable_30mbps REAL,
            pct_below_uso REAL,
            pct_over_300mbps REAL
        );

        CREATE TABLE laua_full_fibre_takeup (
            laua_code TEXT PRIMARY KEY,
            laua_name TEXT,
            takeup_of_full_fibre REAL,
            takeup_of_all_premises REAL
        );

        CREATE TABLE national_summary (
            location TEXT NOT NULL,
            premise_type TEXT,
            rurality TEXT,
            speed_band TEXT,
            premises_pct REAL,
            premises_count INTEGER,
            period TEXT,
            PRIMARY KEY (location, premise_type, rurality, speed_band, period)
        );
        ",
    )?;

    let tx = connection.transaction()?;
    let inserted = insert_postcode_rows(&tx, &postcode_csvs)?;
    if let Some(path) = laua_coverage_csv.as_ref() {
        insert_laua_coverage_rows(&tx, path)?;
    }
    if let Some(path) = full_fibre_takeup_csv.as_ref() {
        insert_laua_takeup_rows(&tx, path)?;
    }
    if let Some(path) = national_summary_csv.as_ref() {
        insert_national_summary_rows(&tx, path)?;
    }
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

fn resolve_top_level_archive(temp: &tempfile::TempDir) -> Result<PathBuf> {
    if let Ok(path) = env::var("BROADBAND_ZIP_PATH") {
        return Ok(PathBuf::from(path));
    }

    let zip_path = temp.path().join("broadband.zip");
    let download_url = env::var("BROADBAND_ZIP_URL").unwrap_or_else(|_| OFCOM_ZIP_URL.to_owned());
    download_file(&download_url, &zip_path)?;
    Ok(zip_path)
}

fn insert_postcode_rows(tx: &rusqlite::Transaction<'_>, csv_files: &[PathBuf]) -> Result<usize> {
    let mut statement = tx.prepare(
        "
        INSERT OR REPLACE INTO postcodes (
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
        let mut reader = open_csv(csv_path, "broadband postcode")?;
        let headers = parse_headers(&mut reader)?;
        let columns = PostcodeColumns::resolve(&headers, csv_path)?;

        for row in reader.records() {
            let row = row.map_err(csv_err)?;
            let postcode = cell(&row, columns.postcode)
                .map(|value| value.to_uppercase())
                .unwrap_or_default();
            let postcode_display = cell(&row, columns.postcode_display)
                .map(|value| value.to_uppercase())
                .unwrap_or_default();
            if postcode.is_empty() || postcode_display.is_empty() {
                continue;
            }

            let outward = postcode_display
                .split_ascii_whitespace()
                .next()
                .unwrap_or("")
                .to_owned();
            let area = cell(&row, columns.area)
                .map(|value| value.to_uppercase())
                .unwrap_or_default();

            statement.execute(rusqlite::params![
                postcode,
                postcode_display,
                outward,
                area,
                opt_f64(&row, columns.pct_under_2mbps),
                opt_f64(&row, columns.pct_2_5mbps),
                opt_f64(&row, columns.pct_5_10mbps),
                opt_f64(&row, columns.pct_10_30mbps),
                opt_f64(&row, columns.pct_30_300mbps),
                opt_f64(&row, columns.pct_over_300mbps),
                opt_f64(&row, columns.sfbb_availability),
                opt_f64(&row, columns.ufbb_100_availability),
                opt_f64(&row, columns.ufbb_availability),
                opt_f64(&row, columns.gigabit_availability),
                opt_f64(&row, columns.nga_availability),
                opt_f64(&row, columns.pct_below_uso),
                opt_f64(&row, columns.pct_unable_2mbps),
                opt_f64(&row, columns.pct_unable_30mbps),
            ])?;

            inserted += 1;
        }
    }

    Ok(inserted)
}

fn insert_laua_coverage_rows(tx: &rusqlite::Transaction<'_>, csv_path: &Path) -> Result<()> {
    let mut reader = open_csv(csv_path, "broadband LAUA coverage")?;
    let headers = parse_headers(&mut reader)?;

    let code = req_col(&headers, &["laua"], "laua code", csv_path)?;
    let name = req_col(&headers, &["laua_name"], "laua name", csv_path)?;
    let all_premises = req_col(&headers, &["all premises"], "all premises", csv_path)?;
    let matched = req_col(
        &headers,
        &["all matched premises"],
        "all matched premises",
        csv_path,
    )?;
    let sfbb = req_col(
        &headers,
        &["sfbb availability (% premises)"],
        "sfbb availability",
        csv_path,
    )?;
    let ufbb100 = req_col(
        &headers,
        &["ufbb (100mbit/s) availability (% premises)"],
        "ufbb 100 availability",
        csv_path,
    )?;
    let ufbb = req_col(
        &headers,
        &["ufbb availability (% premises)"],
        "ufbb availability",
        csv_path,
    )?;
    let full_fibre = req_col(
        &headers,
        &["full fibre availability (% premises)"],
        "full fibre availability",
        csv_path,
    )?;
    let gigabit = req_col(
        &headers,
        &["gigabit availability (% premises)"],
        "gigabit availability",
        csv_path,
    )?;
    let unable_30 = req_col(
        &headers,
        &["% of premises unable to receive 30mbit/s"],
        "pct unable 30mbps",
        csv_path,
    )?;
    let below_uso = req_col(
        &headers,
        &["% of premises below the uso"],
        "pct below uso",
        csv_path,
    )?;
    let over_300 = req_col(
        &headers,
        &["% of premises with >=300mbit/s download speed"],
        "pct over 300mbps",
        csv_path,
    )?;

    let mut statement = tx.prepare(
        "
        INSERT OR REPLACE INTO laua_coverage (
            laua_code, laua_name, all_premises, matched_premises,
            sfbb_availability, ufbb_100_availability, ufbb_availability,
            full_fibre_availability, gigabit_availability, pct_unable_30mbps,
            pct_below_uso, pct_over_300mbps
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
    )?;

    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(laua_code) = cell(&row, code) else {
            continue;
        };
        statement.execute(rusqlite::params![
            laua_code,
            cell(&row, name),
            to_i64(cell(&row, all_premises)),
            to_i64(cell(&row, matched)),
            opt_f64(&row, sfbb),
            opt_f64(&row, ufbb100),
            opt_f64(&row, ufbb),
            opt_f64(&row, full_fibre),
            opt_f64(&row, gigabit),
            opt_f64(&row, unable_30),
            opt_f64(&row, below_uso),
            opt_f64(&row, over_300),
        ])?;
    }

    Ok(())
}

fn insert_laua_takeup_rows(tx: &rusqlite::Transaction<'_>, csv_path: &Path) -> Result<()> {
    let mut reader = open_csv(csv_path, "broadband full fibre take-up")?;
    let headers = parse_headers(&mut reader)?;

    let code = req_col(&headers, &["laua"], "laua code", csv_path)?;
    let name = req_col(&headers, &["laua_name"], "laua name", csv_path)?;
    let takeup_full = req_col(
        &headers,
        &["full-fibre take-up (% of full-fibre coverage)"],
        "full-fibre take-up of full-fibre coverage",
        csv_path,
    )?;
    let takeup_all = req_col(
        &headers,
        &["full-fibre take-up (% of all premises)"],
        "full-fibre take-up of all premises",
        csv_path,
    )?;

    let mut statement = tx.prepare(
        "
        INSERT OR REPLACE INTO laua_full_fibre_takeup (
            laua_code, laua_name, takeup_of_full_fibre, takeup_of_all_premises
        ) VALUES (?1, ?2, ?3, ?4)
        ",
    )?;

    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(laua_code) = cell(&row, code) else {
            continue;
        };

        statement.execute(rusqlite::params![
            laua_code,
            cell(&row, name),
            opt_f64(&row, takeup_full),
            opt_f64(&row, takeup_all),
        ])?;
    }

    Ok(())
}

fn insert_national_summary_rows(tx: &rusqlite::Transaction<'_>, csv_path: &Path) -> Result<()> {
    let mut reader = open_csv(csv_path, "broadband UK and nations summary")?;
    let headers = parse_headers(&mut reader)?;

    let location = req_col(&headers, &["location"], "location", csv_path)?;
    let premise_type = req_col(&headers, &["premise type"], "premise type", csv_path)?;
    let rurality = req_col(&headers, &["rurality"], "rurality", csv_path)?;
    let speed = req_col(&headers, &["speed"], "speed", csv_path)?;
    let pct = req_col(&headers, &["% premises"], "percent premises", csv_path)?;
    let premises = req_col(&headers, &["premises"], "premises", csv_path)?;
    let date = req_col(&headers, &["date"], "date", csv_path)?;

    let mut statement = tx.prepare(
        "
        INSERT OR REPLACE INTO national_summary (
            location, premise_type, rurality, speed_band, premises_pct, premises_count, period
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
    )?;

    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(location_value) = cell(&row, location) else {
            continue;
        };

        statement.execute(rusqlite::params![
            location_value,
            cell(&row, premise_type),
            cell(&row, rurality),
            cell(&row, speed),
            opt_f64(&row, pct),
            to_i64(cell(&row, premises)),
            cell(&row, date),
        ])?;
    }

    Ok(())
}

fn open_csv(path: &Path, label: &str) -> Result<csv::Reader<std::fs::File>> {
    ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open {label} CSV {}: {error}", path.display()),
                "verify broadband source file format",
            )
        })
}

fn parse_headers(reader: &mut csv::Reader<std::fs::File>) -> Result<Vec<String>> {
    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read broadband CSV headers: {error}"),
                "verify broadband source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();
    Ok(headers)
}

fn req_col(headers: &[String], patterns: &[&str], label: &str, path: &Path) -> Result<usize> {
    find_column_index(headers, patterns).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            format!(
                "missing `{label}` column in broadband CSV {}",
                path.display()
            ),
            "verify broadband source file format",
        )
    })
}

#[derive(Debug, Clone, Copy)]
struct PostcodeColumns {
    postcode: usize,
    postcode_display: usize,
    area: usize,
    pct_30_300mbps: usize,
    pct_over_300mbps: usize,
    pct_under_2mbps: usize,
    pct_2_5mbps: usize,
    pct_5_10mbps: usize,
    pct_10_30mbps: usize,
    sfbb_availability: usize,
    ufbb_100_availability: usize,
    ufbb_availability: usize,
    pct_unable_2mbps: usize,
    pct_unable_30mbps: usize,
    gigabit_availability: usize,
    pct_below_uso: usize,
    nga_availability: usize,
}

impl PostcodeColumns {
    fn resolve(headers: &[String], path: &Path) -> Result<Self> {
        Ok(Self {
            postcode: req_col(headers, &["postcode"], "postcode", path)?,
            postcode_display: req_col(
                headers,
                &["postcode_space", "postcode (8 char)"],
                "postcode display",
                path,
            )?,
            area: req_col(headers, &["postcode area"], "postcode area", path)?,
            pct_30_300mbps: req_col(
                headers,
                &["% of premises with 30<300mbit/s download speed"],
                "30<300mbps",
                path,
            )?,
            pct_over_300mbps: req_col(
                headers,
                &["% of premises with >=300mbit/s download speed"],
                ">=300mbps",
                path,
            )?,
            pct_under_2mbps: req_col(
                headers,
                &["% of premises with 0<2mbit/s download speed"],
                "0<2mbps",
                path,
            )?,
            pct_2_5mbps: req_col(
                headers,
                &["% of premises with 2<5mbit/s download speed"],
                "2<5mbps",
                path,
            )?,
            pct_5_10mbps: req_col(
                headers,
                &["% of premises with 5<10mbit/s download speed"],
                "5<10mbps",
                path,
            )?,
            pct_10_30mbps: req_col(
                headers,
                &["% of premises with 10<30mbit/s download speed"],
                "10<30mbps",
                path,
            )?,
            sfbb_availability: req_col(
                headers,
                &["sfbb availability (% premises)"],
                "sfbb availability",
                path,
            )?,
            ufbb_100_availability: req_col(
                headers,
                &["ufbb (100mbit/s) availability (% premises)"],
                "ufbb 100 availability",
                path,
            )?,
            ufbb_availability: req_col(
                headers,
                &["ufbb availability (% premises)"],
                "ufbb availability",
                path,
            )?,
            pct_unable_2mbps: req_col(
                headers,
                &["% of premises unable to receive 2mbit/s"],
                "unable 2mbps",
                path,
            )?,
            pct_unable_30mbps: req_col(
                headers,
                &["% of premises unable to receive 30mbit/s"],
                "unable 30mbps",
                path,
            )?,
            gigabit_availability: req_col(
                headers,
                &["gigabit availability (% premises)"],
                "gigabit availability",
                path,
            )?,
            pct_below_uso: req_col(
                headers,
                &["% of premises below the uso"],
                "pct below uso",
                path,
            )?,
            nga_availability: req_col(
                headers,
                &["% of premises with nga"],
                "nga availability",
                path,
            )?,
        })
    }
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

fn cell(row: &StringRecord, idx: usize) -> Option<&str> {
    row.get(idx)
        .map(str::trim)
        .and_then(|value| (!value.is_empty()).then_some(value))
}

fn opt_f64(row: &StringRecord, idx: usize) -> Option<f64> {
    to_f64(cell(row, idx))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse broadband CSV row: {error}"),
        "verify broadband source file format",
    )
}
