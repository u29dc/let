#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use let_sdk::config::{RIGHTMOVE_SEARCH_TYPES, SearchFilters, load_config};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

const SEARCH_API_PAGE_SIZE: usize = 24;
const SEARCH_MAX_PAGES: usize = 10;
pub(crate) const SEARCH_API_BASE_URL: &str = "https://www.rightmove.co.uk/api/_search";
const SEARCH_HTML_BASE_URL: &str = "https://www.rightmove.co.uk/property-to-rent/find.html";

#[derive(Debug, Clone)]
pub struct DiscoverParams {
    pub region: Option<String>,
    pub location: Option<String>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub min_bedrooms: Option<i64>,
    pub max_bedrooms: Option<i64>,
    pub radius: Option<f64>,
    pub include_let_agreed: Option<bool>,
    pub property_types: Option<String>,
    pub must_have: Option<String>,
    pub dont_show: Option<String>,
    pub location_name: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListingCardSummary {
    pub id: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baths: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub source: String,
    pub location_matches: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_search_area: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketSummary {
    pub count: usize,
    pub priced_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rent: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_rent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rent: Option<i64>,
    pub by_type: BTreeMap<String, MarketTypeSummary>,
    pub duplicate_ids: Vec<String>,
    pub duplicate_location_matches: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketTypeSummary {
    pub count: usize,
    pub priced_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rent: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_rent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rent: Option<i64>,
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
    listings: Vec<ListingCardSummary>,
    stats: LocationDiscoverStats,
}

#[derive(Debug, Clone)]
struct DiscoveryRun {
    ids: Vec<String>,
    ids_by_location: BTreeMap<String, Vec<String>>,
    listings: Vec<ListingCardSummary>,
    requested_limit: usize,
    effective_page_size: usize,
    pages_fetched: usize,
    truncated: bool,
    locations: Vec<LocationDiscoverStats>,
    location_matches_by_id: BTreeMap<String, Vec<String>>,
    duplicate_ids: Vec<String>,
    duplicate_location_matches: BTreeMap<String, Vec<String>>,
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
    use_api: bool,
    api_base_url: &'a str,
    html_base_url: &'a str,
}

impl<'a> DiscoverRuntimeConfig<'a> {
    fn new(
        requested_limit: usize,
        max_retries: usize,
        delay_ms: u64,
        use_api: bool,
        api_base_url: &'a str,
        html_base_url: &'a str,
    ) -> Self {
        Self {
            requested_limit,
            page_size: requested_limit.min(SEARCH_API_PAGE_SIZE),
            max_retries,
            delay_ms,
            use_api,
            api_base_url,
            html_base_url,
        }
    }
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

    let matches = match serde_json::from_str::<TypeaheadResponse>(&body) {
        Ok(payload) => payload
            .typeahead_locations
            .into_iter()
            .map(|entry| {
                json!({
                    "displayName": entry.display_name,
                    "locationIdentifier": entry.location_identifier,
                    "normalizedSearchTerm": entry.normalised_search_term,
                    "source": "typeahead",
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => resolve_location_from_search_page(&runtime, &client, location)?,
    };

    Ok(CommandOutput::new(json!({
        "location": location,
        "matches": matches,
    }))
    .with_count(matches.len()))
}

fn resolve_location_from_search_page(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    location: &str,
) -> Result<Vec<serde_json::Value>, CommandError> {
    let slug = location_slug(location);
    if slug.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!("https://www.rightmove.co.uk/property-to-rent/{slug}.html");
    let response = runtime
        .block_on(async { client.get(url).send().await })
        .map_err(|error| {
            CommandError::runtime(
                "NETWORK_ERROR",
                format!("location lookup fallback failed: {error}"),
                "check network connectivity",
            )
        })?;
    if !response.status().is_success() {
        return Err(CommandError::runtime(
            "LOOKUP_ERROR",
            format!(
                "location lookup fallback failed: http {}",
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
                format!("failed to read location fallback response: {error}"),
                "retry lookup command",
            )
        })?;
    let Some(location_identifier) = extract_search_page_location_identifier(&body) else {
        return Err(CommandError::runtime(
            "PARSE_ERROR",
            "failed to find locationIdentifier in Rightmove search page",
            "retry lookup command or pass --location manually",
        ));
    };
    Ok(vec![json!({
        "displayName": extract_search_page_description(&body).unwrap_or_else(|| location.to_owned()),
        "locationIdentifier": location_identifier,
        "normalizedSearchTerm": location,
        "source": "searchPage",
    })])
}

fn location_slug(location: &str) -> String {
    location
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn extract_search_page_location_identifier(html: &str) -> Option<String> {
    extract_json_string_after(html, "\"locationIdentifier\":")
}

fn extract_search_page_description(html: &str) -> Option<String> {
    extract_json_string_after(html, "\"searchParametersDescription\":")
}

fn extract_json_string_after(input: &str, marker: &str) -> Option<String> {
    let start = input.find(marker)? + marker.len();
    let rest = input.get(start..)?;
    let mut chars = rest.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut value = String::new();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            match ch {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                '/' => value.push('/'),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                'u' => return None,
                other => value.push(other),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return Some(value),
            other => value.push(other),
        }
    }
    None
}

pub fn discover(shared: &SharedArgs, params: &DiscoverParams) -> CommandResult {
    let run = run_discovery(shared, params)?;

    Ok(CommandOutput::new(discovery_payload(&run))
        .with_count(run.ids.len())
        .with_total(run.ids.len())
        .with_has_more(run.truncated))
}

pub fn market(shared: &SharedArgs, params: &DiscoverParams) -> CommandResult {
    let run = run_discovery(shared, params)?;
    let summary = summarize_market(&run.listings);
    let count = summary.count;

    Ok(CommandOutput::new(json!({
        "market": summary,
        "ids": &run.ids,
        "listings": &run.listings,
        "requestedLimit": run.requested_limit,
        "effectivePageSize": run.effective_page_size,
        "pagesFetched": run.pages_fetched,
        "truncated": run.truncated,
        "total": count,
        "locations": &run.locations,
    }))
    .with_count(count)
    .with_total(count)
    .with_has_more(run.truncated))
}

fn run_discovery(
    shared: &SharedArgs,
    params: &DiscoverParams,
) -> Result<DiscoveryRun, CommandError> {
    let paths = shared.resolved_paths();
    let config_path = shared.config_path(&paths)?;

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

    let mut all_ids = Vec::new();
    let mut ids_by_location: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut location_matches_by_id: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut listing_by_id: HashMap<String, ListingCardSummary> = HashMap::new();
    let mut location_stats = Vec::new();
    let mut total_pages_fetched = 0usize;
    let mut any_truncated = false;

    for location in &locations {
        let runtime_config = DiscoverRuntimeConfig::new(
            limit_per_location,
            config.fetch.max_retries.max(1),
            config.fetch.delay_ms,
            config.search.use_api,
            SEARCH_API_BASE_URL,
            SEARCH_HTML_BASE_URL,
        );
        let outcome = discover_location(&runtime, &client, location, &filters, runtime_config);
        total_pages_fetched += outcome.stats.pages_fetched;
        any_truncated |= outcome.stats.truncated;
        for id in &outcome.ids {
            all_ids.push(id.clone());
            push_location_match(
                location_matches_by_id.entry(id.clone()).or_default(),
                &location.name,
            );
        }
        for listing in outcome.listings {
            match listing_by_id.get_mut(&listing.id) {
                Some(existing) => merge_listing_card(existing, &listing),
                None => {
                    listing_by_id.insert(listing.id.clone(), listing);
                }
            }
        }
        ids_by_location.insert(location.name.clone(), outcome.ids);
        location_stats.push(outcome.stats);
    }

    let ids = dedupe(all_ids);
    let duplicate_location_matches = location_matches_by_id
        .iter()
        .filter(|(_, matches)| matches.len() > 1)
        .map(|(id, matches)| (id.clone(), matches.clone()))
        .collect::<BTreeMap<_, _>>();
    let duplicate_ids = duplicate_location_matches
        .keys()
        .cloned()
        .collect::<Vec<_>>();

    let listings = ids
        .iter()
        .map(|id| {
            let mut listing = listing_by_id
                .remove(id)
                .unwrap_or_else(|| ListingCardSummary::from_id(id, "unknown"));
            let matches = location_matches_by_id.get(id).cloned().unwrap_or_default();
            listing.primary_search_area = matches.first().cloned();
            listing.location_matches = matches;
            listing
        })
        .collect::<Vec<_>>();

    Ok(DiscoveryRun {
        ids,
        ids_by_location,
        listings,
        requested_limit: limit_per_location,
        effective_page_size,
        pages_fetched: total_pages_fetched,
        truncated: any_truncated,
        locations: location_stats,
        location_matches_by_id,
        duplicate_ids,
        duplicate_location_matches,
    })
}

fn discovery_payload(run: &DiscoveryRun) -> serde_json::Value {
    json!({
        "ids": &run.ids,
        "idsByLocation": &run.ids_by_location,
        "listings": &run.listings,
        "requestedLimit": run.requested_limit,
        "effectivePageSize": run.effective_page_size,
        "pagesFetched": run.pages_fetched,
        "truncated": run.truncated,
        "total": run.ids.len(),
        "locations": &run.locations,
        "locationMatchesById": &run.location_matches_by_id,
        "duplicateIds": &run.duplicate_ids,
        "duplicateLocationMatches": &run.duplicate_location_matches,
    })
}

fn discover_location(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    location: &let_sdk::config::Location,
    filters: &SearchFilters,
    config: DiscoverRuntimeConfig<'_>,
) -> LocationDiscoverOutcome {
    discover_location_with_base_urls(runtime, client, location, filters, config)
}

fn discover_location_with_base_urls(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    location: &let_sdk::config::Location,
    filters: &SearchFilters,
    config: DiscoverRuntimeConfig<'_>,
) -> LocationDiscoverOutcome {
    if !config.use_api {
        return discover_location_via_html(runtime, client, location, filters, config, None);
    }

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
                    listings: Vec::new(),
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
    let mut listings = Vec::new();
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

        let page_listings = payload
            .properties
            .unwrap_or_default()
            .into_iter()
            .filter_map(|property| listing_card_from_search_property(&property, "api"))
            .collect::<Vec<_>>();

        for listing in page_listings
            .iter()
            .take(config.requested_limit.saturating_sub(ids.len()))
        {
            if seen.insert(listing.id.clone()) {
                ids.push(listing.id.clone());
                listings.push(listing.clone());
            }
        }

        if ids.len() >= config.requested_limit || page_listings.len() < config.page_size {
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
        listings,
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
    let mut listings = Vec::new();
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
        let page_listings = extract_listing_cards_from_html(&response.body);

        for listing in page_listings
            .iter()
            .take(config.requested_limit.saturating_sub(ids.len()))
        {
            if seen.insert(listing.id.clone()) {
                ids.push(listing.id.clone());
                listings.push(listing.clone());
            }
        }

        if ids.len() >= config.requested_limit || page_listings.len() < config.page_size {
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
            source_mode: if config.use_api {
                "html-fallback".to_owned()
            } else {
                "html".to_owned()
            },
            truncated,
            truncation_reason,
            error,
            status: initial_status,
            content_type: initial_content_type,
        },
        ids,
        listings,
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

impl ListingCardSummary {
    fn from_id(id: &str, source: &str) -> Self {
        Self {
            id: id.to_owned(),
            url: rightmove_listing_url(id),
            price: None,
            display_price: None,
            beds: None,
            baths: None,
            property_type: None,
            address: None,
            summary: None,
            source: source.to_owned(),
            location_matches: Vec::new(),
            primary_search_area: None,
        }
    }
}

fn push_location_match(matches: &mut Vec<String>, location: &str) {
    if !matches.iter().any(|value| value == location) {
        matches.push(location.to_owned());
    }
}

fn merge_listing_card(existing: &mut ListingCardSummary, next: &ListingCardSummary) {
    if existing.url.is_empty() {
        existing.url.clone_from(&next.url);
    }
    if existing.price.is_none() {
        existing.price = next.price;
    }
    if existing.display_price.is_none() {
        existing.display_price.clone_from(&next.display_price);
    }
    if existing.beds.is_none() {
        existing.beds = next.beds;
    }
    if existing.baths.is_none() {
        existing.baths = next.baths;
    }
    if existing.property_type.is_none() {
        existing.property_type.clone_from(&next.property_type);
    }
    if existing.address.is_none() {
        existing.address.clone_from(&next.address);
    }
    if existing.summary.is_none() {
        existing.summary.clone_from(&next.summary);
    }
    if existing.source == "unknown" {
        existing.source.clone_from(&next.source);
    }
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

    if let Some(min_price) = params.min_price {
        if min_price < 0 {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                "min price cannot be negative",
                "pass a zero or positive --min-price value",
            ));
        }
        next.min_price = min_price;
    }

    if let Some(max_price) = params.max_price {
        if max_price < 0 {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                "max price cannot be negative",
                "pass a zero or positive --max-price value",
            ));
        }
        next.max_price = max_price;
    }

    if let Some(min_bedrooms) = params.min_bedrooms {
        if min_bedrooms < 0 {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                "min bedrooms cannot be negative",
                "pass a zero or positive --min-bedrooms value",
            ));
        }
        next.min_bedrooms = min_bedrooms;
    }

    if let Some(max_bedrooms) = params.max_bedrooms {
        if max_bedrooms < 0 {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                "max bedrooms cannot be negative",
                "pass a zero or positive --max-bedrooms value",
            ));
        }
        next.max_bedrooms = max_bedrooms;
    }

    if let Some(radius) = params.radius {
        if !radius.is_finite() || radius < 0.0 {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                "radius must be a finite non-negative number",
                "pass a finite zero or positive --radius value",
            ));
        }
        next.radius = radius;
    }

