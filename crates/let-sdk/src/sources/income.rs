#![forbid(unsafe_code)]

use std::env;
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, open_workbook_auto};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{download_file_with_headers, open_source_db, with_temp_dir};

const INCOME_XLSX_URL: &str =
    "https://www.ons.gov.uk/visualisations/dvc3434/fig01/datadownload.xlsx?";

pub fn build(db_path: &Path) -> Result<usize> {
    let (xlsx_path, _temp_guard) = resolve_input_xlsx_path()?;

    let rows = read_rows(&xlsx_path)?;
    let (headers, header_row_index) = find_header_row(&rows)?;
    let columns = resolve_columns(headers)?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS income;
        CREATE TABLE income (
            msoa_code TEXT PRIMARY KEY,
            msoa_name TEXT,
            income_bhc REAL,
            income_ahc REAL
        );
        CREATE INDEX idx_income_bhc ON income(income_bhc);
        CREATE INDEX idx_income_ahc ON income(income_ahc);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "INSERT INTO income (msoa_code, msoa_name, income_bhc, income_ahc) VALUES (?1, ?2, ?3, ?4)",
    )?;

    let mut inserted = 0usize;
    for row in rows.iter().skip(header_row_index + 1) {
        if let Some(parsed) = parse_row(row, &columns) {
            statement.execute(rusqlite::params![parsed.0, parsed.1, parsed.2, parsed.3])?;
            inserted += 1;
        }
    }

    drop(statement);
    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;

    Ok(inserted)
}

fn resolve_input_xlsx_path() -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    if let Ok(local_path) = env::var("INCOME_XLSX_PATH") {
        return Ok((PathBuf::from(local_path), None));
    }

    let temp = with_temp_dir()?;
    let xlsx_path = temp.path().join("income.xlsx");
    let download_url = env::var("INCOME_XLSX_URL").unwrap_or_else(|_| INCOME_XLSX_URL.to_owned());
    download_file_with_headers(
        &download_url,
        &xlsx_path,
        &[
            ("User-Agent", "Mozilla/5.0"),
            (
                "Accept",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
        ],
    )?;

    Ok((xlsx_path, Some(temp)))
}

fn read_rows(path: &Path) -> Result<Vec<Vec<Data>>> {
    let mut workbook = open_workbook_auto(path).map_err(|error| {
        LetError::new(
            ErrorCode::Parse,
            format!("failed to open income workbook {}: {error}", path.display()),
            "verify income source workbook format",
        )
    })?;

    let first_sheet_name = workbook.sheet_names().first().cloned().ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "income workbook has no sheets".to_owned(),
            "verify income source workbook format",
        )
    })?;

    let range = workbook
        .worksheet_range(&first_sheet_name)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!("failed to read income worksheet `{first_sheet_name}`: {error}"),
                "verify income source workbook format",
            )
        })?;

    Ok(range.rows().map(|row| row.to_vec()).collect::<Vec<_>>())
}

fn find_header_row(rows: &[Vec<Data>]) -> Result<(Vec<String>, usize)> {
    for (idx, row) in rows.iter().enumerate() {
        let normalized = normalize_row(row);
        let joined = normalized.join(" ").to_lowercase();
        if (joined.contains("msoa") && joined.contains("income"))
            || (joined.contains("areacd") && joined.contains("income"))
        {
            return Ok((normalized, idx));
        }
    }

    Err(LetError::new(
        ErrorCode::Parse,
        "could not locate header row in income workbook".to_owned(),
        "verify income source workbook format",
    ))
}

#[derive(Debug, Clone)]
struct Columns {
    msoa_code: usize,
    msoa_name: Option<usize>,
    mean_income: usize,
    median_income: usize,
}

type IncomeRow = (String, Option<String>, Option<f64>, Option<f64>);

fn resolve_columns(headers: Vec<String>) -> Result<Columns> {
    let mut msoa_code = find_header(&headers, &["msoa", "code"]);
    if msoa_code.is_none() {
        msoa_code = find_header(&headers, &["areacd"]);
    }

    let msoa_name = find_header(&headers, &["msoa", "name"]);

    let mut mean_income =
        find_header_excluding(&headers, &["mean", "income"], &["lower", "upper", "ci"]);
    if mean_income.is_none() {
        mean_income = find_header(&headers, &["before housing"]);
    }

    let mut median_income =
        find_header_excluding(&headers, &["median", "income"], &["lower", "upper", "ci"]);
    if median_income.is_none() {
        median_income = find_header(&headers, &["after housing"]);
    }

    let Some(msoa_code) = msoa_code else {
        return Err(LetError::new(
            ErrorCode::Parse,
            "missing MSOA code column in income workbook".to_owned(),
            "verify income source workbook format",
        ));
    };

    let Some(mean_income) = mean_income else {
        return Err(LetError::new(
            ErrorCode::Parse,
            "missing mean income column in income workbook".to_owned(),
            "verify income source workbook format",
        ));
    };

    let Some(median_income) = median_income else {
        return Err(LetError::new(
            ErrorCode::Parse,
            "missing median income column in income workbook".to_owned(),
            "verify income source workbook format",
        ));
    };

    Ok(Columns {
        msoa_code,
        msoa_name,
        mean_income,
        median_income,
    })
}

fn parse_row(row: &[Data], columns: &Columns) -> Option<IncomeRow> {
    let msoa_code = string_cell(row.get(columns.msoa_code)?);
    if msoa_code.is_empty() {
        return None;
    }

    let msoa_name = columns
        .msoa_name
        .and_then(|idx| row.get(idx))
        .map(string_cell);
    let income_bhc = row.get(columns.mean_income).and_then(number_cell);
    let income_ahc = row.get(columns.median_income).and_then(number_cell);

    Some((
        msoa_code,
        msoa_name.filter(|value| !value.is_empty()),
        income_bhc,
        income_ahc,
    ))
}

fn normalize_row(row: &[Data]) -> Vec<String> {
    row.iter().map(string_cell).collect::<Vec<_>>()
}

fn string_cell(cell: &Data) -> String {
    match cell {
        Data::String(value) => value.trim().to_owned(),
        Data::Float(value) => {
            if value.fract() == 0.0 {
                format!("{}", *value as i64)
            } else {
                value.to_string()
            }
        }
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::DateTime(value) => value.to_string(),
        Data::DateTimeIso(value) => value.trim().to_owned(),
        Data::DurationIso(value) => value.trim().to_owned(),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

fn number_cell(cell: &Data) -> Option<f64> {
    match cell {
        Data::Float(value) => Some(*value),
        Data::Int(value) => Some(*value as f64),
        Data::String(value) => value.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn find_header(headers: &[String], must_include: &[&str]) -> Option<usize> {
    let lowered = headers
        .iter()
        .map(|header| header.to_lowercase())
        .collect::<Vec<_>>();

    lowered
        .iter()
        .position(|header| must_include.iter().all(|term| header.contains(term)))
}

fn find_header_excluding(
    headers: &[String],
    must_include: &[&str],
    must_exclude: &[&str],
) -> Option<usize> {
    let lowered = headers
        .iter()
        .map(|header| header.to_lowercase())
        .collect::<Vec<_>>();

    lowered.iter().position(|header| {
        must_include.iter().all(|term| header.contains(term))
            && must_exclude.iter().all(|term| !header.contains(term))
    })
}
