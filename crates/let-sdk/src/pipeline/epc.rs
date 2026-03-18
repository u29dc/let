#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashSet};

use reqwest::header::ACCEPT;
use serde_json::Value;

use crate::errors::{ErrorCode, LetError, Result};
use crate::schema::listing::EpcBand;
use crate::utils::text::normalize_postcode;

const EPC_LEGACY_API_BASE_URL: &str = "https://epc.opendatacommunities.org/api/v1";
const EPC_MODERN_API_BASE_URL: &str =
    "https://api.get-energy-performance-data.communities.gov.uk/api";
const ADDRESS_SEARCH_SIZE: usize = 25;
const POSTCODE_SEARCH_SIZE: usize = 100;
const ADDRESS_MATCH_CONFIDENT_SCORE: f64 = 0.92;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpcAuth {
    BearerToken(String),
    LegacyBasic { email: String, api_key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpcCredentials {
    pub auth: EpcAuth,
}

impl EpcCredentials {
    pub fn bearer_token(token: impl Into<String>) -> Self {
        Self {
            auth: EpcAuth::BearerToken(token.into()),
        }
    }

    pub fn legacy_basic(email: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            auth: EpcAuth::LegacyBasic {
                email: email.into(),
                api_key: api_key.into(),
            },
        }
    }

    fn api_kind(&self) -> EpcApiKind {
        match self.auth {
            EpcAuth::BearerToken(_) => EpcApiKind::Modern,
            EpcAuth::LegacyBasic { .. } => EpcApiKind::Legacy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EpcApiKind {
    Legacy,
    Modern,
}

impl EpcApiKind {
    fn default_base_url(self) -> &'static str {
        match self {
            Self::Legacy => EPC_LEGACY_API_BASE_URL,
            Self::Modern => EPC_MODERN_API_BASE_URL,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EpcLookup {
    pub lmk_key: String,
    pub epc_rating: Option<EpcBand>,
    pub floor_area_sqm: Option<f64>,
    pub lodgement_date: Option<String>,
    pub address_match: bool,
    pub matched_address: String,
    pub uprn: Option<String>,
    pub uprn_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct EpcCandidate {
    lmk_key: String,
    address: String,
    postcode: Option<String>,
    epc_rating: Option<EpcBand>,
    floor_area_sqm: Option<f64>,
    lodgement_date: Option<String>,
    uprn: Option<String>,
    uprn_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AddressKey {
    house_number: Option<String>,
    street: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum CandidateMode {
    AddressScoped,
    PostcodeOnly,
}

pub async fn lookup_domestic_epc(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    address: &str,
    postcode: &str,
) -> Result<Option<EpcLookup>> {
    let api_kind = credentials.api_kind();
    lookup_domestic_epc_with_base_url(
        client,
        credentials,
        address,
        postcode,
        api_kind.default_base_url(),
        api_kind,
    )
    .await
}

async fn lookup_domestic_epc_with_base_url(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    address: &str,
    postcode: &str,
    base_url: &str,
    api_kind: EpcApiKind,
) -> Result<Option<EpcLookup>> {
    let normalized_postcode = normalize_postcode(postcode);
    if normalized_postcode.is_empty() {
        return Ok(None);
    }

    let normalized_address = normalize_address(address);
    let address_candidates = if normalized_address.is_empty() {
        Vec::new()
    } else {
        search_candidates(
            client,
            credentials,
            Some(address),
            &normalized_postcode,
            ADDRESS_SEARCH_SIZE,
            base_url,
            api_kind,
        )
        .await?
    };
    let postcode_candidates = if address_candidates.is_empty() {
        search_candidates(
            client,
            credentials,
            None,
            &normalized_postcode,
            POSTCODE_SEARCH_SIZE,
            base_url,
            api_kind,
        )
        .await?
    } else {
        Vec::new()
    };

    let selected = select_best_candidate(
        &address_candidates,
        address,
        &normalized_postcode,
        CandidateMode::AddressScoped,
    )
    .or_else(|| {
        select_best_candidate(
            &postcode_candidates,
            address,
            &normalized_postcode,
            CandidateMode::PostcodeOnly,
        )
    });

    let Some(selected) = selected else {
        return Ok(None);
    };

    let detailed = fetch_certificate(client, credentials, &selected.lmk_key, base_url, api_kind)
        .await
        .ok()
        .flatten();
    let merged = merge_candidate(detailed.as_ref(), &selected);
    let score = address_match_score(address, &merged.address);

    Ok(Some(EpcLookup {
        lmk_key: merged.lmk_key,
        epc_rating: merged.epc_rating,
        floor_area_sqm: merged.floor_area_sqm,
        lodgement_date: merged.lodgement_date,
        address_match: score >= 0.85,
        matched_address: merged.address,
        uprn: merged.uprn,
        uprn_source: merged.uprn_source,
    }))
}

async fn search_candidates(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    address: Option<&str>,
    postcode: &str,
    size: usize,
    base_url: &str,
    api_kind: EpcApiKind,
) -> Result<Vec<EpcCandidate>> {
    let url = build_search_url(base_url, address, postcode, size, api_kind)?;
    let Some(body) = fetch_search_json(client, credentials, &url, "epc domestic search").await?
    else {
        return Ok(Vec::new());
    };
    Ok(extract_records(&body)
        .into_iter()
        .filter_map(|record| parse_candidate(record, None))
        .collect::<Vec<_>>())
}

async fn fetch_certificate(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    lmk_key: &str,
    base_url: &str,
    api_kind: EpcApiKind,
) -> Result<Option<EpcCandidate>> {
    let url = build_certificate_url(base_url, lmk_key, api_kind)?;
    let body = fetch_json(client, credentials, &url, "epc domestic certificate").await?;
    Ok(extract_records(&body)
        .into_iter()
        .find_map(|record| parse_candidate(record, Some(lmk_key))))
}

async fn fetch_search_json(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    url: &str,
    label: &str,
) -> Result<Option<Value>> {
    let response = send_request(client, credentials, url, label).await?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(http_status_error(label, status));
    }

    parse_json_response(response, label).await.map(Some)
}

async fn fetch_json(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    url: &str,
    label: &str,
) -> Result<Value> {
    let response = send_request(client, credentials, url, label).await?;

    let status = response.status();
    if !status.is_success() {
        return Err(http_status_error(label, status));
    }

    parse_json_response(response, label).await
}

async fn send_request(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    url: &str,
    label: &str,
) -> Result<reqwest::Response> {
    let request = client.get(url).header(ACCEPT, "application/json");
    let request = match &credentials.auth {
        EpcAuth::BearerToken(token) => request.bearer_auth(token),
        EpcAuth::LegacyBasic { email, api_key } => request.basic_auth(email, Some(api_key)),
    };

    request.send().await.map_err(|error| {
        LetError::new(
            ErrorCode::Network,
            format!("{label} request failed: {error}"),
            "check EPC_API_BEARER_TOKEN or legacy EPC_API_EMAIL/EPC_API_KEY and network connectivity",
        )
    })
}

fn http_status_error(label: &str, status: reqwest::StatusCode) -> LetError {
    LetError::new(
        ErrorCode::Network,
        format!("{label} failed: http {}", status.as_u16()),
        "check EPC_API_BEARER_TOKEN or legacy EPC_API_EMAIL/EPC_API_KEY and EPC API availability",
    )
}

async fn parse_json_response(response: reqwest::Response, label: &str) -> Result<Value> {
    response.json::<Value>().await.map_err(|error| {
        LetError::new(
            ErrorCode::Parse,
            format!("failed to parse {label} response: {error}"),
            "check EPC API response format",
        )
    })
}

fn build_search_url(
    base_url: &str,
    address: Option<&str>,
    postcode: &str,
    size: usize,
    api_kind: EpcApiKind,
) -> Result<String> {
    let mut url = reqwest::Url::parse(&format!("{base_url}/domestic/search")).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("invalid EPC API base URL: {error}"),
            "check EPC API base URL configuration",
        )
    })?;

    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("postcode", postcode);
        let page_size_key = match api_kind {
            EpcApiKind::Legacy => "size",
            EpcApiKind::Modern => "page_size",
        };
        pairs.append_pair(page_size_key, &size.to_string());
        if let Some(address) = address.map(str::trim).filter(|value| !value.is_empty()) {
            pairs.append_pair("address", address);
        }
    }

    Ok(url.to_string())
}

fn build_certificate_url(
    base_url: &str,
    certificate_id: &str,
    api_kind: EpcApiKind,
) -> Result<String> {
    let url_text = match api_kind {
        EpcApiKind::Legacy => format!("{base_url}/domestic/certificate/{certificate_id}"),
        EpcApiKind::Modern => {
            let mut url =
                reqwest::Url::parse(&format!("{base_url}/certificate")).map_err(|error| {
                    LetError::new(
                        ErrorCode::Internal,
                        format!("invalid EPC API base URL: {error}"),
                        "check EPC API base URL configuration",
                    )
                })?;
            url.query_pairs_mut()
                .append_pair("certificate_number", certificate_id);
            url.to_string()
        }
    };

    Ok(url_text)
}

fn extract_records(body: &Value) -> Vec<&Value> {
    if let Some(items) = body.as_array() {
        return items.iter().collect();
    }

    if let Some(data) = body.get("data") {
        if let Some(items) = data.as_array() {
            return items.iter().collect();
        }
        if data.is_object() {
            return vec![data];
        }
    }

    if let Some(items) = body.get("rows").and_then(Value::as_array) {
        return items.iter().collect();
    }

    body.as_object().map_or_else(Vec::new, |_| vec![body])
}

fn parse_candidate(record: &Value, default_lmk_key: Option<&str>) -> Option<EpcCandidate> {
    let lmk_key = read_string_field(
        record,
        &[
            "lmk-key",
            "lmk_key",
            "LMK_KEY",
            "certificateNumber",
            "certificate_number",
        ],
    )
    .or_else(|| default_lmk_key.map(ToOwned::to_owned))?;
    let full_address = read_string_field(record, &["address", "ADDRESS"]).or_else(|| {
        let mut parts = vec![
            read_string_field(record, &["address1", "addressLine1", "ADDRESS1"]),
            read_string_field(record, &["address2", "addressLine2", "ADDRESS2"]),
            read_string_field(record, &["address3", "addressLine3", "ADDRESS3"]),
            read_string_field(record, &["address4", "addressLine4", "ADDRESS4"]),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        if parts.is_empty() {
            None
        } else {
            let postcode = read_string_field(record, &["postcode", "POSTCODE"]);
            if let Some(postcode) = postcode {
                parts.push(postcode);
            }
            Some(parts.join(", "))
        }
    })?;

    Some(EpcCandidate {
        lmk_key,
        address: full_address,
        postcode: read_string_field(record, &["postcode", "POSTCODE"]),
        epc_rating: read_string_field(
            record,
            &[
                "current-energy-rating",
                "CURRENT_ENERGY_RATING",
                "currentEnergyRating",
                "currentEnergyEfficiencyBand",
            ],
        )
        .as_deref()
        .and_then(parse_epc_band),
        floor_area_sqm: read_f64_field(
            record,
            &[
                "total-floor-area",
                "TOTAL_FLOOR_AREA",
                "totalFloorArea",
                "total_floor_area",
            ],
        ),
        lodgement_date: read_string_field(
            record,
            &[
                "lodgement-date",
                "LODGEMENT_DATE",
                "lodgementDate",
                "registrationDate",
                "registration_date",
            ],
        ),
        uprn: read_string_field(record, &["uprn", "UPRN"]),
        uprn_source: read_string_field(record, &["uprn-source", "UPRN_SOURCE", "uprnSource"]),
    })
}

fn select_best_candidate(
    candidates: &[EpcCandidate],
    listing_address: &str,
    postcode: &str,
    mode: CandidateMode,
) -> Option<EpcCandidate> {
    let normalized_postcode = normalize_postcode(postcode);
    let normalized_address = normalize_address(listing_address);
    let listing_key = extract_address_key(listing_address);

    if normalized_address.is_empty() {
        return candidates
            .iter()
            .find(|candidate| candidate_matches_postcode(candidate, &normalized_postcode))
            .cloned();
    }

    let threshold = match mode {
        CandidateMode::AddressScoped => {
            if candidates.len() <= 1 {
                0.0
            } else {
                0.25
            }
        }
        CandidateMode::PostcodeOnly => {
            if candidates.len() <= 1 {
                0.0
            } else {
                0.55
            }
        }
    };

    candidates
        .iter()
        .filter(|candidate| candidate_matches_postcode(candidate, &normalized_postcode))
        .filter_map(|candidate| {
            candidate_match_score(&listing_key, listing_address, &candidate.address, mode)
                .map(|score| (score, candidate))
        })
        .max_by(|left, right| {
            left.0
                .partial_cmp(&right.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|(score, candidate)| (score >= threshold).then(|| candidate.clone()))
}

fn candidate_matches_postcode(candidate: &EpcCandidate, postcode: &str) -> bool {
    candidate
        .postcode
        .as_deref()
        .map(normalize_postcode)
        .is_none_or(|candidate_postcode| candidate_postcode == postcode)
}

fn merge_candidate(detailed: Option<&EpcCandidate>, selected: &EpcCandidate) -> EpcCandidate {
    let Some(detailed) = detailed else {
        return selected.clone();
    };

    EpcCandidate {
        lmk_key: detailed.lmk_key.clone(),
        address: if detailed.address.trim().is_empty() {
            selected.address.clone()
        } else {
            detailed.address.clone()
        },
        postcode: detailed
            .postcode
            .clone()
            .or_else(|| selected.postcode.clone()),
        epc_rating: detailed
            .epc_rating
            .clone()
            .or_else(|| selected.epc_rating.clone()),
        floor_area_sqm: detailed.floor_area_sqm.or(selected.floor_area_sqm),
        lodgement_date: detailed
            .lodgement_date
            .clone()
            .or_else(|| selected.lodgement_date.clone()),
        uprn: detailed.uprn.clone().or_else(|| selected.uprn.clone()),
        uprn_source: detailed
            .uprn_source
            .clone()
            .or_else(|| selected.uprn_source.clone()),
    }
}

fn address_match_score(listing_address: &str, epc_address: &str) -> f64 {
    let left_key = extract_address_key(listing_address);
    let right_key = extract_address_key(epc_address);
    if left_key.house_number.is_some()
        && left_key.house_number == right_key.house_number
        && left_key.street.is_some()
        && left_key.street == right_key.street
    {
        return ADDRESS_MATCH_CONFIDENT_SCORE;
    }

    let left = normalize_address(listing_address);
    let right = normalize_address(epc_address);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }
    if left.contains(&right) || right.contains(&left) {
        return 0.9;
    }

    let left_tokens = tokenize_address(&left);
    let right_tokens = tokenize_address(&right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }

    let overlap = left_tokens.intersection(&right_tokens).count() as f64;
    if overlap == 0.0 {
        return 0.0;
    }

    overlap / left_tokens.len().max(right_tokens.len()) as f64
}

fn candidate_match_score(
    listing_key: &AddressKey,
    listing_address: &str,
    candidate_address: &str,
    mode: CandidateMode,
) -> Option<f64> {
    let candidate_key = extract_address_key(candidate_address);
    let full_score = address_match_score(listing_address, candidate_address);
    let require_house_number_match =
        listing_key.street.is_some() && listing_key.house_number.is_some();

    if let (Some(listing_number), Some(candidate_number)) = (
        listing_key.house_number.as_deref(),
        candidate_key.house_number.as_deref(),
    ) && require_house_number_match
        && listing_number != candidate_number
    {
        return None;
    }

    let street_score = street_match_score(
        listing_key.street.as_deref(),
        candidate_key.street.as_deref(),
    );
    let same_number = listing_key.house_number.is_some()
        && listing_key.house_number == candidate_key.house_number;

    match mode {
        CandidateMode::AddressScoped => {
            Some(full_score.max(if same_number && street_score == 1.0 {
                ADDRESS_MATCH_CONFIDENT_SCORE
            } else {
                0.0
            }))
        }
        CandidateMode::PostcodeOnly => {
            if same_number && street_score == 1.0 {
                return Some(full_score.max(ADDRESS_MATCH_CONFIDENT_SCORE));
            }

            if require_house_number_match {
                return None;
            }

            if listing_key.street.is_none() {
                return (full_score >= 0.55).then_some(full_score);
            }

            (street_score >= 1.0 && full_score >= 0.55).then_some(full_score.max(0.8))
        }
    }
}

fn tokenize_address(value: &str) -> HashSet<String> {
    normalize_address(value)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn extract_address_key(value: &str) -> AddressKey {
    let tokens = normalize_address(value)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    let mut matched_number_idx = None;
    let mut matched_street_end_idx = None;
    for (idx, token) in tokens.iter().enumerate() {
        if !is_street_type(token) {
            continue;
        }
        if let Some(number_idx) = tokens[..idx]
            .iter()
            .rposition(|candidate| is_house_number_token(candidate))
        {
            matched_number_idx = Some(number_idx);
            matched_street_end_idx = Some(idx);
            break;
        }
    }

    let house_number = matched_number_idx
        .and_then(|idx| tokens.get(idx))
        .map(ToOwned::to_owned)
        .or_else(|| {
            tokens
                .iter()
                .find(|token| is_house_number_token(token))
                .cloned()
        });
    let street =
        matched_number_idx
            .zip(matched_street_end_idx)
            .and_then(|(number_idx, street_end_idx)| {
                let street_tokens = tokens[number_idx + 1..=street_end_idx]
                    .iter()
                    .map(|token| canonical_street_token(token).to_owned())
                    .collect::<Vec<_>>();
                (!street_tokens.is_empty()).then(|| street_tokens.join(" "))
            });

    AddressKey {
        house_number,
        street,
    }
}

fn street_match_score(left: Option<&str>, right: Option<&str>) -> f64 {
    match (left, right) {
        (Some(left), Some(right)) if left == right => 1.0,
        _ => 0.0,
    }
}

fn is_house_number_token(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_digit() && chars.all(|ch| ch.is_ascii_alphanumeric())
}

fn is_street_type(token: &str) -> bool {
    matches!(
        canonical_street_token(token),
        "road"
            | "street"
            | "lane"
            | "avenue"
            | "close"
            | "drive"
            | "way"
            | "court"
            | "place"
            | "crescent"
            | "terrace"
            | "park"
            | "grove"
            | "row"
            | "mews"
    )
}

fn canonical_street_token(token: &str) -> &str {
    match token {
        "rd" => "road",
        "st" => "street",
        "ln" => "lane",
        "ave" => "avenue",
        "cl" => "close",
        "dr" => "drive",
        "ct" => "court",
        "pl" => "place",
        "cres" => "crescent",
        "ter" => "terrace",
        "gr" => "grove",
        _ => token,
    }
}

fn normalize_address(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_space = true;

    for ch in value.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_space = false;
        } else if !previous_space {
            normalized.push(' ');
            previous_space = true;
        }
    }

    normalized.trim().to_owned()
}

fn parse_epc_band(raw: &str) -> Option<EpcBand> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "A" => Some(EpcBand::A),
        "B" => Some(EpcBand::B),
        "C" => Some(EpcBand::C),
        "D" => Some(EpcBand::D),
        "E" => Some(EpcBand::E),
        "F" => Some(EpcBand::F),
        "G" => Some(EpcBand::G),
        _ => None,
    }
}

fn read_string_field(record: &Value, names: &[&str]) -> Option<String> {
    let value = find_field(record, names)?;
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn read_f64_field(record: &Value, names: &[&str]) -> Option<f64> {
    let value = find_field(record, names)?;
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn find_field<'a>(record: &'a Value, names: &[&str]) -> Option<&'a Value> {
    let object = record.as_object()?;
    let targets = names
        .iter()
        .map(|name| normalize_field_key(name))
        .collect::<BTreeSet<_>>();

    object
        .iter()
        .find_map(|(key, value)| targets.contains(&normalize_field_key(key)).then_some(value))
}

fn normalize_field_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::schema::listing::EpcBand;

    const EPC_SEARCH_ADDRESS_FIXTURE: &str =
        include_str!("../../tests/fixtures/epc/domestic-search-address.json");
    const EPC_SEARCH_EMPTY_FIXTURE: &str =
        include_str!("../../tests/fixtures/epc/domestic-search-empty.json");
    const EPC_SEARCH_POSTCODE_FIXTURE: &str =
        include_str!("../../tests/fixtures/epc/domestic-search-postcode.json");
    const EPC_CERTIFICATE_FIXTURE: &str =
        include_str!("../../tests/fixtures/epc/domestic-certificate.json");

    use super::{
        EpcApiKind, EpcCredentials, lookup_domestic_epc_with_base_url, normalize_address,
        parse_candidate,
    };

    #[test]
    fn normalize_address_collapses_punctuation() {
        assert_eq!(
            normalize_address("Flat 2, Example House, SW1A 1AA"),
            "flat 2 example house sw1a 1aa"
        );
    }

    #[test]
    fn parse_candidate_accepts_guidance_field_names() {
        let record = serde_json::json!({
            "LMK_KEY": "cert-1",
            "ADDRESS1": "Flat 2 Example House",
            "ADDRESS2": "10 Sample Road",
            "POSTCODE": "SW1A 1AA",
            "CURRENT_ENERGY_RATING": "C",
            "TOTAL_FLOOR_AREA": "71.5",
            "LODGEMENT_DATE": "2024-01-10",
            "UPRN": "100021345678",
            "UPRN_SOURCE": "Address Matched"
        });

        let candidate = parse_candidate(&record, None).expect("candidate");
        assert_eq!(candidate.lmk_key, "cert-1");
        assert_eq!(
            candidate.address,
            "Flat 2 Example House, 10 Sample Road, SW1A 1AA"
        );
        assert_eq!(candidate.floor_area_sqm, Some(71.5));
        assert_eq!(candidate.uprn.as_deref(), Some("100021345678"));
    }

    #[test]
    fn parse_candidate_accepts_modern_field_names() {
        let record = serde_json::json!({
            "certificateNumber": "0000-0000-0000-0000-0001",
            "addressLine1": "Flat 2 Example House",
            "addressLine2": "10 Sample Road",
            "postcode": "SW1A 1AA",
            "currentEnergyEfficiencyBand": "B",
            "total_floor_area": 81.2,
            "registration_date": "2025-03-14",
            "uprn": 100021345678u64,
            "uprn_source": "Address Matched"
        });

        let candidate = parse_candidate(&record, None).expect("candidate");
        assert_eq!(candidate.lmk_key, "0000-0000-0000-0000-0001");
        assert_eq!(
            candidate.address,
            "Flat 2 Example House, 10 Sample Road, SW1A 1AA"
        );
        assert_eq!(candidate.epc_rating, Some(EpcBand::B));
        assert_eq!(candidate.floor_area_sqm, Some(81.2));
        assert_eq!(candidate.lodgement_date.as_deref(), Some("2025-03-14"));
        assert_eq!(candidate.uprn.as_deref(), Some("100021345678"));
    }

    #[test]
    fn lookup_domestic_epc_falls_back_from_modern_address_404_to_postcode_search() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let server = MockServer::start().await;
            let credentials = EpcCredentials::bearer_token("modern-token");

            Mock::given(method("GET"))
                .and(path("/api/domestic/search"))
                .and(query_param("postcode", "SY35FX"))
                .and(query_param("address", "14 Oliver Road Shrewsbury, SY3 5FX"))
                .and(query_param("page_size", "25"))
                .and(header("authorization", "Bearer modern-token"))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "data": { "error": "No certificates could be found for that query" }
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/domestic/search"))
                .and(query_param("postcode", "SY35FX"))
                .and(query_param("page_size", "100"))
                .and(header("authorization", "Bearer modern-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": [
                        {
                            "certificateNumber": "wrong-12",
                            "addressLine1": "12, Oliver Road",
                            "addressLine2": "Bicton Heath",
                            "postcode": "SY3 5FX",
                            "currentEnergyEfficiencyBand": "B",
                            "registrationDate": "2018-09-04",
                            "uprn": 10014545828u64
                        },
                        {
                            "certificateNumber": "right-14",
                            "addressLine1": "14, Oliver Road",
                            "addressLine2": "Bicton Heath",
                            "postcode": "SY3 5FX",
                            "currentEnergyEfficiencyBand": "B",
                            "registrationDate": "2018-09-04",
                            "uprn": 10014545830u64
                        },
                        {
                            "certificateNumber": "wrong-16",
                            "addressLine1": "16, Oliver Road",
                            "addressLine2": "Bicton Heath",
                            "postcode": "SY3 5FX",
                            "currentEnergyEfficiencyBand": "B",
                            "registrationDate": "2018-09-04",
                            "uprn": 10014545831u64
                        }
                    ],
                    "pagination": {
                        "totalRecords": 3,
                        "currentPage": 1,
                        "totalPages": 1,
                        "nextPage": null,
                        "prevPage": null,
                        "pageSize": 100
                    }
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/certificate"))
                .and(query_param("certificate_number", "right-14"))
                .and(header("authorization", "Bearer modern-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": {
                        "address_line_1": "14, Oliver Road",
                        "address_line_2": "Bicton Heath",
                        "postcode": "SY3 5FX",
                        "total_floor_area": 75,
                        "registration_date": "2018-09-04",
                        "current_energy_efficiency_band": "B",
                        "uprn": 10014545830u64,
                        "uprn_source": "Energy Assessor"
                    }
                })))
                .mount(&server)
                .await;

            let result = lookup_domestic_epc_with_base_url(
                &reqwest::Client::new(),
                &credentials,
                "14 Oliver Road Shrewsbury, SY3 5FX",
                "SY3 5FX",
                &format!("{}/api", server.uri()),
                EpcApiKind::Modern,
            )
            .await
            .expect("lookup succeeds")
            .expect("lookup result");

            assert_eq!(result.lmk_key, "right-14");
            assert_eq!(result.epc_rating, Some(EpcBand::B));
            assert_eq!(result.floor_area_sqm, Some(75.0));
            assert_eq!(result.lodgement_date.as_deref(), Some("2018-09-04"));
            assert!(result.address_match);
            assert_eq!(result.uprn.as_deref(), Some("10014545830"));
            assert_eq!(result.uprn_source.as_deref(), Some("Energy Assessor"));
        });
    }

    #[test]
    fn lookup_domestic_epc_rejects_wrong_house_number_in_postcode_fallback() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let server = MockServer::start().await;
            let credentials = EpcCredentials::bearer_token("modern-token");

            Mock::given(method("GET"))
                .and(path("/api/domestic/search"))
                .and(query_param("postcode", "SY35FX"))
                .and(query_param("address", "14 Oliver Road Shrewsbury, SY3 5FX"))
                .and(query_param("page_size", "25"))
                .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                    "data": { "error": "No certificates could be found for that query" }
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/domestic/search"))
                .and(query_param("postcode", "SY35FX"))
                .and(query_param("page_size", "100"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": [
                        {
                            "certificateNumber": "wrong-12",
                            "addressLine1": "12, Oliver Road",
                            "addressLine2": "Bicton Heath",
                            "postcode": "SY3 5FX"
                        },
                        {
                            "certificateNumber": "wrong-16",
                            "addressLine1": "16, Oliver Road",
                            "addressLine2": "Bicton Heath",
                            "postcode": "SY3 5FX"
                        }
                    ],
                    "pagination": {
                        "totalRecords": 2,
                        "currentPage": 1,
                        "totalPages": 1,
                        "nextPage": null,
                        "prevPage": null,
                        "pageSize": 100
                    }
                })))
                .mount(&server)
                .await;

            let result = lookup_domestic_epc_with_base_url(
                &reqwest::Client::new(),
                &credentials,
                "14 Oliver Road Shrewsbury, SY3 5FX",
                "SY3 5FX",
                &format!("{}/api", server.uri()),
                EpcApiKind::Modern,
            )
            .await
            .expect("lookup succeeds");

            assert!(result.is_none());
        });
    }

    #[test]
    fn lookup_domestic_epc_prefers_address_scoped_match() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let server = MockServer::start().await;
            let credentials = EpcCredentials::legacy_basic("user@example.com", "secret");
            let auth_header = "Basic dXNlckBleGFtcGxlLmNvbTpzZWNyZXQ=";

            Mock::given(method("GET"))
                .and(path("/api/v1/domestic/search"))
                .and(query_param("postcode", "SW1A1AA"))
                .and(query_param("address", "Flat 2 Example House"))
                .and(query_param("size", "25"))
                .and(header("authorization", auth_header))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::from_str::<serde_json::Value>(EPC_SEARCH_ADDRESS_FIXTURE)
                            .expect("address search fixture"),
                    ),
                )
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/v1/domestic/certificate/cert-123"))
                .and(header("authorization", auth_header))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::from_str::<serde_json::Value>(EPC_CERTIFICATE_FIXTURE)
                            .expect("certificate fixture"),
                    ),
                )
                .mount(&server)
                .await;

            let result = lookup_domestic_epc_with_base_url(
                &reqwest::Client::new(),
                &credentials,
                "Flat 2 Example House",
                "SW1A 1AA",
                &format!("{}/api/v1", server.uri()),
                EpcApiKind::Legacy,
            )
            .await
            .expect("lookup succeeds")
            .expect("lookup result");

            assert_eq!(result.lmk_key, "cert-123");
            assert_eq!(result.floor_area_sqm, Some(81.2));
            assert!(result.address_match);
            assert_eq!(result.uprn.as_deref(), Some("100021345678"));
        });
    }

    #[test]
    fn lookup_domestic_epc_falls_back_to_postcode_search() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let server = MockServer::start().await;
            let credentials = EpcCredentials::legacy_basic("user@example.com", "secret");

            Mock::given(method("GET"))
                .and(path("/api/v1/domestic/search"))
                .and(query_param("postcode", "SW1A1AA"))
                .and(query_param("address", "Flat 2 Example House"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::from_str::<serde_json::Value>(EPC_SEARCH_EMPTY_FIXTURE)
                            .expect("empty search fixture"),
                    ),
                )
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/v1/domestic/search"))
                .and(query_param("postcode", "SW1A1AA"))
                .and(query_param("size", "100"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::from_str::<serde_json::Value>(EPC_SEARCH_POSTCODE_FIXTURE)
                            .expect("postcode search fixture"),
                    ),
                )
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/v1/domestic/certificate/cert-456"))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;

            let result = lookup_domestic_epc_with_base_url(
                &reqwest::Client::new(),
                &credentials,
                "Flat 2 Example House",
                "SW1A 1AA",
                &format!("{}/api/v1", server.uri()),
                EpcApiKind::Legacy,
            )
            .await
            .expect("lookup succeeds")
            .expect("lookup result");

            assert_eq!(result.lmk_key, "cert-456");
            assert_eq!(result.epc_rating, Some(EpcBand::C));
        });
    }

    #[test]
    fn lookup_domestic_epc_supports_modern_api() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let server = MockServer::start().await;
            let credentials = EpcCredentials::bearer_token("modern-token");

            Mock::given(method("GET"))
                .and(path("/api/domestic/search"))
                .and(query_param("postcode", "SW1A1AA"))
                .and(query_param("address", "Flat 2 Example House"))
                .and(query_param("page_size", "25"))
                .and(header("authorization", "Bearer modern-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": [
                        {
                            "certificateNumber": "0000-0000-0000-0000-0001",
                            "addressLine1": "Flat 2 Example House",
                            "addressLine2": "10 Sample Road",
                            "postcode": "SW1A 1AA",
                            "currentEnergyEfficiencyBand": "C",
                            "registrationDate": "2025-03-14",
                            "uprn": 100021345678u64
                        }
                    ],
                    "pagination": {
                        "totalRecords": 1,
                        "currentPage": 1,
                        "totalPages": 1,
                        "nextPage": null,
                        "prevPage": null,
                        "pageSize": 25
                    }
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/api/certificate"))
                .and(query_param(
                    "certificate_number",
                    "0000-0000-0000-0000-0001",
                ))
                .and(header("authorization", "Bearer modern-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": {
                        "address_line_1": "Flat 2 Example House",
                        "address_line_2": "10 Sample Road",
                        "postcode": "SW1A 1AA",
                        "total_floor_area": 81.2,
                        "registration_date": "2025-03-14",
                        "current_energy_efficiency_band": "B",
                        "uprn": 100021345678u64,
                        "uprn_source": "Address Matched"
                    }
                })))
                .mount(&server)
                .await;

            let result = lookup_domestic_epc_with_base_url(
                &reqwest::Client::new(),
                &credentials,
                "Flat 2 Example House",
                "SW1A 1AA",
                &format!("{}/api", server.uri()),
                EpcApiKind::Modern,
            )
            .await
            .expect("lookup succeeds")
            .expect("lookup result");

            assert_eq!(result.lmk_key, "0000-0000-0000-0000-0001");
            assert_eq!(result.epc_rating, Some(EpcBand::B));
            assert_eq!(result.floor_area_sqm, Some(81.2));
            assert_eq!(result.lodgement_date.as_deref(), Some("2025-03-14"));
            assert!(result.address_match);
            assert_eq!(result.uprn.as_deref(), Some("100021345678"));
            assert_eq!(result.uprn_source.as_deref(), Some("Address Matched"));
        });
    }
}
