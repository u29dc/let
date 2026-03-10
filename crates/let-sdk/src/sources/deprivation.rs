#![forbid(unsafe_code)]

use std::path::Path;
use std::{env, path::PathBuf};

use csv::{ReaderBuilder, StringRecord};

use crate::errors::{ErrorCode, LetError, Result};

use super::common::{download_file, open_source_db, to_f64, to_i64, with_temp_dir};

const IMD_CSV_URL: &str = "https://assets.publishing.service.gov.uk/media/691ded56d140bbbaa59a2a7d/File_7_IoD2025_All_Ranks_Scores_Deciles_Population_Denominators.csv";

pub fn build(db_path: &Path) -> Result<usize> {
    let temp = with_temp_dir()?;
    let csv_path = resolve_input_csv_path(&temp)?;

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

    let columns = DeprivationColumns::resolve(&headers)?;

    let mut connection = open_source_db(db_path)?;
    connection.execute_batch(
        "
        DROP TABLE IF EXISTS imd;
        CREATE TABLE imd (
            lsoa_code TEXT PRIMARY KEY,
            lsoa_name TEXT,
            lad_code TEXT,
            lad_name TEXT,
            rank INTEGER,
            decile INTEGER,
            score REAL,
            income_score REAL,
            income_rank INTEGER,
            income_decile INTEGER,
            employment_score REAL,
            employment_rank INTEGER,
            employment_decile INTEGER,
            education_score REAL,
            education_rank INTEGER,
            education_decile INTEGER,
            health_score REAL,
            health_rank INTEGER,
            health_decile INTEGER,
            crime_score REAL,
            crime_rank INTEGER,
            crime_decile INTEGER,
            barriers_score REAL,
            barriers_rank INTEGER,
            barriers_decile INTEGER,
            living_environment_score REAL,
            living_environment_rank INTEGER,
            living_environment_decile INTEGER,
            idaci_score REAL,
            idaci_rank INTEGER,
            idaci_decile INTEGER,
            idaopi_score REAL,
            idaopi_rank INTEGER,
            idaopi_decile INTEGER
        );
        CREATE INDEX idx_imd_rank ON imd(rank);
        CREATE INDEX idx_imd_decile ON imd(decile);
        CREATE INDEX idx_imd_income_decile ON imd(income_decile);
        ",
    )?;

    let tx = connection.transaction()?;
    let mut statement = tx.prepare(
        "
        INSERT OR REPLACE INTO imd (
            lsoa_code, lsoa_name, lad_code, lad_name, rank, decile, score,
            income_score, income_rank, income_decile,
            employment_score, employment_rank, employment_decile,
            education_score, education_rank, education_decile,
            health_score, health_rank, health_decile,
            crime_score, crime_rank, crime_decile,
            barriers_score, barriers_rank, barriers_decile,
            living_environment_score, living_environment_rank, living_environment_decile,
            idaci_score, idaci_rank, idaci_decile,
            idaopi_score, idaopi_rank, idaopi_decile
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10,
            ?11, ?12, ?13,
            ?14, ?15, ?16,
            ?17, ?18, ?19,
            ?20, ?21, ?22,
            ?23, ?24, ?25,
            ?26, ?27, ?28,
            ?29, ?30, ?31,
            ?32, ?33, ?34
        )
        ",
    )?;

    let mut inserted = 0usize;
    for row in reader.records() {
        let row = row.map_err(csv_err)?;
        let Some(lsoa_code) = cell(&row, columns.lsoa_code) else {
            continue;
        };

        statement.execute(rusqlite::params![
            lsoa_code,
            opt_cell(&row, columns.lsoa_name),
            opt_cell(&row, columns.lad_code),
            opt_cell(&row, columns.lad_name),
            opt_i64(&row, columns.imd_rank),
            opt_i64(&row, columns.imd_decile),
            opt_f64(&row, columns.imd_score),
            opt_f64(&row, columns.income_score),
            opt_i64(&row, columns.income_rank),
            opt_i64(&row, columns.income_decile),
            opt_f64(&row, columns.employment_score),
            opt_i64(&row, columns.employment_rank),
            opt_i64(&row, columns.employment_decile),
            opt_f64(&row, columns.education_score),
            opt_i64(&row, columns.education_rank),
            opt_i64(&row, columns.education_decile),
            opt_f64(&row, columns.health_score),
            opt_i64(&row, columns.health_rank),
            opt_i64(&row, columns.health_decile),
            opt_f64(&row, columns.crime_score),
            opt_i64(&row, columns.crime_rank),
            opt_i64(&row, columns.crime_decile),
            opt_f64(&row, columns.barriers_score),
            opt_i64(&row, columns.barriers_rank),
            opt_i64(&row, columns.barriers_decile),
            opt_f64(&row, columns.living_environment_score),
            opt_i64(&row, columns.living_environment_rank),
            opt_i64(&row, columns.living_environment_decile),
            opt_f64(&row, columns.idaci_score),
            opt_i64(&row, columns.idaci_rank),
            opt_i64(&row, columns.idaci_decile),
            opt_f64(&row, columns.idaopi_score),
            opt_i64(&row, columns.idaopi_rank),
            opt_i64(&row, columns.idaopi_decile),
        ])?;
        inserted += 1;
    }

    drop(statement);
    validate_imd_coverage(&tx, inserted)?;
    tx.commit()?;
    connection.execute_batch("VACUUM; ANALYZE;")?;
    Ok(inserted)
}