    if let Some(include_let_agreed) = params.include_let_agreed {
        next.include_let_agreed = include_let_agreed;
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

    if next.min_price > next.max_price {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "min price cannot exceed max price",
            "adjust --min-price or --max-price",
        ));
    }

    if next.min_bedrooms > next.max_bedrooms {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "min bedrooms cannot exceed max bedrooms",
            "adjust --min-bedrooms or --max-bedrooms",
        ));
    }

    Ok(next)
}

pub(crate) fn build_search_api_url_from_base(
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

fn extract_listing_cards_from_html(html: &str) -> Vec<ListingCardSummary> {
    let mut cards = extract_listing_cards_from_embedded_json(html);
    if cards.is_empty() {
        cards = extract_listing_ids_from_html(html)
            .into_iter()
            .map(|id| ListingCardSummary::from_id(&id, "html"))
            .collect();
    }
    dedupe_listing_cards(cards)
}

fn extract_listing_cards_from_embedded_json(html: &str) -> Vec<ListingCardSummary> {
    let mut cards = Vec::new();
    for body in extract_json_script_bodies(html) {
        if let Ok(value) = serde_json::from_str::<Value>(&body) {
            collect_search_property_cards(&value, "html", &mut cards);
        }
    }
    dedupe_listing_cards(cards)
}

fn extract_json_script_bodies(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut bodies = Vec::new();
    let mut start = 0usize;

    while let Some(script_start_relative) = lower[start..].find("<script") {
        let script_start = start + script_start_relative;
        let Some(tag_end_relative) = lower[script_start..].find('>') else {
            break;
        };
        let tag_end = script_start + tag_end_relative;
        let tag = &lower[script_start..=tag_end];
        let content_start = tag_end + 1;
        let Some(script_end_relative) = lower[content_start..].find("</script>") else {
            break;
        };
        let script_end = content_start + script_end_relative;

        if tag.contains("application/json") || tag.contains("__next_data__") {
            bodies.push(html[content_start..script_end].trim().to_owned());
        }

        start = script_end + "</script>".len();
    }

    bodies
}

fn collect_search_property_cards(value: &Value, source: &str, cards: &mut Vec<ListingCardSummary>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(properties)) = map.get("properties") {
                for property in properties {
                    if let Some(card) = listing_card_from_search_property(property, source) {
                        cards.push(card);
                    }
                }
            }
            for nested in map.values() {
                collect_search_property_cards(nested, source, cards);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_search_property_cards(item, source, cards);
            }
        }
        _ => {}
    }
}

