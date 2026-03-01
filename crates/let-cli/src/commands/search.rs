#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use let_sdk::config::{RIGHTMOVE_SEARCH_TYPES, SearchFilters, load_config, reset_config_cache};
use let_sdk::load_listings_file;
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, HeaderMap, HeaderValue};
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone)]
pub struct DiscoverParams {
    pub region: Option<String>,
    pub location: Option<String>,
    pub property_types: Option<String>,
    pub must_have: Option<String>,
    pub dont_show: Option<String>,
    pub location_name: Option<String>,
    pub limit: Option<usize>,
}

pub fn diff(shared: &SharedArgs, ids_raw: &str) -> CommandResult {
    let input_ids = parse_csv(ids_raw);
    if input_ids.is_empty() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "no ids provided",
            "provide comma-separated portal ids",
        ));
    }

    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let known_ids = load_listings_file(&db_path)
        .map(|data| known_portal_ids(&data.listings))
        .unwrap_or_default();

    let mut known = Vec::new();
    let mut new_ids = Vec::new();
    for id in &input_ids {
        if known_ids.contains(id) {
            known.push(id.clone());
        } else {
            new_ids.push(id.clone());
        }
    }

    Ok(CommandOutput::new(json!({
        "new": new_ids,
        "known": known,
        "total": input_ids.len(),
    }))
    .with_count(input_ids.len())
    .with_total(input_ids.len())
    .with_has_more(false)
    .with_text(format!(
        "{} ids checked: {} new, {} known",
        input_ids.len(),
        input_ids.len().saturating_sub(known.len()),
        known.len()
    )))
}

pub fn resolve(location: &str) -> CommandResult {
    if location.trim().is_empty() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "location is required",
            "provide a location query, for example `let search resolve York`",
        ));
    }

    let runtime = build_runtime()?;
    let client = build_client(&runtime, 20)?;

    let tokenized = tokenize_location(location);
    let url = format!("https://www.rightmove.co.uk/typeAhead/uknostreet/{tokenized}/");

    let response = runtime
        .block_on(async { client.get(url).send().await })
        .map_err(|error| {
            CommandError::runtime(
                "NETWORK_ERROR",
                format!("location lookup failed: {error}"),
                "check network connectivity",
            )
        })?;

    if !response.status().is_success() {
        return Err(CommandError::runtime(
            "LOOKUP_ERROR",
            format!(
                "location lookup failed: http {}",
                response.status().as_u16()
            ),
            "check location spelling and retry",
        ));
    }

    let body = runtime
        .block_on(async { response.text().await })
        .map_err(|error| {
            CommandError::runtime(
                "PARSE_ERROR",
                format!("failed to read location response: {error}"),
                "retry lookup command",
            )
        })?;

    let payload: TypeaheadResponse = serde_json::from_str(&body).map_err(|error| {
        CommandError::runtime(
            "PARSE_ERROR",
            format!("failed to parse location response: {error}"),
            "retry lookup command",
        )
    })?;

    let locations = payload
        .typeahead_locations
        .into_iter()
        .map(|entry| {
            json!({
                "displayName": entry.display_name,
                "locationIdentifier": entry.location_identifier,
                "normalizedSearchTerm": entry.normalised_search_term,
            })
        })
        .collect::<Vec<_>>();

    Ok(CommandOutput::new(json!({
        "query": location,
        "locations": locations,
    }))
    .with_count(locations.len())
    .with_text(format!("resolved {} location result(s)", locations.len())))
}

