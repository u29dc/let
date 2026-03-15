#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use let_sdk::config::{RIGHTMOVE_SEARCH_TYPES, SearchFilters, load_config};
use let_sdk::{ErrorCode, list_known_portal_ids};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

const SEARCH_API_PAGE_SIZE: usize = 24;
const SEARCH_MAX_PAGES: usize = 10;
const SEARCH_API_BASE_URL: &str = "https://www.rightmove.co.uk/api/_search";
const SEARCH_HTML_BASE_URL: &str = "https://www.rightmove.co.uk/property-to-rent/find.html";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocationDiscoverStats {
    name: String,
    id: String,
    count: usize,
    pages_fetched: usize,
    requested_limit: usize,
    effective_page_size: usize,
    source_mode: String,
    truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncation_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
}

#[derive(Debug, Clone)]
struct LocationDiscoverOutcome {
    ids: Vec<String>,
    stats: LocationDiscoverStats,
}

#[derive(Debug, Clone)]
struct HttpResponseData {
    status: u16,
    content_type: String,
    body: String,
}

#[derive(Debug, Clone, Copy)]
struct DiscoverRuntimeConfig<'a> {
    requested_limit: usize,
    page_size: usize,
    max_retries: usize,
    delay_ms: u64,
    api_base_url: &'a str,
    html_base_url: &'a str,
}

impl<'a> DiscoverRuntimeConfig<'a> {
    fn new(
        requested_limit: usize,
        max_retries: usize,
        delay_ms: u64,
        api_base_url: &'a str,
        html_base_url: &'a str,
    ) -> Self {
        Self {
            requested_limit,
            page_size: requested_limit.min(SEARCH_API_PAGE_SIZE),
            max_retries,
            delay_ms,
            api_base_url,
            html_base_url,
        }
    }
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

    let (known_ids, database_present) = match list_known_portal_ids(&db_path) {
        Ok(ids) => (ids.into_iter().collect::<HashSet<_>>(), true),
        Err(error) if error.code == ErrorCode::NotFound => (HashSet::new(), false),
        Err(error) => return Err(error.into()),
    };

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
        "databasePresent": database_present,
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
    let effective_page_size = limit_per_location.min(SEARCH_API_PAGE_SIZE);

    let runtime = build_runtime()?;
    let client = build_client(&runtime, 30)?;

    let mut ids = Vec::new();
    let mut ids_by_location: HashMap<String, Vec<String>> = HashMap::new();
    let mut location_stats = Vec::new();
    let mut total_pages_fetched = 0usize;
    let mut any_truncated = false;

    for location in &locations {
        let outcome = discover_location(
            &runtime,
            &client,
            location,
            &filters,
            limit_per_location,
            config.fetch.max_retries.max(1),
            config.fetch.delay_ms,
        );
        total_pages_fetched += outcome.stats.pages_fetched;
        any_truncated |= outcome.stats.truncated;
        ids.extend(outcome.ids.iter().cloned());
        ids_by_location.insert(location.name.clone(), outcome.ids);
        location_stats.push(outcome.stats);
    }

    let deduped = dedupe(ids);
    Ok(CommandOutput::new(json!({
        "ids": deduped,
        "idsByLocation": ids_by_location,
        "requestedLimit": limit_per_location,
        "effectivePageSize": effective_page_size,
        "pagesFetched": total_pages_fetched,
        "truncated": any_truncated,
        "total": deduped.len(),
        "locations": location_stats,
    }))
    .with_count(deduped.len())
    .with_total(deduped.len())
    .with_has_more(any_truncated)
    .with_text(format!(
        "discovered {} listing id(s) across {} location(s)",
        deduped.len(),
        locations.len()
    )))
}

fn discover_location(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    location: &let_sdk::config::Location,
    filters: &SearchFilters,
    requested_limit: usize,
    max_retries: usize,
    delay_ms: u64,
) -> LocationDiscoverOutcome {
    let config = DiscoverRuntimeConfig::new(
        requested_limit,
        max_retries,
        delay_ms,
        SEARCH_API_BASE_URL,
        SEARCH_HTML_BASE_URL,
    );

    discover_location_with_base_urls(runtime, client, location, filters, config)
}

