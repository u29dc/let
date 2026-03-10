#![forbid(unsafe_code)]

use std::env;
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, open_workbook_auto};
use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{
    download_file, download_file_with_headers, find_column_index, open_source_db, to_f64, to_i64,
    with_temp_dir,
};

const INCOME_XLSX_URL: &str =
    "https://www.ons.gov.uk/visualisations/dvc3434/fig01/datadownload.xlsx?";
const IMD_CSV_URL: &str = "https://assets.publishing.service.gov.uk/media/691ded56d140bbbaa59a2a7d/File_7_IoD2025_All_Ranks_Scores_Deciles_Population_Denominators.csv";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let (xlsx_path, _xlsx_temp_guard) = resolve_input_xlsx_path()?;
    let imd_path = resolve_imd_csv_path(&temp)?;

    let rows = read_rows(&xlsx_path)?;
    let (headers, header_row_index) = find_header_row(&rows)?;
    let columns = resolve_columns(headers)?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS income;
        DROP TABLE IF EXISTS income_proxy_lsoa;

        CREATE TABLE income (
            msoa_code TEXT PRIMARY KEY,
            msoa_name TEXT,
            income_bhc REAL,
            income_ahc REAL
        );
        CREATE INDEX idx_income_bhc ON income(income_bhc);
        CREATE INDEX idx_income_ahc ON income(income_ahc);

        CREATE TABLE income_proxy_lsoa (
            lsoa_code TEXT PRIMARY KEY,
            income_domain_score REAL,
            income_domain_rank INTEGER,
            income_domain_decile INTEGER,
            idaci_score REAL,
            idaci_rank INTEGER,
            idaci_decile INTEGER
        );
        CREATE INDEX idx_income_proxy_decile ON income_proxy_lsoa(income_domain_decile);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "INSERT OR REPLACE INTO income (msoa_code, msoa_name, income_bhc, income_ahc) VALUES (?1, ?2, ?3, ?4)",
    )?;

    let mut inserted = 0usize;
    for row in rows.iter().skip(header_row_index + 1) {
        if let Some(parsed) = parse_row(row, &columns) {
            statement.execute(rusqlite::params![parsed.0, parsed.1, parsed.2, parsed.3])?;
            inserted += 1;
        }
    }

    drop(statement);
    insert_income_proxy_lsoa(&tx, &imd_path)?;
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

fn resolve_imd_csv_path(temp: &tempfile::TempDir) -> Result<PathBuf> {
    if let Ok(local_path) = env::var("INCOME_IMD_CSV_PATH") {
        return Ok(PathBuf::from(local_path));
    }

    let csv_path = temp.path().join("imd-income.csv");
    let url = env::var("INCOME_IMD_CSV_URL").unwrap_or_else(|_| IMD_CSV_URL.to_owned());
    download_file(&url, &csv_path)?;
    Ok(csv_path)
}