pub fn discover(shared: &SharedArgs, params: &DiscoverParams) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let config_path = paths.derived.config_file;

    reset_config_cache();
    let config = load_config(Some(&config_path))?;

    let is_adhoc = params.location.is_some();
    let mut locations = if let Some(location_id) = params.location.as_ref() {
        vec![let_sdk::config::Location {
            id: location_id.clone(),
            name: params
                .location_name
                .clone()
                .unwrap_or_else(|| location_id.clone()),
        }]
    } else {
        config.search.locations.clone()
    };

    if let Some(region_filter) = params.region.as_ref() {
        let region_filter_lower = region_filter.to_lowercase();
        locations.retain(|location| location.name.to_lowercase().contains(&region_filter_lower));
        if locations.is_empty() {
            return Err(CommandError::runtime(
                "NO_MATCH",
                format!("no locations match `{region_filter}`"),
                "check config search.locations entries",
            ));
        }
    }

    let filters = apply_filter_overrides(&config.search.filters, params, is_adhoc)?;
    let limit_per_location = params.limit.unwrap_or(config.fetch.max_listings).max(1);

    let runtime = build_runtime()?;
    let client = build_client(&runtime, 30)?;

    let mut ids = Vec::new();
    let mut ids_by_location: HashMap<String, Vec<String>> = HashMap::new();
    let mut location_stats = Vec::new();

    for location in &locations {
        let url = build_search_api_url(&location.id, &filters, limit_per_location);
        let response = runtime.block_on(async { client.get(url).send().await });

        match response {
            Ok(response) => {
                let status = response.status();
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_owned();

                if status.is_success() && content_type.contains("application/json") {
                    let body = runtime
                        .block_on(async { response.text().await })
                        .unwrap_or_default();
                    match serde_json::from_str::<SearchApiResponse>(&body) {
                        Ok(payload) => {
                            let location_ids = payload
                                .properties
                                .unwrap_or_default()
                                .into_iter()
                                .map(|property| property.id.to_string())
                                .collect::<Vec<_>>();
                            ids.extend(location_ids.iter().cloned());
                            ids_by_location.insert(location.name.clone(), location_ids.clone());
                            location_stats.push(json!({
                                "name": location.name,
                                "id": location.id,
                                "count": location_ids.len(),
                            }));
                        }
                        Err(error) => {
                            ids_by_location.insert(location.name.clone(), Vec::new());
                            location_stats.push(json!({
                                "name": location.name,
                                "id": location.id,
                                "count": 0,
                                "error": error.to_string(),
                            }));
                        }
                    }
                    continue;
                }

                let body = runtime
                    .block_on(async { response.text().await })
                    .unwrap_or_default();
                let mut location_ids = extract_listing_ids_from_html(&body);

                if location_ids.is_empty() {
                    let fallback_url = build_search_html_url(&location.id, &filters);
                    if let Ok(fallback_response) =
                        runtime.block_on(async { client.get(fallback_url).send().await })
                        && fallback_response.status().is_success()
                    {
                        let fallback_html = runtime
                            .block_on(async { fallback_response.text().await })
                            .unwrap_or_default();
                        location_ids = extract_listing_ids_from_html(&fallback_html);
                    }
                }

                ids.extend(location_ids.iter().cloned());
                ids_by_location.insert(location.name.clone(), location_ids.clone());
                location_stats.push(json!({
                    "name": location.name,
                    "id": location.id,
                    "count": location_ids.len(),
                    "fallback": true,
                    "status": status.as_u16(),
                    "contentType": content_type,
                }));
            }
            Err(error) => {
                ids_by_location.insert(location.name.clone(), Vec::new());
                location_stats.push(json!({
                    "name": location.name,
                    "id": location.id,
                    "count": 0,
                    "error": error.to_string(),
                }));
            }
        }
    }

    let deduped = dedupe(ids);
    Ok(CommandOutput::new(json!({
        "ids": deduped,
        "idsByLocation": ids_by_location,
        "total": deduped.len(),
        "locations": location_stats,
    }))
    .with_count(deduped.len())
    .with_total(deduped.len())
    .with_has_more(false)
    .with_text(format!(
        "discovered {} listing id(s) across {} location(s)",
        deduped.len(),
        locations.len()
    )))
}

fn build_runtime() -> Result<tokio::runtime::Runtime, CommandError> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|error| {
            CommandError::runtime(
                "PROCESS_ERROR",
                format!("failed to initialize runtime: {error}"),
                "retry command",
            )
        })
}

