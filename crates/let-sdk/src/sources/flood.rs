#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    download_file_checked, extract_zip, find_column_index, find_first_matching_file,
    normalize_postcode, open_source_db, to_i64, verify_file_checksum_from_env, with_temp_dir,
};

const FLOOD_POSTCODE_RISK_CSV_URL: &str = "https://environment.data.gov.uk/api/file/download?fileDataSetId=fb921496-1788-4fc2-b469-7b51e2a45553&fileName=Postcodes_Risk_Assessment_All.csv";
const FLOOD_ROFRS_ZIP_URL: &str = "https://environment.data.gov.uk/api/file/download?fileDataSetId=97781741-2982-4802-af2e-313fe3fd8f7e&fileName=RoFRS_Postcodes_in_Areas_at_Risk.zip";

#[derive(Debug, Clone, Default)]
struct FloodRecord {
    risk: Option<String>,
    source: Option<String>,
    high_cnt: Option<i64>,
    med_cnt: Option<i64>,
    low_cnt: Option<i64>,
    groundwater_risk: Option<String>,
    rofrs_total_cnt: Option<i64>,
    rofrs_high_cnt: Option<i64>,
    rofrs_medium_cnt: Option<i64>,
    rofrs_low_cnt: Option<i64>,
    rofrs_very_low_cnt: Option<i64>,
}

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;

    let primary_csv = resolve_primary_csv_path(&temp)?;
    let rofrs_csv = resolve_rofrs_csv_path(&temp)?;

    let mut by_postcode: HashMap<String, FloodRecord> = HashMap::new();
    load_primary_records(&primary_csv, &mut by_postcode)?;
    if let Some(path) = rofrs_csv.as_ref() {
        load_rofrs_records(path, &mut by_postcode)?;
    }

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS flood;
        CREATE TABLE flood (
            postcode TEXT PRIMARY KEY,
            risk TEXT,
            source TEXT,
            high_cnt INTEGER,
            med_cnt INTEGER,
            low_cnt INTEGER,
            groundwater_risk TEXT,
            rofrs_total_cnt INTEGER,
            rofrs_high_cnt INTEGER,
            rofrs_medium_cnt INTEGER,
            rofrs_low_cnt INTEGER,
            rofrs_very_low_cnt INTEGER
        );
        CREATE INDEX idx_flood_risk ON flood(risk);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "
        INSERT OR REPLACE INTO flood (
            postcode, risk, source, high_cnt, med_cnt, low_cnt, groundwater_risk,
            rofrs_total_cnt, rofrs_high_cnt, rofrs_medium_cnt, rofrs_low_cnt, rofrs_very_low_cnt
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
    )?;

    let mut inserted = 0usize;
    let mut postcodes = by_postcode.keys().cloned().collect::<Vec<_>>();
    postcodes.sort();

    for postcode in postcodes {
        let Some(record) = by_postcode.get(&postcode) else {
            continue;
        };

        statement.execute(rusqlite::params![
            postcode,
            record.risk,
            record.source,
            record.high_cnt,
            record.med_cnt,
            record.low_cnt,
            record.groundwater_risk,
            record.rofrs_total_cnt,
            record.rofrs_high_cnt,
            record.rofrs_medium_cnt,
            record.rofrs_low_cnt,
            record.rofrs_very_low_cnt,
        ])?;
        inserted += 1;
    }

    drop(statement);
    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(inserted)
}

fn resolve_primary_csv_path(temp: &tempfile::TempDir) -> Result<PathBuf> {
    if let Ok(local_path) = env::var("FLOOD_CSV_PATH") {
        let path = PathBuf::from(local_path);
        verify_file_checksum_from_env(&path, &["FLOOD_CSV_SHA256"], "flood postcode CSV")?;
        return Ok(path);
    }

    let csv_path = temp.path().join("flood_postcode_risk.csv");
    let url = env::var("FLOOD_CSV_URL").unwrap_or_else(|_| FLOOD_POSTCODE_RISK_CSV_URL.to_owned());
    download_file_checked(&url, &csv_path, &["FLOOD_CSV_SHA256"], "flood postcode CSV")?;
    Ok(csv_path)
}