fn insert_income_proxy_lsoa(tx: &rusqlite::Transaction<'_>, csv_path: &Path) -> Result<()> {
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(csv_path)
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!(
                    "failed to open IMD income CSV {}: {error}",
                    csv_path.display()
                ),
                "verify income proxy source format",
            )
        })?;

    let headers = reader
        .headers()
        .map_err(|error| {
            LetError::new(
                ErrorCode::Parse,
                format!(
                    "failed to read IMD income CSV headers {}: {error}",
                    csv_path.display()
                ),
                "verify income proxy source format",
            )
        })?
        .iter()
        .map(|value| value.trim().to_lowercase())
        .collect::<Vec<_>>();

    let lsoa_idx = find_column_index(&headers, &["lsoa code"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing lsoa code column in IMD income CSV".to_owned(),
            "verify income proxy source format",
        )
    })?;
    let income_score_idx = find_column_index(&headers, &["income score"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing income score column in IMD income CSV".to_owned(),
            "verify income proxy source format",
        )
    })?;
    let income_rank_idx = find_column_index(&headers, &["income rank"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing income rank column in IMD income CSV".to_owned(),
            "verify income proxy source format",
        )
    })?;
    let income_decile_idx = find_column_index(&headers, &["income decile"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing income decile column in IMD income CSV".to_owned(),
            "verify income proxy source format",
        )
    })?;
    let idaci_score_idx = find_column_index(&headers, &["idaci", "score"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing idaci score column in IMD income CSV".to_owned(),
            "verify income proxy source format",
        )
    })?;
    let idaci_rank_idx = find_column_index(&headers, &["idaci", "rank"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing idaci rank column in IMD income CSV".to_owned(),
            "verify income proxy source format",
        )
    })?;
    let idaci_decile_idx = find_column_index(&headers, &["idaci", "decile"]).ok_or_else(|| {
        LetError::new(
            ErrorCode::Parse,
            "missing idaci decile column in IMD income CSV".to_owned(),
            "verify income proxy source format",
        )
    })?;

    let mut statement = tx.prepare(
        "
        INSERT OR REPLACE INTO income_proxy_lsoa (
            lsoa_code, income_domain_score, income_domain_rank, income_domain_decile,
            idaci_score, idaci_rank, idaci_decile
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
    )?;

    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(lsoa_code) = cell(&row, lsoa_idx) else {
            continue;
        };

        statement.execute(rusqlite::params![
            lsoa_code,
            to_f64(cell(&row, income_score_idx)),
            to_i64(cell(&row, income_rank_idx)),
            to_i64(cell(&row, income_decile_idx)),
            to_f64(cell(&row, idaci_score_idx)),
            to_i64(cell(&row, idaci_rank_idx)),
            to_i64(cell(&row, idaci_decile_idx)),
        ])?;
    }

    Ok(())
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
    before_housing: usize,
    after_housing: usize,
}

type IncomeRow = (String, Option<String>, Option<f64>, Option<f64>);

fn resolve_columns(headers: Vec<String>) -> Result<Columns> {
    let mut msoa_code = find_header(&headers, &["msoa", "code"]);
    if msoa_code.is_none() {
        msoa_code = find_header(&headers, &["areacd"]);
    }

    let msoa_name = find_header(&headers, &["msoa", "name"]);
    let before_housing = find_header(&headers, &["before housing"]).or_else(|| {
        find_header_excluding(
            &headers,
            &["income"],
            &["after housing", "confidence", "ci"],
        )
    });
    let after_housing = find_header(&headers, &["after housing"]).or_else(|| {
        find_header_excluding(
            &headers,
            &["income"],
            &["before housing", "confidence", "ci"],
        )
    });

    let Some(msoa_code) = msoa_code else {
        return Err(LetError::new(
            ErrorCode::Parse,
            "missing MSOA code column in income workbook".to_owned(),
            "verify income source workbook format",
        ));
    };
    let Some(before_housing) = before_housing else {
        return Err(LetError::new(
            ErrorCode::Parse,
            "missing before housing income column in income workbook".to_owned(),
            "verify income source workbook format",
        ));
    };
    let Some(after_housing) = after_housing else {
        return Err(LetError::new(
            ErrorCode::Parse,
            "missing after housing income column in income workbook".to_owned(),
            "verify income source workbook format",
        ));
    };

    Ok(Columns {
        msoa_code,
        msoa_name,
        before_housing,
        after_housing,
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
    let income_bhc = row.get(columns.before_housing).and_then(number_cell);
    let income_ahc = row.get(columns.after_housing).and_then(number_cell);

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

fn cell(row: &StringRecord, idx: usize) -> Option<&str> {
    row.get(idx)
        .map(str::trim)
        .and_then(|value| (!value.is_empty()).then_some(value))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse income proxy CSV row: {error}"),
        "verify income proxy source format",
    )
}

#[cfg(test)]
mod tests {
    use super::{find_header, find_header_excluding};

    #[test]
    fn finds_expected_income_headers() {
        let headers = vec![
            "AREACD".to_owned(),
            "Disposable (net) annual income before housing costs (£ thousands)".to_owned(),
            "Disposable (net) annual income after housing costs (£ thousands)".to_owned(),
        ];

        assert_eq!(find_header(&headers, &["areacd"]), Some(0));
        assert_eq!(find_header(&headers, &["before housing"]), Some(1));
        assert_eq!(find_header(&headers, &["after housing"]), Some(2));
        assert_eq!(
            find_header_excluding(&headers, &["income"], &["after housing"]),
            Some(1)
        );
    }
}