fn build_client(
    runtime: &tokio::runtime::Runtime,
    timeout_seconds: u64,
) -> Result<reqwest::Client, CommandError> {
    runtime
        .block_on(async {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_seconds))
                .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
                .default_headers(default_json_headers())
                .build()
        })
        .map_err(|error| {
            CommandError::runtime(
                "NETWORK_ERROR",
                format!("failed to build http client: {error}"),
                "check TLS/certificate configuration",
            )
        })
}

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn dedupe(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for id in ids {
        if seen.insert(id.clone()) {
            unique.push(id);
        }
    }
    unique
}

fn default_json_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-GB,en;q=0.9"));
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    headers
}

fn tokenize_location(name: &str) -> String {
    let cleaned = name
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .map(|ch| ch.to_ascii_uppercase())
        .collect::<Vec<_>>();

    cleaned
        .chunks(2)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("/")
}

fn known_portal_ids(listings: &[let_sdk::schema::listing::Listing]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for listing in listings {
        if let Some(id) = listing.portal_ids.rightmove.as_ref() {
            ids.insert(id.clone());
        }
        if let Some(id) = listing.portal_ids.zoopla.as_ref() {
            ids.insert(id.clone());
        }
        if let Some(id) = listing.portal_ids.onthemarket.as_ref() {
            ids.insert(id.clone());
        }
    }
    ids
}

fn apply_filter_overrides(
    base: &SearchFilters,
    params: &DiscoverParams,
    is_adhoc: bool,
) -> Result<SearchFilters, CommandError> {
    let mut next = base.clone();

    if is_adhoc {
        if params.must_have.is_none() {
            next.must_have = Vec::new();
        }
        if params.dont_show.is_none() {
            next.dont_show = Vec::new();
        }
        if params.property_types.is_none() {
            next.property_types = Vec::new();
        }
    }

    if let Some(property_types_raw) = params.property_types.as_ref() {
        let property_types = parse_csv(property_types_raw);
        let valid: HashSet<&str> = RIGHTMOVE_SEARCH_TYPES.into_iter().collect();
        let invalid = property_types
            .iter()
            .filter(|property_type| !valid.contains(property_type.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !invalid.is_empty() {
            return Err(CommandError::runtime(
                "INVALID_PROPERTY_TYPE",
                format!("invalid property type(s): {}", invalid.join(", ")),
                format!("valid types: {}", RIGHTMOVE_SEARCH_TYPES.join(", ")),
            ));
        }
        next.property_types = property_types;
    }

    if let Some(must_have_raw) = params.must_have.as_ref() {
        next.must_have = if must_have_raw == "none" {
            Vec::new()
        } else {
            parse_csv(must_have_raw)
        };
    }

    if let Some(dont_show_raw) = params.dont_show.as_ref() {
        next.dont_show = if dont_show_raw == "none" {
            Vec::new()
        } else {
            parse_csv(dont_show_raw)
        };
    }

    Ok(next)
}

fn build_search_api_url(
    location_id: &str,
    filters: &SearchFilters,
    max_per_location: usize,
) -> String {
    let mut query = vec![
        ("locationIdentifier", location_id.to_owned()),
        (
            "numberOfPropertiesPerPage",
            max_per_location.min(24).to_string(),
        ),
        ("radius", filters.radius.to_string()),
        ("sortType", "6".to_owned()),
        ("index", "0".to_owned()),
        ("includeSSTC", "false".to_owned()),
        ("viewType", "LIST".to_owned()),
        ("channel", "RENT".to_owned()),
        ("areaSizeUnit", "sqft".to_owned()),
        ("currencyCode", "GBP".to_owned()),
        ("isFetching", "false".to_owned()),
    ];

    query.push(("minBedrooms", filters.min_bedrooms.to_string()));
    query.push(("maxBedrooms", filters.max_bedrooms.to_string()));
    query.push(("minPrice", filters.min_price.to_string()));
    query.push(("maxPrice", filters.max_price.to_string()));
    query.push(("includeLetAgreed", filters.include_let_agreed.to_string()));

    let mut url = reqwest::Url::parse("https://www.rightmove.co.uk/api/_search")
        .expect("search api base URL should be valid");
    {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, &value);
        }
        for property_type in &filters.property_types {
            pairs.append_pair("propertyTypes", property_type);
        }
        if !filters.dont_show.is_empty() {
            pairs.append_pair("dontShow", &filters.dont_show.join(","));
        }
        if !filters.must_have.is_empty() {
            pairs.append_pair("mustHave", &filters.must_have.join(","));
        }
    }

    url.to_string()
}