fn listing_card_from_search_property(value: &Value, source: &str) -> Option<ListingCardSummary> {
    let id = first_string_path(value, &[&["id"], &["propertyId"]])?;
    let display_price = first_display_price(value);
    let price = first_i64_path(
        value,
        &[
            &["price", "amount"],
            &["price", "amountPerMonth"],
            &["prices", "pricePerMonth"],
            &["monthlyRent"],
        ],
    )
    .or_else(|| display_price.as_deref().and_then(parse_rent_pcm));

    Some(ListingCardSummary {
        url: first_string_path(
            value,
            &[
                &["propertyUrl"],
                &["property_url"],
                &["url"],
                &["detailUrl"],
                &["propertyUrlPath"],
            ],
        )
        .map(|url| normalize_rightmove_url(&id, &url))
        .unwrap_or_else(|| rightmove_listing_url(&id)),
        id,
        price,
        display_price,
        beds: first_i64_path(value, &[&["bedrooms"], &["beds"]]),
        baths: first_i64_path(value, &[&["bathrooms"], &["baths"]]),
        property_type: first_clean_string_path(
            value,
            &[
                &["propertySubType"],
                &["propertyTypeFullDescription"],
                &["propertyType"],
                &["displayPropertyType"],
            ],
        ),
        address: first_clean_string_path(
            value,
            &[&["displayAddress"], &["address", "displayAddress"]],
        ),
        summary: first_clean_string_path(
            value,
            &[
                &["summary"],
                &["description"],
                &["text", "description"],
                &["heading"],
            ],
        ),
        source: source.to_owned(),
        location_matches: Vec::new(),
        primary_search_area: None,
    })
}

