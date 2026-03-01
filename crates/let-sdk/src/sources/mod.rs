#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::errors::{ErrorCode, LetError, Result};

mod broadband;
mod census;
mod common;
mod crime;
mod deprivation;
mod flood;
mod income;
mod naptan;
mod population;
mod postcodes;
mod uprn;

pub const SOURCE_NAMES: [&str; 10] = [
    "broadband",
    "postcodes",
    "deprivation",
    "census",
    "population",
    "income",
    "flood",
    "naptan",
    "uprn",
    "crime",
];

#[derive(Debug, Clone)]
pub struct SourceBuildReport {
    pub source: String,
    pub db_path: PathBuf,
    pub rows: usize,
    pub duration_ms: u64,
}

pub fn list_sources() -> &'static [&'static str] {
    &SOURCE_NAMES
}

pub fn is_valid_source(name: &str) -> bool {
    SOURCE_NAMES.contains(&name)
}

pub fn build_source(sources_dir: &Path, name: &str) -> Result<SourceBuildReport> {
    if !is_valid_source(name) {
        return Err(LetError::new(
            ErrorCode::InvalidInput,
            format!("unknown source: {name}"),
            "run `let build sources list` for supported source names",
        ));
    }

    let started = Instant::now();
    let db_path = sources_dir.join(format!("{name}.db"));
    let rows = match name {
        "broadband" => broadband::build(&db_path)?,
        "postcodes" => postcodes::build(&db_path)?,
        "deprivation" => deprivation::build(&db_path)?,
        "census" => census::build(&db_path)?,
        "population" => population::build(&db_path)?,
        "income" => income::build(&db_path)?,
        "flood" => flood::build(&db_path)?,
        "naptan" => naptan::build(&db_path)?,
        "uprn" => uprn::build(&db_path)?,
        "crime" => crime::build(&db_path)?,
        _ => unreachable!("validated source names must match dispatch"),
    };

    Ok(SourceBuildReport {
        source: name.to_owned(),
        db_path,
        rows,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}