fn build_search_html_url(location_id: &str, filters: &SearchFilters) -> String {
    let mut url = reqwest::Url::parse("https://www.rightmove.co.uk/property-to-rent/find.html")
        .expect("search html base URL should be valid");
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("locationIdentifier", location_id);
        pairs.append_pair("sortType", "6");
        pairs.append_pair("minBedrooms", &filters.min_bedrooms.to_string());
        pairs.append_pair("maxBedrooms", &filters.max_bedrooms.to_string());
        pairs.append_pair("minPrice", &filters.min_price.to_string());
        pairs.append_pair("maxPrice", &filters.max_price.to_string());
        pairs.append_pair("includeLetAgreed", &filters.include_let_agreed.to_string());
        pairs.append_pair("radius", &filters.radius.to_string());
        if !filters.property_types.is_empty() {
            pairs.append_pair("propertyTypes", &filters.property_types.join(","));
        }
        if !filters.dont_show.is_empty() {
            pairs.append_pair("dontShow", &filters.dont_show.join(","));
        }
        if !filters.must_have.is_empty() {
            pairs.append_pair("mustHave", &filters.must_have.join(","));
        }
    }
    url.to_string()
}

fn extract_listing_ids_from_html(html: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut start = 0usize;
    let marker = "/properties/";

    while let Some(found) = html[start..].find(marker) {
        let idx = start + found + marker.len();
        let mut digits = String::new();
        for ch in html[idx..].chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else {
                break;
            }
        }

        if digits.len() >= 6 && seen.insert(digits.clone()) {
            ids.push(digits);
        }
        start = idx;
    }

    ids
}

#[derive(Debug, serde::Deserialize)]
struct TypeaheadResponse {
    #[serde(rename = "typeAheadLocations")]
    typeahead_locations: Vec<TypeaheadLocation>,
}

#[derive(Debug, serde::Deserialize)]
struct TypeaheadLocation {
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "locationIdentifier")]
    location_identifier: String,
    #[serde(rename = "normalisedSearchTerm")]
    normalised_search_term: String,
}

#[derive(Debug, serde::Deserialize)]
struct SearchApiResponse {
    properties: Option<Vec<SearchApiProperty>>,
}

#[derive(Debug, serde::Deserialize)]
struct SearchApiProperty {
    id: i64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{build_search_api_url, parse_csv, tokenize_location};

    #[test]
    fn parse_csv_filters_empty_items() {
        let ids = parse_csv("1701, 1702, ,1703");
        assert_eq!(ids, vec!["1701", "1702", "1703"]);
    }

    #[test]
    fn tokenize_location_matches_rightmove_pattern() {
        assert_eq!(tokenize_location("York"), "YO/RK");
        assert_eq!(tokenize_location("Newcastle"), "NE/WC/AS/TL/E");
    }

    #[test]
    fn search_url_contains_expected_parameters() {
        let filters: let_sdk::config::SearchFilters = serde_json::from_value(json!({
            "minBedrooms": 2,
            "maxBedrooms": 4,
            "minPrice": 700,
            "maxPrice": 1600,
            "propertyTypes": ["flat"],
            "includeLetAgreed": false,
            "radius": 3,
            "dontShow": ["houseShare"],
            "mustHave": ["garden"]
        }))
        .expect("deserialize filters");

        let url = build_search_api_url("REGION^904", &filters, 50);
        assert!(url.contains("locationIdentifier=REGION%5E904"));
        assert!(url.contains("numberOfPropertiesPerPage=24"));
        assert!(url.contains("propertyTypes=flat"));
        assert!(url.contains("mustHave=garden"));
    }
}