fn resolve_input_csv_path(temp: &tempfile::TempDir) -> Result<PathBuf> {
    if let Ok(local_path) = env::var("DEPRIVATION_CSV_PATH") {
        return Ok(PathBuf::from(local_path));
    }

    let csv_path = temp.path().join("deprivation.csv");
    let url = env::var("DEPRIVATION_CSV_URL").unwrap_or_else(|_| IMD_CSV_URL.to_owned());
    download_file(&url, &csv_path)?;
    Ok(csv_path)
}

#[derive(Debug, Clone, Copy)]
struct DeprivationColumns {
    lsoa_code: usize,
    lsoa_name: usize,
    lad_code: usize,
    lad_name: usize,
    imd_score: usize,
    imd_rank: usize,
    imd_decile: usize,
    income_score: usize,
    income_rank: usize,
    income_decile: usize,
    employment_score: usize,
    employment_rank: usize,
    employment_decile: usize,
    education_score: usize,
    education_rank: usize,
    education_decile: usize,
    health_score: usize,
    health_rank: usize,
    health_decile: usize,
    crime_score: usize,
    crime_rank: usize,
    crime_decile: usize,
    barriers_score: usize,
    barriers_rank: usize,
    barriers_decile: usize,
    living_environment_score: usize,
    living_environment_rank: usize,
    living_environment_decile: usize,
    idaci_score: usize,
    idaci_rank: usize,
    idaci_decile: usize,
    idaopi_score: usize,
    idaopi_rank: usize,
    idaopi_decile: usize,
}

impl DeprivationColumns {
    fn resolve(headers: &[String]) -> Result<Self> {
        Ok(Self {
            lsoa_code: req_col(headers, &["lsoa code"], "lsoa code")?,
            lsoa_name: req_col(headers, &["lsoa name"], "lsoa name")?,
            lad_code: req_col(headers, &["local authority district code"], "lad code")?,
            lad_name: req_col(headers, &["local authority district name"], "lad name")?,
            imd_score: req_col(headers, &["imd", "score"], "imd score")?,
            imd_rank: req_col(headers, &["imd", "rank"], "imd rank")?,
            imd_decile: req_col(headers, &["imd", "decile"], "imd decile")?,
            income_score: req_col(headers, &["income score"], "income score")?,
            income_rank: req_col(headers, &["income rank"], "income rank")?,
            income_decile: req_col(headers, &["income decile"], "income decile")?,
            employment_score: req_col(headers, &["employment score"], "employment score")?,
            employment_rank: req_col(headers, &["employment rank"], "employment rank")?,
            employment_decile: req_col(headers, &["employment decile"], "employment decile")?,
            education_score: req_col(headers, &["education", "score"], "education score")?,
            education_rank: req_col(headers, &["education", "rank"], "education rank")?,
            education_decile: req_col(headers, &["education", "decile"], "education decile")?,
            health_score: req_col(headers, &["health deprivation", "score"], "health score")?,
            health_rank: req_col(headers, &["health deprivation", "rank"], "health rank")?,
            health_decile: req_col(headers, &["health deprivation", "decile"], "health decile")?,
            crime_score: req_col(headers, &["crime score"], "crime score")?,
            crime_rank: req_col(headers, &["crime rank"], "crime rank")?,
            crime_decile: req_col(headers, &["crime decile"], "crime decile")?,
            barriers_score: req_col(headers, &["barriers to housing", "score"], "barriers score")?,
            barriers_rank: req_col(headers, &["barriers to housing", "rank"], "barriers rank")?,
            barriers_decile: req_col(
                headers,
                &["barriers to housing", "decile"],
                "barriers decile",
            )?,
            living_environment_score: req_col(
                headers,
                &["living environment", "score"],
                "living environment score",
            )?,
            living_environment_rank: req_col(
                headers,
                &["living environment", "rank"],
                "living environment rank",
            )?,
            living_environment_decile: req_col(
                headers,
                &["living environment", "decile"],
                "living environment decile",
            )?,
            idaci_score: req_col(headers, &["idaci", "score"], "idaci score")?,
            idaci_rank: req_col(headers, &["idaci", "rank"], "idaci rank")?,
            idaci_decile: req_col(headers, &["idaci", "decile"], "idaci decile")?,
            idaopi_score: req_col(headers, &["idaopi", "score"], "idaopi score")?,
            idaopi_rank: req_col(headers, &["idaopi", "rank"], "idaopi rank")?,
            idaopi_decile: req_col(headers, &["idaopi", "decile"], "idaopi decile")?,
        })
    }
}

