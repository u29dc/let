#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::errors::{ErrorCode, LetError, Result};

use self::common::{
    SourceInputDescriptor, remove_file_if_exists, replace_file_atomically, temp_db_path_for,
    write_source_metadata,
};

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
    let temp_db_path = temp_db_path_for(name, &db_path)?;

    let rows = match name {
        "broadband" => broadband::build(&temp_db_path),
        "postcodes" => postcodes::build(&temp_db_path),
        "deprivation" => deprivation::build(&temp_db_path),
        "census" => census::build(&temp_db_path),
        "population" => population::build(&temp_db_path),
        "income" => income::build(&temp_db_path),
        "flood" => flood::build(&temp_db_path),
        "naptan" => naptan::build(&temp_db_path),
        "uprn" => uprn::build(&temp_db_path),
        "crime" => crime::build(&temp_db_path),
        _ => unreachable!("validated source names must match dispatch"),
    };

    let rows = match rows {
        Ok(value) => value,
        Err(error) => {
            let _ = remove_file_if_exists(&temp_db_path);
            return Err(error);
        }
    };

    if let Err(error) = write_source_metadata(&temp_db_path, name, rows, &source_inputs(name)) {
        let _ = remove_file_if_exists(&temp_db_path);
        return Err(error);
    }

    if let Err(error) = replace_file_atomically(&temp_db_path, &db_path) {
        let _ = remove_file_if_exists(&temp_db_path);
        return Err(error);
    }

    Ok(SourceBuildReport {
        source: name.to_owned(),
        db_path,
        rows,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn source_inputs(name: &str) -> Vec<SourceInputDescriptor> {
    match name {
        "broadband" => vec![SourceInputDescriptor {
            source_id: "ofcom-fixed-coverage-r01",
            source_url: Some(BROADBAND_OFCOM_ZIP_URL),
            override_envs: &["BROADBAND_ZIP_PATH", "BROADBAND_ZIP_URL"],
            declared_version: Some("202507-r01"),
            notes: Some("Connected Nations fixed broadband coverage release archive"),
        }],
        "postcodes" => vec![SourceInputDescriptor {
            source_id: "onspd-uk",
            source_url: Some(POSTCODES_ONSPD_ZIP_URL),
            override_envs: &["POSTCODES_ZIP_PATH", "POSTCODES_ZIP_URL"],
            declared_version: None,
            notes: Some("ONSPD UK download"),
        }],
        "deprivation" => vec![SourceInputDescriptor {
            source_id: "dclg-imd-2025",
            source_url: Some(DEPRIVATION_IMD_CSV_URL),
            override_envs: &["DEPRIVATION_CSV_PATH", "DEPRIVATION_CSV_URL"],
            declared_version: Some("2025"),
            notes: Some("Indices of Deprivation 2025 file 7"),
        }],
        "census" => vec![SourceInputDescriptor {
            source_id: "nomis-census-ts054",
            source_url: Some(CENSUS_TS054_ZIP_URL),
            override_envs: &["CENSUS_TS054_ZIP_PATH", "CENSUS_TS054_ZIP_URL"],
            declared_version: Some("census-2021"),
            notes: Some("Tenure of household dataset"),
        }],
        "population" => vec![SourceInputDescriptor {
            source_id: "nomis-census-ts001",
            source_url: Some(POPULATION_TS001_ZIP_URL),
            override_envs: &["POPULATION_TS001_ZIP_PATH", "POPULATION_TS001_ZIP_URL"],
            declared_version: Some("census-2021"),
            notes: Some("Population dataset"),
        }],
        "income" => vec![
            SourceInputDescriptor {
                source_id: "ons-saie",
                source_url: Some(INCOME_XLSX_URL),
                override_envs: &["INCOME_XLSX_PATH", "INCOME_XLSX_URL"],
                declared_version: None,
                notes: Some("Small area model-based income estimates"),
            },
            SourceInputDescriptor {
                source_id: "dclg-imd-income-domain",
                source_url: Some(DEPRIVATION_IMD_CSV_URL),
                override_envs: &["INCOME_IMD_CSV_PATH", "INCOME_IMD_CSV_URL"],
                declared_version: Some("2025"),
                notes: Some("Income domain and IDACI proxy metrics"),
            },
        ],
        "flood" => vec![
            SourceInputDescriptor {
                source_id: "ea-flood-risk-postcode",
                source_url: Some(FLOOD_POSTCODE_RISK_CSV_URL),
                override_envs: &["FLOOD_CSV_PATH", "FLOOD_CSV_URL"],
                declared_version: None,
                notes: Some("Flood risk postcode search tool dataset"),
            },
            SourceInputDescriptor {
                source_id: "ea-rofrs-postcodes",
                source_url: Some(FLOOD_ROFRS_ZIP_URL),
                override_envs: &[
                    "FLOOD_ROFRS_CSV_PATH",
                    "FLOOD_ROFRS_ZIP_PATH",
                    "FLOOD_ROFRS_ZIP_URL",
                ],
                declared_version: None,
                notes: Some("RoFRS postcodes in areas at risk"),
            },
        ],
        "naptan" => vec![SourceInputDescriptor {
            source_id: "naptan-access-nodes",
            source_url: Some(NAPTAN_CSV_URL),
            override_envs: &["NAPTAN_CSV_PATH", "NAPTAN_CSV_URL"],
            declared_version: None,
            notes: Some("NaPTAN access node export"),
        }],
        "uprn" => vec![SourceInputDescriptor {
            source_id: "os-open-uprn",
            source_url: Some(UPRN_ZIP_URL),
            override_envs: &["UPRN_ZIP_PATH", "UPRN_ZIP_URL"],
            declared_version: None,
            notes: Some("OS Open UPRN CSV archive"),
        }],
        "crime" => vec![SourceInputDescriptor {
            source_id: "data-police-latest",
            source_url: Some(CRIME_ZIP_URL),
            override_envs: &["CRIME_ARCHIVE_PATH"],
            declared_version: None,
            notes: Some("Police data monthly archive"),
        }],
        _ => Vec::new(),
    }
}

const BROADBAND_OFCOM_ZIP_URL: &str = "https://www.ofcom.org.uk/siteassets/resources/documents/research-and-data/multi-sector/infrastructure-research/connected-nations-2025/202507_fixed_broadband_coverage_r01.zip";
const POSTCODES_ONSPD_ZIP_URL: &str =
    "https://www.arcgis.com/sharing/rest/content/items/3be72478d8454b59bb86ba97b4ee325b/data";
const DEPRIVATION_IMD_CSV_URL: &str = "https://assets.publishing.service.gov.uk/media/691ded56d140bbbaa59a2a7d/File_7_IoD2025_All_Ranks_Scores_Deciles_Population_Denominators.csv";
const CENSUS_TS054_ZIP_URL: &str =
    "https://www.nomisweb.co.uk/output/census/2021/census2021-ts054.zip";
const POPULATION_TS001_ZIP_URL: &str =
    "https://www.nomisweb.co.uk/output/census/2021/census2021-ts001.zip";
const INCOME_XLSX_URL: &str =
    "https://www.ons.gov.uk/visualisations/dvc3434/fig01/datadownload.xlsx?";
const FLOOD_POSTCODE_RISK_CSV_URL: &str = "https://environment.data.gov.uk/api/file/download?fileDataSetId=fb921496-1788-4fc2-b469-7b51e2a45553&fileName=Postcodes_Risk_Assessment_All.csv";
const FLOOD_ROFRS_ZIP_URL: &str = "https://environment.data.gov.uk/api/file/download?fileDataSetId=97781741-2982-4802-af2e-313fe3fd8f7e&fileName=RoFRS_Postcodes_in_Areas_at_Risk.zip";
const NAPTAN_CSV_URL: &str = "https://naptan.api.dft.gov.uk/v1/access-nodes?dataFormat=csv";
const UPRN_ZIP_URL: &str =
    "https://api.os.uk/downloads/v1/products/OpenUPRN/downloads?area=GB&format=CSV&redirect";
const CRIME_ZIP_URL: &str = "https://data.police.uk/data/archive/latest.zip";