fn discover_location_with_base_urls(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    location: &let_sdk::config::Location,
    filters: &SearchFilters,
    config: DiscoverRuntimeConfig<'_>,
) -> LocationDiscoverOutcome {
    let first_api_url = build_search_api_url_from_base(
        config.api_base_url,
        &location.id,
        filters,
        config.page_size,
        0,
    );
    let first_response =
        match fetch_response_with_retries(runtime, client, &first_api_url, config.max_retries) {
            Ok(response) => response,
            Err(error) => {
                return LocationDiscoverOutcome {
                    ids: Vec::new(),
                    stats: LocationDiscoverStats {
                        name: location.name.clone(),
                        id: location.id.clone(),
                        count: 0,
                        pages_fetched: 0,
                        requested_limit: config.requested_limit,
                        effective_page_size: config.page_size,
                        source_mode: "api".to_owned(),
                        truncated: false,
                        truncation_reason: None,
                        error: Some(error),
                        status: None,
                        content_type: None,
                    },
                };
            }
        };

    if first_response.status == 200 && first_response.content_type.contains("application/json") {
        return discover_location_via_api(
            runtime,
            client,
            location,
            filters,
            config,
            first_response,
        );
    }

    discover_location_via_html(runtime, client, location, filters, config, None)
}

fn discover_location_via_api(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    location: &let_sdk::config::Location,
    filters: &SearchFilters,
    config: DiscoverRuntimeConfig<'_>,
    first_response: HttpResponseData,
) -> LocationDiscoverOutcome {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut pages_fetched = 0usize;
    let mut truncated = false;
    let mut truncation_reason = None;
    let mut error = None;

    let mut page_index = 0usize;
    let mut next_response = Some(first_response);

    while ids.len() < config.requested_limit && page_index < SEARCH_MAX_PAGES {
        let response = match next_response.take() {
            Some(response) => response,
            None => {
                let offset = page_index * config.page_size;
                let url = build_search_api_url_from_base(
                    config.api_base_url,
                    &location.id,
                    filters,
                    config.page_size,
                    offset,
                );
                match fetch_response_with_retries(runtime, client, &url, config.max_retries) {
                    Ok(response) => response,
                    Err(fetch_error) => {
                        truncated = true;
                        truncation_reason = Some("api-request-failed".to_owned());
                        error = Some(fetch_error);
                        break;
                    }
                }
            }
        };

        pages_fetched += 1;

        let payload = match serde_json::from_str::<SearchApiResponse>(&response.body) {
            Ok(payload) => payload,
            Err(parse_error) => {
                truncated = true;
                truncation_reason = Some("api-parse-failed".to_owned());
                error = Some(parse_error.to_string());
                break;
            }
        };

        let page_ids = payload
            .properties
            .unwrap_or_default()
            .into_iter()
            .map(|property| property.id.to_string())
            .collect::<Vec<_>>();

        for id in page_ids
            .iter()
            .take(config.requested_limit.saturating_sub(ids.len()))
        {
            if seen.insert(id.clone()) {
                ids.push(id.clone());
            }
        }

        if ids.len() >= config.requested_limit || page_ids.len() < config.page_size {
            break;
        }

        page_index += 1;
        if page_index >= SEARCH_MAX_PAGES {
            truncated = true;
            truncation_reason = Some("page-cap-reached".to_owned());
            break;
        }

        if config.delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(config.delay_ms));
        }
    }

    LocationDiscoverOutcome {
        stats: LocationDiscoverStats {
            name: location.name.clone(),
            id: location.id.clone(),
            count: ids.len(),
            pages_fetched,
            requested_limit: config.requested_limit,
            effective_page_size: config.page_size,
            source_mode: "api".to_owned(),
            truncated,
            truncation_reason,
            error,
            status: Some(200),
            content_type: Some("application/json".to_owned()),
        },
        ids,
    }
}

