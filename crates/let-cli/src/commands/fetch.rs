#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::DateTime;
use let_sdk::config::load_config;
use let_sdk::schema::listing::{
    Agent, ExtractionStatus, GeoLocation, Lettings, Listing, ListingImage, ListingStatus, MapViews,
    PinType, PortalIds, RemoteLocalAsset, StationDistance,
};
use let_sdk::{
    DbMeta, EnrichmentMode, SourceEnricher, load_listings_file, recalc_assessed_scores,
    score_listings_with_config, upsert_listings,
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone)]
pub struct FetchParams {
    pub ids: String,
    pub region: Option<String>,
    pub override_postcode: Option<String>,
    pub override_address: Option<String>,
    pub skip_images: bool,
    pub skip_epc: bool,
}

#[derive(Debug, Clone)]
struct FetchOverride {
    postcode: Option<String>,
    address: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FetchedItem {
    id: String,
    address: String,
    score: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    enrichment_applied: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    enrichment_missing: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    enrichment_unavailable_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FailedItem {
    id: String,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FetchOutput {
    fetched: Vec<FetchedItem>,
    failed: Vec<FailedItem>,
    total: usize,
    skip_images: bool,
    skip_epc: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    override_applied: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    override_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_error: Option<String>,
}

pub fn run(shared: &SharedArgs, params: &FetchParams) -> CommandResult {
    let input_ids = parse_ids(&params.ids);
    if input_ids.is_empty() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "no ids provided",
            "provide comma-separated portal IDs",
        ));
    }
    let fetch_override = build_fetch_override(params, &input_ids)?;

    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let config = load_config(Some(&paths.derived.config_file))?;
    let db_path = paths.derived.database;
    let existing = load_listings_file(&db_path)?;
    let source_enricher = SourceEnricher::open(&paths.resolved.sources)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|error| {
            CommandError::runtime(
                "PROCESS_ERROR",
                format!("failed to initialize runtime: {error}"),
                "retry command",
            )
        })?;

    let client = runtime
        .block_on(async {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
                .build()
        })
        .map_err(|error| {
            CommandError::runtime(
                "NETWORK_ERROR",
                format!("failed to build http client: {error}"),
                "check TLS/certificate configuration",
            )
        })?;

    let mut fetched = Vec::new();
    let mut failed = Vec::new();
    let mut new_listings = Vec::new();

    for (index, id) in input_ids.iter().enumerate() {
        match fetch_one_listing(
            &runtime,
            &client,
            id,
            config.fetch.max_retries.max(1),
            params.region.clone(),
            params.skip_images,
        ) {
            Ok(mut listing) => {
                if let Some(override_input) = fetch_override.as_ref() {
                    apply_fetch_override(&mut listing, override_input);
                }
                let enrichment = source_enricher
                    .enrich_listing(&mut listing, EnrichmentMode::ReplaceFromSources)?;
                fetched.push(FetchedItem {
                    id: id.clone(),
                    address: listing.address.clone(),
                    score: None,
                    enrichment_applied: enrichment.applied_fields,
                    enrichment_missing: enrichment.missing_categories,
                    enrichment_unavailable_sources: enrichment.unavailable_sources,
                });
                new_listings.push(listing);
            }
            Err(error) => failed.push(FailedItem {
                id: id.clone(),
                error,
            }),
        }

        if config.fetch.delay_ms > 0 && index + 1 < input_ids.len() {
            std::thread::sleep(Duration::from_millis(config.fetch.delay_ms));
        }
    }

    let merged = existing
        .listings
        .iter()
        .cloned()
        .chain(new_listings)
        .collect::<Vec<_>>();
    let unique_listings = deduplicate_listings(merged);

    let mut scored = score_listings_with_config(&unique_listings, &config);
    recalc_assessed_scores(&mut scored);
    update_fetched_scores(&mut fetched, &scored);

    let existing_ids = existing
        .listings
        .iter()
        .map(|listing| listing.id.clone())
        .collect::<HashSet<_>>();
    let input_id_set = input_ids.iter().cloned().collect::<HashSet<_>>();

    let truly_new = scored
        .iter()
        .filter(|listing| !existing_ids.contains(&listing.id))
        .cloned()
        .collect::<Vec<_>>();
    let updated = scored
        .iter()
        .filter(|listing| {
            existing_ids.contains(&listing.id)
                && listing
                    .portal_ids
                    .rightmove
                    .as_ref()
                    .is_some_and(|id| input_id_set.contains(id))
        })
        .cloned()
        .collect::<Vec<_>>();

    let save_error = upsert_listings(
        &db_path,
        &truly_new,
        &updated,
        &scored,
        &DbMeta {
            updated_at: let_sdk::utils::time::now_iso(),
            last_search_total: existing.last_search_total,
        },
        &existing.search_urls,
        &existing.locations,
    )
    .err()
    .map(|error| error.to_string());

    let payload = FetchOutput {
        fetched,
        failed,
        total: input_ids.len(),
        skip_images: params.skip_images,
        skip_epc: params.skip_epc,
        override_applied: fetch_override.as_ref().map(|_| true),
        override_fields: fetch_override_fields(fetch_override.as_ref()),
        save_error,
    };

    Ok(CommandOutput::new(crate::commands::to_camel_json(&payload))
        .with_count(payload.fetched.len())
        .with_total(payload.total)
        .with_has_more(!payload.failed.is_empty())
        .with_text(format!(
            "fetched {} of {} listing(s)",
            payload.fetched.len(),
            payload.total
        )))
}

