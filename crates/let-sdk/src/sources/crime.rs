#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashMap};
use std::env;
use std::path::{Path, PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    collect_matching_files, download_file_checked, extract_zip, find_column_index, open_source_db,
    verify_file_checksum_from_env, with_temp_dir,
};

const CRIME_ZIP_URL: &str = "https://data.police.uk/data/archive/latest.zip";

#[derive(Debug, Clone, Copy, Default)]
struct CrimeCounts {
    total: i64,
    violent: i64,
    burglary: i64,
    robbery: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct OutcomeCounts {
    total: i64,
    positive: i64,
    no_further_action: i64,
    under_investigation: i64,
    unknown: i64,
}

pub fn build(db_path: &Path) -> Result<usize> {
    let (zip_path, _temp_guard) = resolve_archive_path()?;

    let temp = with_temp_dir()?;
    let extract_dir = temp.path().join("extract");
    extract_zip(&zip_path, &extract_dir)?;

    let mut street_files = Vec::new();
    collect_matching_files(
        &extract_dir,
        &|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with("-street.csv"))
        },
        &mut street_files,
    )?;
    street_files.sort();

    if street_files.is_empty() {
        return Err(LetError::new(
            ErrorCode::NotFound,
            "no street crime CSV files found in archive".to_owned(),
            "verify crime source archive contents",
        ));
    }

    let mut outcome_files = Vec::new();
    collect_matching_files(
        &extract_dir,
        &|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with("-outcomes.csv"))
        },
        &mut outcome_files,
    )?;
    outcome_files.sort();

    let mut monthly_counts: HashMap<(String, String), CrimeCounts> = HashMap::new();
    let mut monthly_outcomes: HashMap<(String, String), OutcomeCounts> = HashMap::new();
    let mut months = BTreeSet::new();

    for file in street_files {
        process_street_file(&file, &mut monthly_counts, &mut months)?;
    }
    for file in outcome_files {
        process_outcomes_file(&file, &mut monthly_outcomes, &mut months)?;
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

    let mut outcome_totals_12m: HashMap<String, OutcomeCounts> = HashMap::new();
    for ((lsoa, month), counts) in &monthly_outcomes {
        if !last_12_months.contains(month) {
            continue;
        }
        let aggregate = outcome_totals_12m.entry(lsoa.clone()).or_default();
        aggregate.total += counts.total;
        aggregate.positive += counts.positive;
        aggregate.no_further_action += counts.no_further_action;
        aggregate.under_investigation += counts.under_investigation;
        aggregate.unknown += counts.unknown;
    }

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS crime_monthly;
        DROP TABLE IF EXISTS crime_outcomes_monthly;
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

        CREATE TABLE crime_outcomes_monthly (
            lsoa_code TEXT NOT NULL,
            month TEXT NOT NULL,
            total INTEGER,
            positive INTEGER,
            no_further_action INTEGER,
            under_investigation INTEGER,
            unknown INTEGER,
            PRIMARY KEY (lsoa_code, month)
        );
        CREATE INDEX idx_crime_outcomes_lsoa ON crime_outcomes_monthly(lsoa_code);
        CREATE INDEX idx_crime_outcomes_month ON crime_outcomes_monthly(month);

        CREATE TABLE crime_12m (
            lsoa_code TEXT PRIMARY KEY,
            total INTEGER,
            violent INTEGER,
            burglary INTEGER,
            robbery INTEGER,
            outcomes_total INTEGER,
            outcomes_positive INTEGER,
            outcomes_no_further_action INTEGER,
            outcomes_under_investigation INTEGER,
            outcomes_unknown INTEGER,
            outcome_positive_rate REAL,
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
        let mut outcomes_statement = tx.prepare(
            "INSERT OR REPLACE INTO crime_outcomes_monthly (lsoa_code, month, total, positive, no_further_action, under_investigation, unknown) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;

        for ((lsoa, month), counts) in &monthly_outcomes {
            outcomes_statement.execute(rusqlite::params![
                lsoa,
                month,
                counts.total,
                counts.positive,
                counts.no_further_action,
                counts.under_investigation,
                counts.unknown,
            ])?;
        }
    }

    {
        let mut summary_statement = tx.prepare(
            "INSERT OR REPLACE INTO crime_12m (lsoa_code, total, violent, burglary, robbery, outcomes_total, outcomes_positive, outcomes_no_further_action, outcomes_under_investigation, outcomes_unknown, outcome_positive_rate, month_start, month_end) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )?;

        let lsoas = totals_12m
            .keys()
            .chain(outcome_totals_12m.keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        for lsoa in lsoas {
            let crime = totals_12m.get(&lsoa).copied().unwrap_or_default();
            let outcomes = outcome_totals_12m.get(&lsoa).copied().unwrap_or_default();
            let outcome_positive_rate = if outcomes.total > 0 {
                Some(
                    ((outcomes.positive as f64 / outcomes.total as f64) * 100.0 * 100.0).round()
                        / 100.0,
                )
            } else {
                None
            };

            summary_statement.execute(rusqlite::params![
                lsoa,
                crime.total,
                crime.violent,
                crime.burglary,
                crime.robbery,
                outcomes.total,
                outcomes.positive,
                outcomes.no_further_action,
                outcomes.under_investigation,
                outcomes.unknown,
                outcome_positive_rate,
                month_start,
                month_end,
            ])?;
        }
    }

    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(totals_12m.len().max(outcome_totals_12m.len()))
}

fn resolve_archive_path() -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    if let Ok(local_path) = env::var("CRIME_ARCHIVE_PATH") {
        let path = PathBuf::from(local_path);
        verify_file_checksum_from_env(&path, &["CRIME_ARCHIVE_SHA256"], "crime archive")?;
        return Ok((path, None));
    }

    let temp = with_temp_dir()?;
    let zip_path = temp.path().join("crime-latest.zip");
    download_file_checked(
        CRIME_ZIP_URL,
        &zip_path,
        &["CRIME_ARCHIVE_SHA256"],
        "crime archive",
    )?;
    Ok((zip_path, Some(temp)))
}

fn process_street_file(
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

fn process_outcomes_file(
    file_path: &Path,
    monthly_outcomes: &mut HashMap<(String, String), OutcomeCounts>,
    months: &mut BTreeSet<String>,
) -> Result<()> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(file_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!(
                    "failed to open crime outcomes CSV {}: {error}",
                    file_path.display()
                ),
                "verify crime outcomes source file format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!(
                    "failed to read crime outcomes CSV headers {}: {error}",
                    file_path.display()
                ),
                "verify crime outcomes source file format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let month_idx = find_column_index(&headers, &["month"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            format!("missing month column in {}", file_path.display()),
            "verify crime outcomes source file format",
        )
    })?;
    let lsoa_idx = find_column_index(&headers, &["lsoa code"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            format!("missing lsoa code column in {}", file_path.display()),
            "verify crime outcomes source file format",
        )
    })?;
    let outcome_idx = find_column_index(&headers, &["outcome type"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            format!("missing outcome type column in {}", file_path.display()),
            "verify crime outcomes source file format",
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
        let outcome = cell(&row, outcome_idx)
            .unwrap_or_default()
            .to_ascii_lowercase();

        months.insert(month.to_owned());
        let entry = monthly_outcomes
            .entry((lsoa.to_owned(), month.to_owned()))
            .or_default();
        entry.total += 1;

        match classify_outcome(&outcome) {
            OutcomeBand::Positive => entry.positive += 1,
            OutcomeBand::NoFurtherAction => entry.no_further_action += 1,
            OutcomeBand::UnderInvestigation => entry.under_investigation += 1,
            OutcomeBand::Unknown => entry.unknown += 1,
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutcomeBand {
    Positive,
    NoFurtherAction,
    UnderInvestigation,
    Unknown,
}

fn classify_outcome(outcome: &str) -> OutcomeBand {
    let outcome = outcome.to_ascii_lowercase();
    let positive_patterns = [
        "charged",
        "summons",
        "court",
        "caution",
        "community resolution",
        "formal action",
        "action to be taken",
        "offender given",
        "offender sent",
        "penalty notice",
    ];
    if positive_patterns.iter().any(|term| outcome.contains(term)) {
        return OutcomeBand::Positive;
    }

    let nfa_patterns = [
        "no suspect identified",
        "no further action",
        "unable to prosecute",
        "not in the public interest",
        "evidential difficulties prevent further action",
        "investigation complete; no suspect identified",
    ];
    if nfa_patterns.iter().any(|term| outcome.contains(term)) {
        return OutcomeBand::NoFurtherAction;
    }

    let pending_patterns = ["under investigation", "awaiting court outcome"];
    if pending_patterns.iter().any(|term| outcome.contains(term)) {
        return OutcomeBand::UnderInvestigation;
    }

    OutcomeBand::Unknown
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

#[cfg(test)]
mod tests {
    use super::{OutcomeBand, classify_outcome};

    #[test]
    fn classify_positive_outcomes() {
        assert_eq!(
            classify_outcome("Offender sent to prison"),
            OutcomeBand::Positive
        );
        assert_eq!(
            classify_outcome("Formal action is not in the public interest"),
            OutcomeBand::Positive
        );
    }

    #[test]
    fn classify_no_further_action_outcomes() {
        assert_eq!(
            classify_outcome("Investigation complete; no suspect identified"),
            OutcomeBand::NoFurtherAction
        );
    }

    #[test]
    fn classify_under_investigation_outcomes() {
        assert_eq!(
            classify_outcome("Under investigation"),
            OutcomeBand::UnderInvestigation
        );
    }
}
