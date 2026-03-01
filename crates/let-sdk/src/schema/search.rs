#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchLocation {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchDiscoverResult {
    pub discovered: Vec<SearchLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchDiffResult {
    pub known: Vec<String>,
    pub new: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiSearchParams {
    pub location_identifier: String,
    pub index: usize,
    pub number_of_properties_per_page: usize,
    pub radius: f64,
    pub min_price: Option<u64>,
    pub max_price: Option<u64>,
    pub min_bedrooms: Option<u8>,
    pub max_bedrooms: Option<u8>,
    pub property_types: Vec<String>,
    pub include_let_agreed: bool,
}
