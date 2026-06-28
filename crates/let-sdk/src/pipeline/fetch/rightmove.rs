#![forbid(unsafe_code)]

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::schema::listing::{
    Agent, ExtractionStatus, GeoLocation, Lettings, Listing, ListingImage, ListingStatus, MapViews,
    PinType, PortalIds, RemoteLocalAsset, StationDistance,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightmoveListingPageStatus {
    Active,
    LetAgreed,
    Removed,
}

impl RightmoveListingPageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::LetAgreed => "letAgreed",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RightmovePageCapture {
    pub rightmove_id: String,
    pub url: String,
    pub fetched_at: String,
    pub page_status: String,
    pub content_hash: String,
    pub page_model: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RightmoveDescriptionExtract {
    pub raw_html: String,
    pub text: String,
    pub normalized_text: String,
    pub key_features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RightmoveMediaExtract {
    pub photos: Vec<String>,
    pub floorplans: Vec<String>,
    pub epc_graphs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RightmovePropertyExtract {
    pub rightmove_id: String,
    pub url: String,
    pub page_status: String,
    pub fetched_at: String,
    pub content_hash: String,
    pub title: Option<String>,
    pub address: Option<String>,
    pub postcode: Option<String>,
    pub display_price: Option<String>,
    pub price_pcm: Option<i64>,
    pub bedrooms: Option<i64>,
    pub bathrooms: Option<i64>,
    pub property_type: Option<String>,
    pub agent_name: Option<String>,
    pub agent_phone: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub pin_type: Option<String>,
    pub listed_date: Option<String>,
    pub available_date: Option<String>,
    pub deposit: Option<i64>,
    pub description: RightmoveDescriptionExtract,
    #[serde(default)]
    pub nearest_stations: Vec<StationDistance>,
    pub media: RightmoveMediaExtract,
}

pub fn fetch_page_capture(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    rightmove_id: &str,
    max_retries: usize,
) -> std::result::Result<RightmovePageCapture, String> {
    let html = fetch_listing_html(runtime, client, rightmove_id, max_retries)?;
    capture_page_model(rightmove_id, &html)
}

pub fn capture_page_model(
    rightmove_id: &str,
    html: &str,
) -> std::result::Result<RightmovePageCapture, String> {
    let page_status = classify_listing_page(html)?;
    let page_model = match extract_page_model(html) {
        Ok(page_model) => page_model,
        Err(_) if page_status == RightmoveListingPageStatus::Removed => {
            removed_listing_page_model()
        }
        Err(error) => return Err(error),
    };
    Ok(RightmovePageCapture {
        rightmove_id: rightmove_id.to_owned(),
        url: rightmove_url(rightmove_id),
        fetched_at: crate::utils::time::now_iso(),
        page_status: page_status.as_str().to_owned(),
        content_hash: sha256_hex(html),
        page_model,
    })
}

fn removed_listing_page_model() -> Value {
    json!({
        "propertyData": {
            "heading": "Removed Rightmove listing",
            "text": {
                "description": "This property has been removed from the market."
            },
            "images": [],
            "floorplans": [],
            "epcGraphs": []
        }
    })
}

pub fn extract_property_evidence(
    capture: &RightmovePageCapture,
) -> std::result::Result<RightmovePropertyExtract, String> {
    let property_data = get_path(&capture.page_model, &["propertyData"])
        .ok_or_else(|| "propertyData not found".to_owned())?;

    let price_display = get_path(property_data, &["prices", "primaryPrice"])
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let outcode = get_path(property_data, &["address", "outcode"]).and_then(Value::as_str);
    let incode = get_path(property_data, &["address", "incode"]).and_then(Value::as_str);
    let postcode = match (outcode, incode) {
        (Some(left), Some(right)) => Some(format!("{left} {right}")),
        _ => None,
    };
    let listing_history =
        get_path(property_data, &["listingHistory", "listingUpdateReason"]).and_then(Value::as_str);

    Ok(RightmovePropertyExtract {
        rightmove_id: capture.rightmove_id.clone(),
        url: capture.url.clone(),
        page_status: capture.page_status.clone(),
        fetched_at: capture.fetched_at.clone(),
        content_hash: capture.content_hash.clone(),
        title: get_path(property_data, &["heading"]).and_then(string_value),
        address: get_path(property_data, &["address", "displayAddress"]).and_then(string_value),
        postcode,
        price_pcm: price_display.as_deref().and_then(parse_price),
        display_price: price_display,
        bedrooms: get_path(property_data, &["bedrooms"]).and_then(Value::as_i64),
        bathrooms: get_path(property_data, &["bathrooms"]).and_then(Value::as_i64),
        property_type: get_path(property_data, &["propertySubType"]).and_then(string_value),
        agent_name: get_path(property_data, &["customer", "branchDisplayName"])
            .and_then(string_value),
        agent_phone: get_path(
            property_data,
            &["contactInfo", "telephoneNumbers", "localNumber"],
        )
        .and_then(string_value),
        latitude: get_path(property_data, &["location", "latitude"]).and_then(Value::as_f64),
        longitude: get_path(property_data, &["location", "longitude"]).and_then(Value::as_f64),
        pin_type: get_path(property_data, &["location", "pinType"]).and_then(string_value),
        listed_date: listing_history.and_then(parse_listed_date),
        available_date: get_path(property_data, &["lettings", "letAvailableDate"])
            .and_then(string_value),
        deposit: get_path(property_data, &["lettings", "deposit"]).and_then(value_to_i64),
        description: extract_description(property_data),
        nearest_stations: extract_stations(property_data),
        media: RightmoveMediaExtract {
            photos: extract_image_urls(get_path(property_data, &["images"])),
            floorplans: extract_url_list(get_path(property_data, &["floorplans"])),
            epc_graphs: extract_url_list(get_path(property_data, &["epcGraphs"])),
        },
    })
}

pub fn listing_from_capture(
    capture: &RightmovePageCapture,
    region: Option<String>,
    include_images: bool,
) -> std::result::Result<Listing, String> {
    transform_listing(
        &capture.page_model,
        &capture.rightmove_id,
        region,
        include_images,
    )
}

pub fn fetch_listing(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    rightmove_id: &str,
    max_retries: usize,
    region: Option<String>,
    include_images: bool,
) -> std::result::Result<Listing, String> {
    let html = fetch_listing_html(runtime, client, rightmove_id, max_retries)?;
    let page_model = extract_page_model(&html)?;
    transform_listing(&page_model, rightmove_id, region, include_images)
}

fn fetch_listing_html(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    rightmove_id: &str,
    max_retries: usize,
) -> std::result::Result<String, String> {
    let url = format!("https://www.rightmove.co.uk/properties/{rightmove_id}");
    let mut last_error = String::new();

    for attempt in 1..=max_retries.max(1) {
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

pub fn classify_listing_page(
    html: &str,
) -> std::result::Result<RightmoveListingPageStatus, String> {
    match extract_page_model(html) {
        Ok(page_model) => match get_path(
            &page_model,
            &["analyticsInfo", "analyticsProperty", "letAgreed"],
        )
        .and_then(Value::as_bool)
        {
            Some(true) => Ok(RightmoveListingPageStatus::LetAgreed),
            Some(false) => Ok(RightmoveListingPageStatus::Active),
            None => {
                if detect_removed_html(html) {
                    Ok(RightmoveListingPageStatus::Removed)
                } else {
                    Err("listing availability signal not found in PAGE_MODEL".to_owned())
                }
            }
        },
        Err(error) => {
            if detect_removed_html(html) {
                Ok(RightmoveListingPageStatus::Removed)
            } else {
                Err(format!(
                    "listing availability could not be determined: {error}"
                ))
            }
        }
    }
}

pub fn extract_page_model(html: &str) -> std::result::Result<Value, String> {
    let marker = if let Some(index) = html.find("window.PAGE_MODEL") {
        ("window.PAGE_MODEL", index)
    } else if let Some(index) = html.find("window.__PAGE_MODEL") {
        ("window.__PAGE_MODEL", index)
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
    let page_model = serde_json::from_str::<Value>(json_text)
        .map_err(|error| format!("invalid PAGE_MODEL JSON: {error}"))?;
    decode_page_model(page_model)
}

fn decode_page_model(page_model: Value) -> std::result::Result<Value, String> {
    let Some(encoded) = page_model.get("data").and_then(Value::as_str) else {
        return Ok(page_model);
    };
    let slots = serde_json::from_str::<Vec<Value>>(encoded)
        .map_err(|error| format!("invalid encoded PAGE_MODEL data: {error}"))?;
    decode_indexed_value(&slots, 0, &mut Vec::new())
}

fn decode_indexed_value(
    slots: &[Value],
    index: usize,
    stack: &mut Vec<usize>,
) -> std::result::Result<Value, String> {
    if stack.contains(&index) {
        return Err("encoded PAGE_MODEL contains a cyclic reference".to_owned());
    }
    let Some(value) = slots.get(index) else {
        return Err(format!(
            "encoded PAGE_MODEL reference {index} is out of range"
        ));
    };

    stack.push(index);
    let decoded = match value {
        Value::Array(items) => {
            let mut decoded_items = Vec::with_capacity(items.len());
            for item in items {
                decoded_items.push(decode_indexed_slot_value(slots, item, stack)?);
            }
            Value::Array(decoded_items)
        }
        Value::Object(map) => {
            let mut decoded_map = serde_json::Map::with_capacity(map.len());
            for (key, item) in map {
                decoded_map.insert(key.clone(), decode_indexed_slot_value(slots, item, stack)?);
            }
            Value::Object(decoded_map)
        }
        other => other.clone(),
    };
    stack.pop();
    Ok(decoded)
}

fn decode_indexed_slot_value(
    slots: &[Value],
    value: &Value,
    stack: &mut Vec<usize>,
) -> std::result::Result<Value, String> {
    if let Some(index) = value.as_u64() {
        let index = usize::try_from(index)
            .map_err(|_| "encoded PAGE_MODEL reference is too large".to_owned())?;
        decode_indexed_value(slots, index, stack)
    } else {
        Ok(value.clone())
    }
}

pub fn find_json_end(input: &str) -> Option<usize> {
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
    include_images: bool,
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
    let images = if include_images {
        extract_images(property_data)
    } else {
        Vec::new()
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
        url: rightmove_url(rightmove_id),
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
        fetched_at: crate::utils::time::now_iso(),
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
    let extracted = extract_description(property_data);
    let features = extracted.key_features.join(", ");
    let description = extracted.text;
    sanitize_for_ai(&format!("{features} {description}"))
}

fn extract_description(property_data: &Value) -> RightmoveDescriptionExtract {
    let raw_html = get_path(property_data, &["text", "description"])
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let decoded = decode_html_entities(&raw_html);
    let text = normalize_spaces(&strip_html_tags(&decoded));
    let key_features = get_path(property_data, &["keyFeatures"])
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(normalize_spaces)
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    RightmoveDescriptionExtract {
        raw_html,
        text: text.clone(),
        normalized_text: sanitize_for_ai(&format!("{} {text}", key_features.join(", "))),
        key_features,
    }
}

pub fn sanitize_for_ai(input: &str) -> String {
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

pub fn strip_html_tags(input: &str) -> String {
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

pub fn decode_html_entities(input: &str) -> String {
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

fn extract_image_urls(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_object()
                        .and_then(|obj| obj.get("url"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn extract_url_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_object()
                        .and_then(|obj| obj.get("url"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
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

pub fn parse_price(price: &str) -> Option<i64> {
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

fn detect_removed_html(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("no longer on the market")
        || lower.contains("no longer available")
        || lower.contains("this property has been removed")
}

fn rightmove_url(rightmove_id: &str) -> String {
    format!("https://www.rightmove.co.uk/properties/{rightmove_id}")
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn string_value(value: &Value) -> Option<String> {
    value.as_str().map(ToOwned::to_owned)
}

fn normalize_spaces(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

pub fn build_google_maps_url(lat: f64, lng: f64, address: &str, postcode: &str) -> String {
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

pub fn build_google_maps_street_view_url(lat: f64, lng: f64) -> String {
    format!("https://www.google.com/maps/@?api=1&map_action=pano&viewpoint={lat},{lng}")
}

#[cfg(test)]
mod tests {
    use super::{
        RightmoveListingPageStatus, capture_page_model, classify_listing_page, extract_page_model,
        extract_property_evidence, find_json_end, parse_price,
    };

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

    #[test]
    fn extract_page_model_supports_encoded_underscore_marker() {
        let html = r#"
            <script>
                window.__PAGE_MODEL = {"data":"[{\"propertyData\":1,\"analyticsInfo\":8},{\"id\":2,\"prices\":3,\"location\":5},\"89606028\",{\"primaryPrice\":4},\"£1,300 pcm\",{\"latitude\":6,\"longitude\":7},51.1,1.2,{\"analyticsProperty\":9},{\"letAgreed\":10},false]","encoding":"on"};
            </script>
        "#;

        let model = extract_page_model(html).expect("page model should parse");
        assert_eq!(model["propertyData"]["id"], "89606028");
        assert_eq!(
            model["propertyData"]["prices"]["primaryPrice"],
            "£1,300 pcm"
        );
        assert_eq!(model["propertyData"]["location"]["latitude"], 51.1);
        let status = classify_listing_page(html).expect("page status should classify");
        assert_eq!(status, RightmoveListingPageStatus::Active);
    }

    #[test]
    fn classify_listing_page_uses_structured_let_agreed_signal() {
        let html = r#"
            <a href="/property-to-rent/find.html?includeLetAgreed=true">More properties</a>
            <script>
                window.PAGE_MODEL = {
                    "analyticsInfo": {
                        "analyticsProperty": {
                            "letAgreed": false
                        }
                    }
                }
            </script>
        "#;

        let status = classify_listing_page(html).expect("page status should classify");
        assert_eq!(status, RightmoveListingPageStatus::Active);
    }

    #[test]
    fn classify_listing_page_marks_let_agreed_when_signal_is_true() {
        let html = r#"
            <script>
                window.PAGE_MODEL = {
                    "analyticsInfo": {
                        "analyticsProperty": {
                            "letAgreed": true
                        }
                    }
                }
            </script>
        "#;

        let status = classify_listing_page(html).expect("page status should classify");
        assert_eq!(status, RightmoveListingPageStatus::LetAgreed);
    }

    #[test]
    fn classify_listing_page_falls_back_to_removed_markers() {
        let html = "<html><body>This property has been removed from the market.</body></html>";
        let status = classify_listing_page(html).expect("page status should classify");
        assert_eq!(status, RightmoveListingPageStatus::Removed);
    }

    #[test]
    fn removed_listing_without_page_model_captures_tombstone() {
        let html = "<html><body>This property has been removed from the market.</body></html>";

        let capture = capture_page_model("170448131", html).expect("removed page should capture");
        let extracted = extract_property_evidence(&capture).expect("tombstone should extract");

        assert_eq!(capture.page_status, "removed");
        assert_eq!(extracted.page_status, "removed");
        assert_eq!(
            extracted.title.as_deref(),
            Some("Removed Rightmove listing")
        );
    }

    #[test]
    fn classify_listing_page_errors_on_unknown_success_page_shape() {
        let html = "<html><body>Welcome to Rightmove</body></html>";
        let error = classify_listing_page(html).expect_err("page shape should be unknown");
        assert!(error.contains("could not be determined"));
    }
}