fn resolve_rofrs_csv_path(temp: &tempfile::TempDir) -> Result<Option<PathBuf>> {
    if let Ok(local_csv) = env::var("FLOOD_ROFRS_CSV_PATH") {
        let path = PathBuf::from(local_csv);
        verify_file_checksum_from_env(&path, &["FLOOD_ROFRS_CSV_SHA256"], "flood RoFRS CSV")?;
        return Ok(Some(path));
    }

    if let Ok(local_zip) = env::var("FLOOD_ROFRS_ZIP_PATH") {
        verify_file_checksum_from_env(
            Path::new(&local_zip),
            &["FLOOD_ROFRS_ZIP_SHA256"],
            "flood RoFRS ZIP",
        )?;
        let extract_dir = temp.path().join("rofrs-local");
        extract_zip(Path::new(&local_zip), &extract_dir)?;
        return discover_rofrs_csv(&extract_dir);
    }

    let zip_path = temp.path().join("flood_rofrs.zip");
    let extract_dir = temp.path().join("rofrs");
    let url = env::var("FLOOD_ROFRS_ZIP_URL").unwrap_or_else(|_| FLOOD_ROFRS_ZIP_URL.to_owned());

    if let Err(error) = download_file_checked(
        &url,
        &zip_path,
        &["FLOOD_ROFRS_ZIP_SHA256"],
        "flood RoFRS ZIP",
    ) {
        let checksum_enforced = env::var("FLOOD_ROFRS_ZIP_SHA256")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);

        if checksum_enforced {
            return Err(error);
        }

        return Ok(None);
    }

    extract_zip(&zip_path, &extract_dir)?;
    discover_rofrs_csv(&extract_dir)
}

fn discover_rofrs_csv(root: &Path) -> Result<Option<PathBuf>> {
    let discovered = find_first_matching_file(root, &|path| {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.to_ascii_lowercase().contains("rofrs"))
    })?;

    if let Some(path) = discovered.as_ref() {
        verify_file_checksum_from_env(path, &["FLOOD_ROFRS_CSV_SHA256"], "flood RoFRS CSV")?;
    }

    Ok(discovered)
}

fn load_primary_records(path: &Path, by_postcode: &mut HashMap<String, FloodRecord>) -> Result<()> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open flood CSV {}: {error}", path.display()),
                "verify flood source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!(
                    "failed to read flood CSV headers {}: {error}",
                    path.display()
                ),
                "verify flood source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let postcode_idx = find_column_index(&headers, &["postcode", "pc"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing postcode column in flood CSV".to_owned(),
            "verify flood source file format",
        )
    })?;

    let risk_idx = find_column_index(
        &headers,
        &["overall risk", "risk overall", "risk category", "risk"],
    );
    let high_idx = find_column_index(&headers, &["high_cnt", "high count"]);
    let med_idx = find_column_index(&headers, &["med_cnt", "medium count"]);
    let low_idx = find_column_index(&headers, &["low_cnt", "low count"]);
    let groundwater_idx = find_column_index(
        &headers,
        &["gwtr_risk", "groundwater risk", "ground water risk"],
    );

    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(raw_postcode) = cell(&row, postcode_idx) else {
            continue;
        };
        let normalized = normalize_postcode(raw_postcode);
        if normalized.is_empty() {
            continue;
        }

        let high_cnt = high_idx.and_then(|idx| to_i64(cell(&row, idx)));
        let med_cnt = med_idx.and_then(|idx| to_i64(cell(&row, idx)));
        let low_cnt = low_idx.and_then(|idx| to_i64(cell(&row, idx)));

        let risk = risk_idx
            .and_then(|idx| cell(&row, idx))
            .map(normalize_risk_text)
            .or_else(|| derive_risk_from_counts(high_cnt, med_cnt, low_cnt));

        let groundwater_risk = groundwater_idx
            .and_then(|idx| cell(&row, idx))
            .map(normalize_risk_text);

        let entry = by_postcode.entry(normalized).or_default();
        entry.risk = risk.or(entry.risk.clone());
        entry.source = Some(match entry.source.as_deref() {
            Some("ea-rofrs") => "ea-postcode-risk+ea-rofrs".to_owned(),
            Some(existing) => existing.to_owned(),
            None => "ea-postcode-risk".to_owned(),
        });
        entry.high_cnt = high_cnt.or(entry.high_cnt);
        entry.med_cnt = med_cnt.or(entry.med_cnt);
        entry.low_cnt = low_cnt.or(entry.low_cnt);
        entry.groundwater_risk = groundwater_risk.or(entry.groundwater_risk.clone());
    }

    Ok(())
}