fn parse_ids(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn build_fetch_override(
    params: &FetchParams,
    input_ids: &[String],
) -> std::result::Result<Option<FetchOverride>, CommandError> {
    let postcode = params.override_postcode.as_deref().map(str::trim);
    let address = params.override_address.as_deref().map(str::trim);
    if postcode.is_none_or(str::is_empty) && address.is_none_or(str::is_empty) {
        return Ok(None);
    }

    if input_ids.len() != 1 {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "fetch overrides require exactly one id",
            "run `let fetch <single-id> --override-postcode ... --override-address ...`",
        ));
    }

    let normalized_postcode = postcode
        .filter(|value| !value.is_empty())
        .map(canonicalize_postcode)
        .filter(|value| !value.is_empty());
    let normalized_address = address
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if normalized_postcode.is_none() && normalized_address.is_none() {
        return Ok(None);
    }

    Ok(Some(FetchOverride {
        postcode: normalized_postcode,
        address: normalized_address,
    }))
}

fn apply_fetch_override(listing: &mut Listing, override_input: &FetchOverride) {
    if let Some(postcode) = override_input.postcode.as_ref() {
        listing.postcode = postcode.clone();
    }
    if let Some(address) = override_input.address.as_ref() {
        listing.address = address.clone();
    }

    listing.google_maps_url = build_google_maps_url(
        listing.location.lat,
        listing.location.lng,
        &listing.address,
        &listing.postcode,
    );
    listing.epc_search_url = if listing.postcode.is_empty() {
        None
    } else {
        let encoded_postcode =
            url::form_urlencoded::byte_serialize(listing.postcode.as_bytes()).collect::<String>();
        Some(format!(
            "https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode={}",
            encoded_postcode
        ))
    };
}

fn fetch_override_fields(override_input: Option<&FetchOverride>) -> Vec<String> {
    let Some(override_input) = override_input else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    if override_input.postcode.is_some() {
        fields.push("postcode".to_owned());
    }
    if override_input.address.is_some() {
        fields.push("address".to_owned());
    }
    fields
}

fn canonicalize_postcode(raw: &str) -> String {
    let compact = let_sdk::utils::text::normalize_postcode(raw);
    if compact.len() <= 3 {
        return compact;
    }
    let split = compact.len() - 3;
    format!("{} {}", &compact[..split], &compact[split..])
}

fn fetch_one_listing(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    rightmove_id: &str,
    max_retries: usize,
    region: Option<String>,
    skip_images: bool,
) -> std::result::Result<Listing, String> {
    let html = fetch_listing_html(runtime, client, rightmove_id, max_retries)?;
    let page_model = extract_page_model(&html)?;
    transform_listing(&page_model, rightmove_id, region, skip_images)
}