fn discover_location_via_html(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    location: &let_sdk::config::Location,
    filters: &SearchFilters,
    config: DiscoverRuntimeConfig<'_>,
    first_response: Option<HttpResponseData>,
) -> LocationDiscoverOutcome {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut pages_fetched = 0usize;
    let mut truncated = false;
    let mut truncation_reason = None;
    let mut error = None;
    let mut initial_status = first_response.as_ref().map(|response| response.status);
    let mut initial_content_type = first_response
        .as_ref()
        .map(|response| response.content_type.clone());

    let mut page_index = 0usize;
    let mut next_response = first_response;

    while ids.len() < config.requested_limit && page_index < SEARCH_MAX_PAGES {
        let response = match next_response.take() {
            Some(response) => response,
            None => {
                let offset = page_index * config.page_size;
                let url = build_search_html_url_from_base(
                    config.html_base_url,
                    &location.id,
                    filters,
                    offset,
                );
                match fetch_response_with_retries(runtime, client, &url, config.max_retries) {
                    Ok(response) => response,
                    Err(fetch_error) => {
                        truncated = true;
                        truncation_reason = Some("html-request-failed".to_owned());
                        error = Some(fetch_error);
                        break;
                    }
                }
            }
        };

        if pages_fetched == 0 {
            initial_status = Some(response.status);
            initial_content_type = Some(response.content_type.clone());
        }

        if response.status != 200 {
            truncated = true;
            truncation_reason = Some("html-status-not-success".to_owned());
            error = Some(format!("http {}", response.status));
            break;
        }

        pages_fetched += 1;
        let page_ids = extract_listing_ids_from_html(&response.body);

        for id in page_ids
            .iter()
            .take(config.requested_limit.saturating_sub(ids.len()))
        {
            if seen.insert(id.clone()) {
                ids.push(id.clone());
            }
        }

        if ids.len() >= config.requested_limit || page_ids.len() < config.page_size {
            break;
        }

        page_index += 1;
        if page_index >= SEARCH_MAX_PAGES {
            truncated = true;
            truncation_reason = Some("page-cap-reached".to_owned());
            break;
        }

        if config.delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(config.delay_ms));
        }
    }

    LocationDiscoverOutcome {
        stats: LocationDiscoverStats {
            name: location.name.clone(),
            id: location.id.clone(),
            count: ids.len(),
            pages_fetched,
            requested_limit: config.requested_limit,
            effective_page_size: config.page_size,
            source_mode: "html-fallback".to_owned(),
            truncated,
            truncation_reason,
            error,
            status: initial_status,
            content_type: initial_content_type,
        },
        ids,
    }
}
fn fetch_response_with_retries(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    url: &str,
    max_retries: usize,
) -> Result<HttpResponseData, String> {
    let attempts = max_retries.max(1);
    let mut last_error = None;

    for attempt in 1..=attempts {
        match runtime.block_on(async { client.get(url).send().await }) {
            Ok(response) => {
                let status = response.status();
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_owned();

                let body = runtime
                    .block_on(async { response.text().await })
                    .map_err(|error| format!("failed to read response body: {error}"))?;

                let retryable = status.as_u16() == 429 || status.is_server_error();
                if retryable && attempt < attempts {
                    std::thread::sleep(Duration::from_millis((attempt as u64) * 1000));
                    continue;
                }

                return Ok(HttpResponseData {
                    status: status.as_u16(),
                    content_type,
                    body,
                });
            }
            Err(error) => {
                last_error = Some(error.to_string());
                if attempt < attempts {
                    std::thread::sleep(Duration::from_millis((attempt as u64) * 1000));
                    continue;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "search request failed".to_owned()))
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

fn build_search_api_url_from_base(
    base_url: &str,
    location_id: &str,
    filters: &SearchFilters,
    page_size: usize,
    index: usize,
) -> String {
    let mut query = vec![
        ("locationIdentifier", location_id.to_owned()),
        ("numberOfPropertiesPerPage", page_size.to_string()),
        ("radius", filters.radius.to_string()),
        ("sortType", "6".to_owned()),
        ("index", index.to_string()),
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

    let mut url = reqwest::Url::parse(base_url).expect("search api base URL should be valid");
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

fn build_search_html_url_from_base(
    base_url: &str,
    location_id: &str,
    filters: &SearchFilters,
    index: usize,
) -> String {
    let mut url = reqwest::Url::parse(base_url).expect("search html base URL should be valid");
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("locationIdentifier", location_id);
        pairs.append_pair("sortType", "6");
        pairs.append_pair("index", &index.to_string());
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
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SEARCH_API_PAGE_ONE: &str =
        include_str!("../../tests/fixtures/rightmove/search-api-page-1.json");
    const SEARCH_API_PAGE_TWO: &str =
        include_str!("../../tests/fixtures/rightmove/search-api-page-2.json");
    const SEARCH_API_NON_JSON_ERROR: &str =
        include_str!("../../tests/fixtures/rightmove/search-api-non-json-error.html");
    const SEARCH_HTML_PAGE_ONE: &str =
        include_str!("../../tests/fixtures/rightmove/search-html-page-1.html");
    const SEARCH_HTML_PAGE_TWO: &str =
        include_str!("../../tests/fixtures/rightmove/search-html-page-2.html");

    use super::{
        DiscoverRuntimeConfig, SEARCH_API_BASE_URL, build_client, build_runtime,
        build_search_api_url_from_base, discover_location_with_base_urls, parse_csv,
        tokenize_location,
    };

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

        let url =
            build_search_api_url_from_base(SEARCH_API_BASE_URL, "REGION^904", &filters, 24, 0);
        assert!(url.contains("locationIdentifier=REGION%5E904"));
        assert!(url.contains("numberOfPropertiesPerPage=24"));
        assert!(url.contains("index=0"));
        assert!(url.contains("propertyTypes=flat"));
        assert!(url.contains("mustHave=garden"));
    }

    #[test]
    fn api_discovery_paginates_across_multiple_pages() {
        let (mock_runtime, server) = start_mock_server();
        mount_json_fixture(&mock_runtime, &server, 0, SEARCH_API_PAGE_ONE);
        mount_json_fixture(&mock_runtime, &server, 24, SEARCH_API_PAGE_TWO);

        let runtime = build_runtime().expect("runtime");
        let client = build_client(&runtime, 5).expect("client");
        let outcome = discover_location_with_base_urls(
            &runtime,
            &client,
            &sample_location(),
            &sample_filters(),
            DiscoverRuntimeConfig::new(
                30,
                1,
                0,
                &format!("{}/api/_search", server.uri()),
                &format!("{}/property-to-rent/find.html", server.uri()),
            ),
        );

        assert_eq!(outcome.ids.len(), 30, "{outcome:?}");
        assert_eq!(outcome.stats.pages_fetched, 2);
        assert_eq!(outcome.stats.source_mode, "api");
        assert!(!outcome.stats.truncated);
    }

    #[test]
    fn discovery_falls_back_to_html_and_paginates() {
        let (mock_runtime, server) = start_mock_server();
        mount_html_fixture(
            &mock_runtime,
            &server,
            "/api/_search",
            0,
            SEARCH_API_NON_JSON_ERROR,
        );
        mount_html_fixture(
            &mock_runtime,
            &server,
            "/property-to-rent/find.html",
            0,
            SEARCH_HTML_PAGE_ONE,
        );
        mount_html_fixture(
            &mock_runtime,
            &server,
            "/property-to-rent/find.html",
            24,
            SEARCH_HTML_PAGE_TWO,
        );

        let runtime = build_runtime().expect("runtime");
        let client = build_client(&runtime, 5).expect("client");
        let outcome = discover_location_with_base_urls(
            &runtime,
            &client,
            &sample_location(),
            &sample_filters(),
            DiscoverRuntimeConfig::new(
                27,
                1,
                0,
                &format!("{}/api/_search", server.uri()),
                &format!("{}/property-to-rent/find.html", server.uri()),
            ),
        );

        assert_eq!(outcome.ids.len(), 27, "{outcome:?}");
        assert_eq!(outcome.stats.pages_fetched, 2);
        assert_eq!(outcome.stats.source_mode, "html-fallback");
        assert!(!outcome.stats.truncated);
    }

    fn sample_location() -> let_sdk::config::Location {
        let_sdk::config::Location {
            id: "REGION^904".to_owned(),
            name: "York".to_owned(),
        }
    }

    fn sample_filters() -> let_sdk::config::SearchFilters {
        serde_json::from_value(json!({
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
        .expect("deserialize filters")
    }

    fn start_mock_server() -> (tokio::runtime::Runtime, MockServer) {
        let runtime = tokio::runtime::Runtime::new().expect("mock runtime");
        let server = runtime.block_on(async { MockServer::start().await });
        (runtime, server)
    }

    fn mount_json_fixture(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        index: usize,
        fixture: &str,
    ) {
        let body: serde_json::Value = serde_json::from_str(fixture).expect("fixture JSON");
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path("/api/_search"))
                .and(query_param("index", index.to_string()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(body),
                )
                .mount(server)
                .await;
        });
    }

    fn mount_html_fixture(
        runtime: &tokio::runtime::Runtime,
        server: &MockServer,
        route: &str,
        index: usize,
        fixture: &str,
    ) {
        runtime.block_on(async {
            Mock::given(method("GET"))
                .and(path(route))
                .and(query_param("index", index.to_string()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/html")
                        .set_body_string(fixture.to_owned()),
                )
                .mount(server)
                .await;
        });
    }
}