fn load_rofrs_records(path: &Path, by_postcode: &mut HashMap<String, FloodRecord>) -> Result<()> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to open RoFRS CSV {}: {error}", path.display()),
                "verify RoFRS source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!(
                    "failed to read RoFRS CSV headers {}: {error}",
                    path.display()
                ),
                "verify RoFRS source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let postcode_idx = find_column_index(&headers, &["pc", "postcode"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing postcode column in RoFRS CSV".to_owned(),
            "verify RoFRS source file format",
        )
    })?;

    let total_idx = find_column_index(&headers, &["cntpc"]);
    let high_idx = find_column_index(&headers, &["tot_cnt_high", "high"]);
    let medium_idx = find_column_index(&headers, &["tot_cnt_medium", "medium"]);
    let low_idx = find_column_index(&headers, &["tot_cnt_low", "low"]);
    let very_low_idx = find_column_index(&headers, &["tot_cnt_verylow", "verylow"]);

    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(raw_postcode) = cell(&row, postcode_idx) else {
            continue;
        };
        let normalized = normalize_postcode(raw_postcode.trim_end_matches('*'));
        if normalized.is_empty() {
            continue;
        }

        let rofrs_total = total_idx.and_then(|idx| to_i64(cell(&row, idx)));
        let rofrs_high = high_idx.and_then(|idx| to_i64(cell(&row, idx)));
        let rofrs_medium = medium_idx.and_then(|idx| to_i64(cell(&row, idx)));
        let rofrs_low = low_idx.and_then(|idx| to_i64(cell(&row, idx)));
        let rofrs_very_low = very_low_idx.and_then(|idx| to_i64(cell(&row, idx)));

        let entry = by_postcode.entry(normalized).or_default();
        entry.rofrs_total_cnt = rofrs_total.or(entry.rofrs_total_cnt);
        entry.rofrs_high_cnt = rofrs_high.or(entry.rofrs_high_cnt);
        entry.rofrs_medium_cnt = rofrs_medium.or(entry.rofrs_medium_cnt);
        entry.rofrs_low_cnt = rofrs_low.or(entry.rofrs_low_cnt);
        entry.rofrs_very_low_cnt = rofrs_very_low.or(entry.rofrs_very_low_cnt);

        if entry.risk.is_none() {
            entry.risk = derive_risk_from_counts(rofrs_high, rofrs_medium, rofrs_low)
                .or_else(|| derive_risk_from_very_low(rofrs_very_low));
        }

        entry.source = Some(match entry.source.as_deref() {
            Some("ea-postcode-risk") => "ea-postcode-risk+ea-rofrs".to_owned(),
            Some(existing) => existing.to_owned(),
            None => "ea-rofrs".to_owned(),
        });
    }

    Ok(())
}

fn derive_risk_from_counts(
    high: Option<i64>,
    medium: Option<i64>,
    low: Option<i64>,
) -> Option<String> {
    if high.unwrap_or_default() > 0 {
        return Some("high".to_owned());
    }
    if medium.unwrap_or_default() > 0 {
        return Some("medium".to_owned());
    }
    if low.unwrap_or_default() > 0 {
        return Some("low".to_owned());
    }
    None
}

fn derive_risk_from_very_low(very_low: Option<i64>) -> Option<String> {
    (very_low.unwrap_or_default() > 0).then(|| "very low".to_owned())
}

fn normalize_risk_text(value: &str) -> String {
    value.trim().to_ascii_lowercase()
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

#[cfg(test)]
mod tests {
    use super::{derive_risk_from_counts, derive_risk_from_very_low, normalize_risk_text};

    #[test]
    fn derive_risk_prefers_more_severe_band() {
        assert_eq!(
            derive_risk_from_counts(Some(2), Some(3), Some(5)),
            Some("high".to_owned())
        );
        assert_eq!(
            derive_risk_from_counts(Some(0), Some(1), Some(8)),
            Some("medium".to_owned())
        );
        assert_eq!(
            derive_risk_from_counts(Some(0), Some(0), Some(1)),
            Some("low".to_owned())
        );
        assert_eq!(derive_risk_from_counts(Some(0), Some(0), Some(0)), None);
    }

    #[test]
    fn very_low_risk_is_derived_when_no_other_counts() {
        assert_eq!(
            derive_risk_from_very_low(Some(1)),
            Some("very low".to_owned())
        );
        assert_eq!(derive_risk_from_very_low(Some(0)), None);
    }

    #[test]
    fn risk_text_is_normalized() {
        assert_eq!(normalize_risk_text(" Unlikely "), "unlikely");
    }
}