fn dedupe_listing_cards(cards: Vec<ListingCardSummary>) -> Vec<ListingCardSummary> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for card in cards {
        if seen.insert(card.id.clone()) {
            unique.push(card);
        }
    }
    unique
}

fn first_display_price(value: &Value) -> Option<String> {
    if let Some(display_prices) =
        value_at_path(value, &["price", "displayPrices"]).and_then(Value::as_array)
    {
        for display in display_prices {
            if let Some(text) = first_clean_string_path(display, &[&["displayPrice"]]) {
                return Some(text);
            }
        }
    }

    first_clean_string_path(
        value,
        &[
            &["price", "displayPrice"],
            &["price", "primaryPrice"],
            &["prices", "primaryPrice"],
            &["displayPrice"],
        ],
    )
}

fn first_string_path(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| value_at_path(value, path).and_then(value_to_string))
}

fn first_clean_string_path(value: &Value, paths: &[&[&str]]) -> Option<String> {
    first_string_path(value, paths).and_then(|text| {
        let cleaned = clean_text(&text);
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        }
    })
}

fn first_i64_path(value: &Value, paths: &[&[&str]]) -> Option<i64> {
    paths
        .iter()
        .find_map(|path| value_at_path(value, path).and_then(value_to_i64))
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn parse_rent_pcm(raw: &str) -> Option<i64> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("pw") && !lower.contains("pcm") {
        return None;
    }

    let mut digits = String::new();
    let mut started = false;
    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            started = true;
        } else if started && (ch == ',' || ch.is_whitespace()) {
            continue;
        } else if started {
            break;
        }
    }

    digits.parse::<i64>().ok()
}