fn fetch_listing_html(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    rightmove_id: &str,
    max_retries: usize,
) -> std::result::Result<String, String> {
    let url = format!("https://www.rightmove.co.uk/properties/{rightmove_id}");
    let mut last_error = String::new();

    for attempt in 1..=max_retries {
        let result = runtime.block_on(async { client.get(&url).send().await });
        match result {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    return runtime
                        .block_on(async { response.text().await })
                        .map_err(|error| format!("failed to read response body: {error}"));
                }

                let retryable = status.as_u16() == 429 || status.is_server_error();
                last_error = format!("http {}", status.as_u16());
                if retryable && attempt < max_retries {
                    let backoff = Duration::from_millis((attempt as u64) * 1000);
                    std::thread::sleep(backoff);
                    continue;
                }
                return Err(format!("fetch failed: {last_error}"));
            }
            Err(error) => {
                last_error = error.to_string();
                if attempt < max_retries {
                    let backoff = Duration::from_millis((attempt as u64) * 1000);
                    std::thread::sleep(backoff);
                    continue;
                }
                return Err(format!("fetch failed: {last_error}"));
            }
        }
    }

    Err(format!("fetch failed: {last_error}"))
}

fn extract_page_model(html: &str) -> std::result::Result<Value, String> {
    let marker = if let Some(index) = html.find("window.PAGE_MODEL") {
        ("window.PAGE_MODEL", index)
    } else if let Some(index) = html.find("window.pageModel") {
        ("window.pageModel", index)
    } else {
        return Err("PAGE_MODEL marker not found".to_owned());
    };

    let mut cursor = marker.1 + marker.0.len();
    let bytes = html.as_bytes();
    while let Some(ch) = bytes.get(cursor) {
        if *ch == b'=' {
            cursor += 1;
            break;
        }
        cursor += 1;
    }

    while let Some(ch) = bytes.get(cursor) {
        if !ch.is_ascii_whitespace() {
            break;
        }
        cursor += 1;
    }

    let remaining = &html[cursor..];
    let end = find_json_end(remaining).ok_or_else(|| "invalid PAGE_MODEL JSON".to_owned())?;
    let json_text = &remaining[..=end];
    serde_json::from_str::<Value>(json_text)
        .map_err(|error| format!("invalid PAGE_MODEL JSON: {error}"))
}