fn req_col(headers: &[String], patterns: &[&str], label: &str) -> Result<usize> {
    let normalized_patterns = patterns
        .iter()
        .map(|item| item.trim().to_lowercase())
        .collect::<Vec<_>>();
    headers
        .iter()
        .position(|header| {
            normalized_patterns
                .iter()
                .all(|pattern| header.contains(pattern))
        })
        .ok_or_else(|| {
            LetError::new(
                ErrorCode::Parse,
                format!("missing `{label}` column in deprivation CSV"),
                "verify deprivation source file format",
            )
        })
}

fn validate_imd_coverage(tx: &rusqlite::Transaction<'_>, inserted: usize) -> Result<()> {
    if inserted == 0 {
        return Err(LetError::new(
            ErrorCode::Parse,
            "deprivation CSV produced zero rows".to_owned(),
            "verify deprivation source file format",
        ));
    }

    let (score_count, rank_count, decile_count): (i64, i64, i64) = tx.query_row(
        "
        SELECT
            COALESCE(SUM(CASE WHEN score IS NOT NULL THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN rank IS NOT NULL THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN decile IS NOT NULL THEN 1 ELSE 0 END), 0)
        FROM imd
        ",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    let total = inserted as f64;
    let score_coverage = score_count as f64 / total;
    let rank_coverage = rank_count as f64 / total;
    let decile_coverage = decile_count as f64 / total;
    if score_coverage >= 0.8 && (rank_coverage < 0.5 || decile_coverage < 0.5) {
        return Err(LetError::new(
            ErrorCode::Parse,
            format!(
                "deprivation coverage check failed: rows={inserted}, score_non_null={score_count}, rank_non_null={rank_count}, decile_non_null={decile_count}"
            ),
            "verify deprivation header mapping for rank/decile columns",
        ));
    }

    Ok(())
}

fn cell(row: &StringRecord, idx: usize) -> Option<&str> {
    row.get(idx)
        .map(str::trim)
        .and_then(|value| (!value.is_empty()).then_some(value))
}

fn opt_cell(row: &StringRecord, idx: usize) -> Option<String> {
    cell(row, idx).map(|value| value.to_owned())
}

fn opt_i64(row: &StringRecord, idx: usize) -> Option<i64> {
    to_i64(cell(row, idx))
}

fn opt_f64(row: &StringRecord, idx: usize) -> Option<f64> {
    to_f64(cell(row, idx))
}

fn csv_err(error: csv::Error) -> LetError {
    LetError::new(
        ErrorCode::Parse,
        format!("failed to parse deprivation CSV row: {error}"),
        "verify deprivation source file format",
    )
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{req_col, validate_imd_coverage};

    #[test]
    fn req_col_requires_all_tokens() {
        let headers = vec![
            "index of multiple deprivation (imd) rank".to_owned(),
            "income rank".to_owned(),
            "income decile".to_owned(),
        ];
        assert_eq!(
            req_col(&headers, &["imd", "rank"], "imd rank").ok(),
            Some(0)
        );
        assert!(req_col(&headers, &["imd", "decile"], "imd decile").is_err());
    }

    #[test]
    fn coverage_check_rejects_sparse_rank_and_decile() {
        let mut connection = Connection::open_in_memory().expect("open sqlite");
        connection
            .execute_batch(
                "
                CREATE TABLE imd (
                    lsoa_code TEXT PRIMARY KEY,
                    rank INTEGER,
                    decile INTEGER,
                    score REAL
                );
                ",
            )
            .expect("create table");
        let tx = connection.transaction().expect("transaction");
        for idx in 0..10 {
            let score = (idx as f64) + 1.0;
            let rank = (idx < 2).then_some((idx + 1) as i64);
            let decile = (idx < 2).then_some(9_i64);
            tx.execute(
                "INSERT INTO imd (lsoa_code, rank, decile, score) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![format!("LSOA{idx:03}"), rank, decile, score],
            )
            .expect("insert row");
        }

        let error = validate_imd_coverage(&tx, 10).expect_err("coverage should fail");
        assert!(error.message.contains("coverage check failed"));
    }

    #[test]
    fn coverage_check_accepts_dense_rank_and_decile() {
        let mut connection = Connection::open_in_memory().expect("open sqlite");
        connection
            .execute_batch(
                "
                CREATE TABLE imd (
                    lsoa_code TEXT PRIMARY KEY,
                    rank INTEGER,
                    decile INTEGER,
                    score REAL
                );
                ",
            )
            .expect("create table");
        let tx = connection.transaction().expect("transaction");
        for idx in 0..10 {
            tx.execute(
                "INSERT INTO imd (lsoa_code, rank, decile, score) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![format!("LSOA{idx:03}"), idx + 1, 8_i64, (idx as f64) + 1.0],
            )
            .expect("insert row");
        }

        validate_imd_coverage(&tx, 10).expect("coverage should pass");
    }
}
