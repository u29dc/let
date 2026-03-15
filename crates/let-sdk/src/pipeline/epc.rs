#![forbid(unsafe_code)]

use std::collections::{BTreeSet, HashSet};

use reqwest::header::ACCEPT;
use serde_json::Value;

use crate::errors::{ErrorCode, LetError, Result};
use crate::schema::listing::EpcBand;
use crate::utils::text::normalize_postcode;

const EPC_API_BASE_URL: &str = "https://epc.opendatacommunities.org/api/v1";
const ADDRESS_SEARCH_SIZE: usize = 25;
const POSTCODE_SEARCH_SIZE: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpcCredentials {
    pub email: String,
    pub api_key: String,
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
    lookup_domestic_epc_with_base_url(client, credentials, address, postcode, EPC_API_BASE_URL)
        .await
}

async fn lookup_domestic_epc_with_base_url(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    address: &str,
    postcode: &str,
    base_url: &str,
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

    let detailed = fetch_certificate(client, credentials, &selected.lmk_key, base_url)
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
) -> Result<Vec<EpcCandidate>> {
    let url = build_search_url(base_url, address, postcode, size)?;
    let body = fetch_json(client, credentials, &url, "epc domestic search").await?;
    Ok(extract_records(&body)
        .into_iter()
        .filter_map(parse_candidate)
        .collect::<Vec<_>>())
}

async fn fetch_certificate(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    lmk_key: &str,
    base_url: &str,
) -> Result<Option<EpcCandidate>> {
    let url = format!("{base_url}/domestic/certificate/{lmk_key}");
    let body = fetch_json(client, credentials, &url, "epc domestic certificate").await?;
    Ok(extract_records(&body).into_iter().find_map(parse_candidate))
}

async fn fetch_json(
    client: &reqwest::Client,
    credentials: &EpcCredentials,
    url: &str,
    label: &str,
) -> Result<Value> {
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .basic_auth(&credentials.email, Some(&credentials.api_key))
        .send()
        .await
        .map_err(|error| {
            LetError::new(
                ErrorCode::Network,
                format!("{label} request failed: {error}"),
                "check EPC_API_EMAIL, EPC_API_KEY, and network connectivity",
            )
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(LetError::new(
            ErrorCode::Network,
            format!("{label} failed: http {}", status.as_u16()),
            "check EPC_API_EMAIL, EPC_API_KEY, and EPC API availability",
        ));
    }

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
        pairs.append_pair("size", &size.to_string());
        if let Some(address) = address.map(str::trim).filter(|value| !value.is_empty()) {
            pairs.append_pair("address", address);
        }
    }

    Ok(url.to_string())
}

fn extract_records(body: &Value) -> Vec<&Value> {
    if let Some(items) = body.as_array() {
        return items.iter().collect();
    }

    if let Some(items) = body
        .get("rows")
        .and_then(Value::as_array)
        .or_else(|| body.get("data").and_then(Value::as_array))
    {
        return items.iter().collect();
    }

    body.as_object().map_or_else(Vec::new, |_| vec![body])
}

fn parse_candidate(record: &Value) -> Option<EpcCandidate> {
    let lmk_key = read_string_field(record, &["lmk-key", "lmk_key", "LMK_KEY"])?;
    let full_address = read_string_field(record, &["address", "ADDRESS"]).or_else(|| {
        let mut parts = vec![
            read_string_field(record, &["address1", "ADDRESS1"]),
            read_string_field(record, &["address2", "ADDRESS2"]),
            read_string_field(record, &["address3", "ADDRESS3"]),
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
            ],
        )
        .as_deref()
        .and_then(parse_epc_band),
        floor_area_sqm: read_f64_field(
            record,
            &["total-floor-area", "TOTAL_FLOOR_AREA", "totalFloorArea"],
        ),
        lodgement_date: read_string_field(
            record,
            &["lodgement-date", "LODGEMENT_DATE", "lodgementDate"],
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
        .map(|candidate| {
            (
                address_match_score(listing_address, &candidate.address),
                candidate,
            )
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

fn tokenize_address(value: &str) -> HashSet<String> {
    normalize_address(value)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
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
        EpcCredentials, lookup_domestic_epc_with_base_url, normalize_address, parse_candidate,
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

        let candidate = parse_candidate(&record).expect("candidate");
        assert_eq!(candidate.lmk_key, "cert-1");
        assert_eq!(
            candidate.address,
            "Flat 2 Example House, 10 Sample Road, SW1A 1AA"
        );
        assert_eq!(candidate.floor_area_sqm, Some(71.5));
        assert_eq!(candidate.uprn.as_deref(), Some("100021345678"));
    }

    #[test]
    fn lookup_domestic_epc_prefers_address_scoped_match() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let server = MockServer::start().await;
            let credentials = EpcCredentials {
                email: "user@example.com".to_owned(),
                api_key: "secret".to_owned(),
            };
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
            let credentials = EpcCredentials {
                email: "user@example.com".to_owned(),
                api_key: "secret".to_owned(),
            };

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
            )
            .await
            .expect("lookup succeeds")
            .expect("lookup result");

            assert_eq!(result.lmk_key, "cert-456");
            assert_eq!(result.epc_rating, Some(EpcBand::C));
        });
    }
}