fn find_json_end(input: &str) -> Option<usize> {
    let mut chars = input.char_indices();
    if chars.next()?.1 != '{' {
        return None;
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string {
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn transform_listing(
    page_model: &Value,
    rightmove_id: &str,
    region: Option<String>,
    skip_images: bool,
) -> std::result::Result<Listing, String> {
    let property_data = get_path(page_model, &["propertyData"])
        .ok_or_else(|| "propertyData not found".to_owned())?;

    let lat = get_path(property_data, &["location", "latitude"])
        .and_then(Value::as_f64)
        .ok_or_else(|| "location.latitude not found".to_owned())?;
    let lng = get_path(property_data, &["location", "longitude"])
        .and_then(Value::as_f64)
        .ok_or_else(|| "location.longitude not found".to_owned())?;

    let price_display = get_path(property_data, &["prices", "primaryPrice"])
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let price =
        parse_price(&price_display).ok_or_else(|| "price not found or invalid".to_owned())?;

    let outcode = get_path(property_data, &["address", "outcode"]).and_then(Value::as_str);
    let incode = get_path(property_data, &["address", "incode"]).and_then(Value::as_str);
    let postcode = match (outcode, incode) {
        (Some(left), Some(right)) => format!("{left} {right}"),
        _ => String::new(),
    };

    let address = get_path(property_data, &["address", "displayAddress"])
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let description = build_description(property_data);
    let images = if skip_images {
        Vec::new()
    } else {
        extract_images(property_data)
    };
    let stations = extract_stations(property_data);
    let has_all_optional = !postcode.is_empty()
        && !description.is_empty()
        && !images.is_empty()
        && !stations.is_empty();

    let floorplan = RemoteLocalAsset {
        remote: extract_first_url(get_path(property_data, &["floorplans"])),
        local: None,
    };
    let epc = RemoteLocalAsset {
        remote: extract_first_url(get_path(property_data, &["epcGraphs"])),
        local: None,
    };

    let pin_type = get_path(property_data, &["location", "pinType"])
        .and_then(Value::as_str)
        .and_then(parse_pin_type);
    let bedrooms = get_path(property_data, &["bedrooms"])
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let bathrooms = get_path(property_data, &["bathrooms"])
        .and_then(Value::as_i64)
        .filter(|value| *value >= 1)
        .unwrap_or(1);
    let property_type = get_path(property_data, &["propertySubType"])
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
        .to_owned();
    let listed_date = get_path(property_data, &["listingHistory", "listingUpdateReason"])
        .and_then(Value::as_str)
        .and_then(parse_listed_date);
    let lettings = Lettings {
        available_date: get_path(property_data, &["lettings", "letAvailableDate"])
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        deposit: get_path(property_data, &["lettings", "deposit"]).and_then(value_to_i64),
    };
    let agent = Agent {
        name: get_path(property_data, &["customer", "branchDisplayName"])
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        phone: get_path(
            property_data,
            &["contactInfo", "telephoneNumbers", "localNumber"],
        )
        .and_then(Value::as_str)
        .map(ToOwned::to_owned),
    };

    let epc_search_url = if postcode.is_empty() {
        None
    } else {
        let encoded_postcode =
            url::form_urlencoded::byte_serialize(postcode.as_bytes()).collect::<String>();
        Some(format!(
            "https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode={}",
            encoded_postcode
        ))
    };

    Ok(Listing {
        id: Uuid::new_v4().to_string(),
        portal_ids: PortalIds {
            rightmove: Some(rightmove_id.to_owned()),
            zoopla: None,
            onthemarket: None,
        },
        uprn: None,
        uprn_source: None,
        uprn_confidence: None,
        url: format!("https://www.rightmove.co.uk/properties/{rightmove_id}"),
        location: GeoLocation { lat, lng, pin_type },
        postcode: postcode.clone(),
        address: address.clone(),
        region,
        google_maps_url: build_google_maps_url(lat, lng, &address, &postcode),
        google_maps_street_view_url: build_google_maps_street_view_url(lat, lng),
        area: Default::default(),
        price,
        price_display: if price_display.is_empty() {
            format!("£{price} pcm")
        } else {
            price_display
        },
        bedrooms,
        bathrooms,
        property_type,
        description,
        notes: Vec::new(),
        images,
        floorplan,
        epc,
        map_views: MapViews::default(),
        epc_rating: None,
        floor_area_sqm: None,
        epc_lodgement_date: None,
        epc_address_match: None,
        epc_search_url,
        nearest_stations: stations,
        gigabit_availability: None,
        listed_date,
        lettings,
        agent,
        assessment: None,
        assessed_at: None,
        assessed_score: None,
        scores: None,
        fetched_at: let_sdk::utils::time::now_iso(),
        extraction_status: if has_all_optional {
            ExtractionStatus::Success
        } else {
            ExtractionStatus::Partial
        },
        status: ListingStatus::Active,
        notion_page_id: None,
    })
}

fn parse_pin_type(raw: &str) -> Option<PinType> {
    match raw {
        "ACCURATE_POINT" => Some(PinType::AccuratePoint),
        "APPROXIMATE_POINT" => Some(PinType::ApproximatePoint),
        _ => None,
    }
}

fn build_description(property_data: &Value) -> String {
    let description = get_path(property_data, &["text", "description"])
        .and_then(Value::as_str)
        .unwrap_or_default();
    let features = get_path(property_data, &["keyFeatures"])
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    sanitize_for_ai(&format!("{features} {description}"))
}

fn sanitize_for_ai(input: &str) -> String {
    let decoded = decode_html_entities(input);
    let stripped = strip_html_tags(&decoded).to_lowercase();
    let mut out = String::with_capacity(stripped.len());
    let mut previous_space = false;

    for ch in stripped.chars() {
        let keep =
            ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '.' | ',' | '!' | '?' | '\'' | '-');
        let normalized = if keep { ch } else { ' ' };
        if normalized.is_whitespace() {
            if !previous_space {
                out.push(' ');
                previous_space = true;
            }
        } else {
            out.push(normalized);
            previous_space = false;
        }
    }

    out.trim().to_owned()
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&pound;", "£")
}

fn extract_images(property_data: &Value) -> Vec<ListingImage> {
    get_path(property_data, &["images"])
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_object()
                        .and_then(|obj| obj.get("url"))
                        .and_then(Value::as_str)
                        .map(|url| ListingImage {
                            remote: url.to_owned(),
                            local: None,
                        })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn extract_first_url(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("url"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn extract_stations(property_data: &Value) -> Vec<StationDistance> {
    let mut stations = get_path(property_data, &["nearestStations"])
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|station| {
                    let obj = station.as_object()?;
                    let name = obj.get("name").and_then(Value::as_str)?.to_owned();
                    let distance = obj.get("distance").and_then(Value::as_f64)?;
                    let unit_raw = obj
                        .get("unit")
                        .and_then(Value::as_str)
                        .unwrap_or("miles")
                        .to_owned();
                    let miles = normalize_station_distance(distance, &unit_raw);
                    Some(StationDistance {
                        name,
                        distance: miles,
                        unit: "miles".to_owned(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    stations.sort_by(|left, right| {
        left.distance
            .partial_cmp(&right.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if stations.len() > 3 {
        stations.truncate(3);
    }
    stations
}

fn normalize_station_distance(distance: f64, unit: &str) -> f64 {
    let normalized = unit.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "miles" | "mile" | "mi") {
        return distance;
    }
    if matches!(
        normalized.as_str(),
        "km" | "kilometer" | "kilometers" | "kilometre" | "kilometres"
    ) {
        return distance * 0.621_371_f64;
    }
    if matches!(
        normalized.as_str(),
        "m" | "meter" | "meters" | "metre" | "metres"
    ) {
        return distance * 0.000_621_371_f64;
    }
    distance
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|item| i64::try_from(item).ok()))
        .or_else(|| value.as_f64().map(|item| item.round() as i64))
}

fn parse_price(price: &str) -> Option<i64> {
    if price.trim().is_empty() {
        return None;
    }
    let normalized = price.to_ascii_lowercase();
    let numeric_source = normalized.replace([',', '£'], "");

    let has_weekly = normalized.contains("pw")
        || normalized.contains("per week")
        || normalized.contains("p/w")
        || normalized.contains("weekly");
    let has_monthly = normalized.contains("pcm")
        || normalized.contains("per month")
        || normalized.contains("per calendar month")
        || normalized.contains("p/m")
        || normalized.contains("monthly");

    let mut number = String::new();
    for ch in numeric_source.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number.push(ch);
        } else if !number.is_empty() {
            break;
        }
    }

    let amount = number.parse::<f64>().ok()?;
    let monthly = if has_weekly && !has_monthly {
        amount * (52.0 / 12.0)
    } else {
        amount
    };

    Some(monthly.round() as i64)
}

fn parse_listed_date(update_reason: &str) -> Option<String> {
    let chars = update_reason.chars().collect::<Vec<_>>();
    for i in 0..chars.len().saturating_sub(9) {
        let candidate = chars[i..=i + 9].iter().collect::<String>();
        let bytes = candidate.as_bytes();
        if bytes.get(2) == Some(&b'/') && bytes.get(5) == Some(&b'/') {
            let day = &candidate[0..2];
            let month = &candidate[3..5];
            let year = &candidate[6..10];
            if day.chars().all(|ch| ch.is_ascii_digit())
                && month.chars().all(|ch| ch.is_ascii_digit())
                && year.chars().all(|ch| ch.is_ascii_digit())
            {
                return Some(format!("{year}-{month}-{day}"));
            }
        }
    }

    let tokens = update_reason.split_whitespace().collect::<Vec<_>>();
    for window in tokens.windows(3) {
        let day = window[0].trim_matches(|ch: char| !ch.is_ascii_alphanumeric());
        let month = window[1].to_ascii_lowercase();
        let year = window[2].trim_matches(|ch: char| !ch.is_ascii_alphanumeric());
        if !(day.chars().all(|ch| ch.is_ascii_digit())
            && year.len() == 4
            && year.chars().all(|ch| ch.is_ascii_digit()))
        {
            continue;
        }
        if let Some(month_num) = month_name_to_num(&month) {
            return Some(format!("{year}-{month_num}-{:0>2}", day));
        }
    }

    None
}

fn month_name_to_num(month: &str) -> Option<&'static str> {
    match month {
        "january" => Some("01"),
        "february" => Some("02"),
        "march" => Some("03"),
        "april" => Some("04"),
        "may" => Some("05"),
        "june" => Some("06"),
        "july" => Some("07"),
        "august" => Some("08"),
        "september" => Some("09"),
        "october" => Some("10"),
        "november" => Some("11"),
        "december" => Some("12"),
        _ => None,
    }
}

fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn build_google_maps_url(lat: f64, lng: f64, address: &str, postcode: &str) -> String {
    let place = if address.is_empty() && postcode.is_empty() {
        format!("{lat},{lng}")
    } else {
        format!("{address}, {postcode}")
    };
    let encoded_place = url::form_urlencoded::byte_serialize(place.as_bytes()).collect::<String>();
    format!(
        "https://www.google.com/maps/place/{}/@{lat},{lng},17z/data=!3m1!1e3",
        encoded_place
    )
}

fn build_google_maps_street_view_url(lat: f64, lng: f64) -> String {
    format!("https://www.google.com/maps/@?api=1&map_action=pano&viewpoint={lat},{lng}")
}

fn deduplicate_listings(listings: Vec<Listing>) -> Vec<Listing> {
    let mut index_by_key = HashMap::<String, usize>::new();
    let mut unique = Vec::<Listing>::new();

    for mut listing in listings {
        let key = listing
            .portal_ids
            .rightmove
            .clone()
            .unwrap_or_else(|| listing.id.clone());

        if let Some(existing_index) = index_by_key.get(&key).copied() {
            let existing = unique
                .get(existing_index)
                .cloned()
                .expect("existing dedupe index should be valid");
            carry_over_persistent_fields(&mut listing, &existing);
            if is_newer_listing(&listing, &existing) {
                unique[existing_index] = listing;
            }
        } else {
            index_by_key.insert(key, unique.len());
            unique.push(listing);
        }
    }

    unique
}

fn carry_over_persistent_fields(incoming: &mut Listing, existing: &Listing) {
    if incoming.portal_ids.rightmove != existing.portal_ids.rightmove {
        return;
    }

    incoming.id = existing.id.clone();
    if incoming.portal_ids.zoopla.is_none() {
        incoming.portal_ids.zoopla = existing.portal_ids.zoopla.clone();
    }
    if incoming.portal_ids.onthemarket.is_none() {
        incoming.portal_ids.onthemarket = existing.portal_ids.onthemarket.clone();
    }
    if incoming.notion_page_id.is_none() {
        incoming.notion_page_id = existing.notion_page_id.clone();
    }
    if incoming.assessment.is_none() {
        incoming.assessment = existing.assessment.clone();
    }
    if incoming.assessed_at.is_none() {
        incoming.assessed_at = existing.assessed_at.clone();
    }
    if incoming.assessed_score.is_none() {
        incoming.assessed_score = existing.assessed_score;
    }
}

fn is_newer_listing(left: &Listing, right: &Listing) -> bool {
    let left_ts = parse_timestamp(&left.fetched_at);
    let right_ts = parse_timestamp(&right.fetched_at);
    left_ts > right_ts
}

fn parse_timestamp(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .unwrap_or(0)
}

fn update_fetched_scores(items: &mut [FetchedItem], listings: &[Listing]) {
    for item in items.iter_mut() {
        item.score = listings
            .iter()
            .find(|listing| listing.portal_ids.rightmove.as_deref() == Some(item.id.as_str()))
            .and_then(|listing| listing.scores.as_ref().map(|scores| scores.overall));
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_page_model, find_json_end, parse_ids, parse_price};

    #[test]
    fn parse_ids_ignores_empty_segments() {
        let ids = parse_ids("1701, ,1702,,1703");
        assert_eq!(ids, vec!["1701", "1702", "1703"]);
    }

    #[test]
    fn parse_price_converts_weekly_to_monthly() {
        assert_eq!(parse_price("£950 pw"), Some(4117));
        assert_eq!(parse_price("£1,250 pcm"), Some(1250));
    }

    #[test]
    fn json_end_handles_nested_objects() {
        let input = r#"{"a":{"b":"x"},"c":1} trailing"#;
        let end = find_json_end(input).expect("json end should exist");
        assert_eq!(&input[..=end], r#"{"a":{"b":"x"},"c":1}"#);
    }

    #[test]
    fn extract_page_model_supports_uppercase_marker() {
        let html = r#"<script>window.PAGE_MODEL = {"propertyData":{"id":1}}</script>"#;
        let model = extract_page_model(html).expect("page model should parse");
        assert_eq!(model["propertyData"]["id"], 1);
    }
}