fn clean_text(raw: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }

    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&pound;", "GBP ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_rightmove_url(id: &str, raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return raw.to_owned();
    }
    if raw.starts_with('/') {
        return format!("https://www.rightmove.co.uk{raw}");
    }
    rightmove_listing_url(id)
}

fn rightmove_listing_url(id: &str) -> String {
    format!("https://www.rightmove.co.uk/properties/{id}")
}

pub fn summarize_market(listings: &[ListingCardSummary]) -> MarketSummary {
    let rents = listings
        .iter()
        .filter_map(|listing| listing.price)
        .collect::<Vec<_>>();
    let overall_rent_stats = rent_stats(&rents);

    let mut grouped: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let mut grouped_counts: BTreeMap<String, usize> = BTreeMap::new();
    for listing in listings {
        let key = listing
            .property_type
            .clone()
            .unwrap_or_else(|| "unknown".to_owned());
        *grouped_counts.entry(key.clone()).or_default() += 1;
        if let Some(price) = listing.price {
            grouped.entry(key).or_default().push(price);
        } else {
            grouped.entry(key).or_default();
        }
    }

    let by_type = grouped_counts
        .into_iter()
        .map(|(property_type, count)| {
            let prices = grouped.remove(&property_type).unwrap_or_default();
            let stats = rent_stats(&prices);
            (
                property_type,
                MarketTypeSummary {
                    count,
                    priced_count: prices.len(),
                    min_rent: stats.0,
                    median_rent: stats.1,
                    max_rent: stats.2,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let duplicate_location_matches = listings
        .iter()
        .filter(|listing| listing.location_matches.len() > 1)
        .map(|listing| (listing.id.clone(), listing.location_matches.clone()))
        .collect::<BTreeMap<_, _>>();
    let duplicate_ids = duplicate_location_matches
        .keys()
        .cloned()
        .collect::<Vec<_>>();

    MarketSummary {
        count: listings.len(),
        priced_count: rents.len(),
        min_rent: overall_rent_stats.0,
        median_rent: overall_rent_stats.1,
        max_rent: overall_rent_stats.2,
        by_type,
        duplicate_ids,
        duplicate_location_matches,
    }
}

fn rent_stats(prices: &[i64]) -> (Option<i64>, Option<f64>, Option<i64>) {
    if prices.is_empty() {
        return (None, None, None);
    }

    let mut sorted = prices.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] as f64 + sorted[middle] as f64) / 2.0
    } else {
        sorted[middle] as f64
    };

    (
        sorted.first().copied(),
        Some(median),
        sorted.last().copied(),
    )
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
    properties: Option<Vec<Value>>,
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
        DiscoverParams, DiscoverRuntimeConfig, ListingCardSummary, SEARCH_API_BASE_URL,
        apply_filter_overrides, build_client, build_runtime, build_search_api_url_from_base,
        discover_location_with_base_urls, extract_listing_cards_from_html,
        extract_search_page_description, extract_search_page_location_identifier,
        listing_card_from_search_property, location_slug, parse_csv, summarize_market,
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
    fn location_slug_matches_rightmove_town_pages() {
        assert_eq!(location_slug("Sevenoaks"), "Sevenoaks");
        assert_eq!(location_slug("Market Harborough"), "Market-Harborough");
        assert_eq!(location_slug("Sevenoaks, Kent"), "Sevenoaks-Kent");
    }

    #[test]
    fn search_page_location_parser_extracts_identifier_and_description() {
        let html = r#"{
            "searchParametersDescription":"Properties To Rent in Sevenoaks, Kent",
            "searchParameters":{"locationIdentifier":"REGION^1191"}
        }"#;

        assert_eq!(
            extract_search_page_location_identifier(html).as_deref(),
            Some("REGION^1191")
        );
        assert_eq!(
            extract_search_page_description(html).as_deref(),
            Some("Properties To Rent in Sevenoaks, Kent")
        );
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
        assert!(url.contains("minBedrooms=2"));
        assert!(url.contains("maxBedrooms=4"));
        assert!(url.contains("minPrice=700"));
        assert!(url.contains("maxPrice=1600"));
        assert!(url.contains("radius=3"));
        assert!(url.contains("includeLetAgreed=false"));
    }

    #[test]
    fn filter_overrides_preserve_base_defaults_without_params() {
        let base = sample_filters();
        let merged =
            apply_filter_overrides(&base, &empty_discover_params(), false).expect("merge filters");

        assert_eq!(merged, base);
    }

    #[test]
    fn filter_overrides_apply_core_search_fields() {
        let base = sample_filters();
        let params = DiscoverParams {
            min_price: Some(900),
            max_price: Some(2_200),
            min_bedrooms: Some(1),
            max_bedrooms: Some(3),
            radius: Some(0.5),
            include_let_agreed: Some(true),
            property_types: Some("terraced,flat".to_owned()),
            must_have: Some("parking".to_owned()),
            dont_show: Some("retirement".to_owned()),
            ..empty_discover_params()
        };

        let merged = apply_filter_overrides(&base, &params, false).expect("merge filters");

        assert_eq!(merged.min_price, 900);
        assert_eq!(merged.max_price, 2_200);
        assert_eq!(merged.min_bedrooms, 1);
        assert_eq!(merged.max_bedrooms, 3);
        assert_eq!(merged.radius, 0.5);
        assert!(merged.include_let_agreed);
        assert_eq!(merged.property_types, vec!["terraced", "flat"]);
        assert_eq!(merged.must_have, vec!["parking"]);
        assert_eq!(merged.dont_show, vec!["retirement"]);
    }

    #[test]
    fn filter_overrides_reject_non_finite_radius() {
        let base = sample_filters();
        let params = DiscoverParams {
            radius: Some(f64::NAN),
            ..empty_discover_params()
        };

        let error = apply_filter_overrides(&base, &params, false).expect_err("reject NaN radius");

        assert_eq!(error.code, "VALIDATION_ERROR");
        assert!(error.message.contains("finite"));
    }

    #[test]
    fn api_listing_card_parser_extracts_summary_fields() {
        let property = json!({
            "id": 170448131,
            "propertyUrl": "/properties/170448131#/?channel=RES_LET",
            "price": {
                "amount": 1250,
                "displayPrices": [{ "displayPrice": "\u{00a3}1,250 pcm" }]
            },
            "bedrooms": 2,
            "bathrooms": 1,
            "propertySubType": "Flat",
            "displayAddress": "Station Road, York",
            "summary": "Two bedroom flat near the station."
        });

        let card = listing_card_from_search_property(&property, "api").expect("card");

        assert_eq!(card.id, "170448131");
        assert_eq!(
            card.url,
            "https://www.rightmove.co.uk/properties/170448131#/?channel=RES_LET"
        );
        assert_eq!(card.price, Some(1250));
        assert_eq!(card.display_price.as_deref(), Some("\u{00a3}1,250 pcm"));
        assert_eq!(card.beds, Some(2));
        assert_eq!(card.baths, Some(1));
        assert_eq!(card.property_type.as_deref(), Some("Flat"));
        assert_eq!(card.address.as_deref(), Some("Station Road, York"));
        assert_eq!(
            card.summary.as_deref(),
            Some("Two bedroom flat near the station.")
        );
        assert_eq!(card.source, "api");
    }

    #[test]
    fn html_listing_card_parser_prefers_embedded_json_cards() {
        let html = r#"
            <html>
              <body>
                <script id="__NEXT_DATA__" type="application/json">
                  {
                    "props": {
                      "pageProps": {
                        "searchResults": {
                          "properties": [
                            {
                              "id": "170001101",
                              "price": { "displayPrices": [{ "displayPrice": "\u00a31,475 pcm" }] },
                              "bedrooms": 3,
                              "bathrooms": 2,
                              "propertyTypeFullDescription": "Terraced house",
                              "displayAddress": "Market Street, York",
                              "summary": "<p>Central home with courtyard.</p>"
                            }
                          ]
                        }
                      }
                    }
                  }
                </script>
                <a href="/properties/999999999">Fallback id</a>
              </body>
            </html>
        "#;

        let cards = extract_listing_cards_from_html(html);

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "170001101");
        assert_eq!(cards[0].price, Some(1475));
        assert_eq!(cards[0].beds, Some(3));
        assert_eq!(cards[0].baths, Some(2));
        assert_eq!(cards[0].property_type.as_deref(), Some("Terraced house"));
        assert_eq!(cards[0].address.as_deref(), Some("Market Street, York"));
        assert_eq!(
            cards[0].summary.as_deref(),
            Some("Central home with courtyard.")
        );
        assert_eq!(cards[0].source, "html");
    }

    #[test]
    fn market_summary_counts_rents_types_and_duplicate_location_matches() {
        let mut duplicate = test_card("2", Some(1_500), Some("Flat"));
        duplicate.location_matches = vec!["York".to_owned(), "Leeds".to_owned()];
        duplicate.primary_search_area = Some("York".to_owned());

        let listings = vec![
            test_card("1", Some(1_000), Some("Flat")),
            duplicate,
            test_card("3", Some(2_000), Some("Terraced house")),
            test_card("4", None, None),
        ];

        let summary = summarize_market(&listings);

        assert_eq!(summary.count, 4);
        assert_eq!(summary.priced_count, 3);
        assert_eq!(summary.min_rent, Some(1_000));
        assert_eq!(summary.median_rent, Some(1_500.0));
        assert_eq!(summary.max_rent, Some(2_000));
        assert_eq!(summary.by_type["Flat"].count, 2);
        assert_eq!(summary.by_type["Flat"].priced_count, 2);
        assert_eq!(summary.by_type["Flat"].median_rent, Some(1_250.0));
        assert_eq!(summary.by_type["unknown"].count, 1);
        assert_eq!(summary.duplicate_ids, vec!["2"]);
        assert_eq!(
            summary.duplicate_location_matches["2"],
            vec!["York".to_owned(), "Leeds".to_owned()]
        );
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
                true,
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
                true,
                &format!("{}/api/_search", server.uri()),
                &format!("{}/property-to-rent/find.html", server.uri()),
            ),
        );

        assert_eq!(outcome.ids.len(), 27, "{outcome:?}");
        assert_eq!(outcome.stats.pages_fetched, 2);
        assert_eq!(outcome.stats.source_mode, "html-fallback");
        assert!(!outcome.stats.truncated);
    }

    #[test]
    fn discovery_uses_html_only_when_config_disables_api() {
        let (mock_runtime, server) = start_mock_server();
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
                false,
                &format!("{}/api/_search", server.uri()),
                &format!("{}/property-to-rent/find.html", server.uri()),
            ),
        );

        assert_eq!(outcome.ids.len(), 27, "{outcome:?}");
        assert_eq!(outcome.stats.pages_fetched, 2);
        assert_eq!(outcome.stats.source_mode, "html");
        assert!(!outcome.stats.truncated);

        let requests = mock_runtime
            .block_on(async { server.received_requests().await })
            .expect("request recording enabled");
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path() == "/api/_search")
                .count(),
            0
        );
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

    fn empty_discover_params() -> DiscoverParams {
        DiscoverParams {
            region: None,
            location: None,
            min_price: None,
            max_price: None,
            min_bedrooms: None,
            max_bedrooms: None,
            radius: None,
            include_let_agreed: None,
            property_types: None,
            must_have: None,
            dont_show: None,
            location_name: None,
            limit: None,
        }
    }

    fn test_card(id: &str, price: Option<i64>, property_type: Option<&str>) -> ListingCardSummary {
        ListingCardSummary {
            id: id.to_owned(),
            url: format!("https://www.rightmove.co.uk/properties/{id}"),
            price,
            display_price: None,
            beds: None,
            baths: None,
            property_type: property_type.map(ToOwned::to_owned),
            address: None,
            summary: None,
            source: "test".to_owned(),
            location_matches: vec!["York".to_owned()],
            primary_search_area: Some("York".to_owned()),
        }
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
