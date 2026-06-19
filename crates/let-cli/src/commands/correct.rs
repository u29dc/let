#![forbid(unsafe_code)]

use let_sdk::intelligence::{CorrectionClearParams, CorrectionKind, CorrectionSaveParams};
use let_sdk::paths::resolve_paths;
use serde_json::{Value, json};

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone)]
pub struct AddressCorrectionParams {
    pub id: String,
    pub address: Option<String>,
    pub postcode: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EpcCorrectionParams {
    pub id: String,
    pub certificate_url: Option<String>,
    pub lmk_key: Option<String>,
    pub uprn: Option<String>,
    pub rating: Option<String>,
    pub floor_area_sqm: Option<f64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaCorrectionParams {
    pub id: String,
    pub map_lat: Option<f64>,
    pub map_lng: Option<f64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClearCorrectionParams {
    pub id: String,
    pub kind: CorrectionKind,
    pub correction_id: String,
}

pub fn address(shared: &SharedArgs, params: AddressCorrectionParams) -> CommandResult {
    if params
        .address
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        && params
            .postcode
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && params.lat.is_none()
        && params.lng.is_none()
    {
        return Err(validation_error(
            "address correction requires at least one of --address, --postcode, or --lat/--lng",
            "pass the most precise address evidence you have",
        ));
    }
    validate_lat_lng_pair(params.lat, params.lng, "--lat", "--lng")?;
    let mut payload = serde_json::Map::new();
    insert_string(&mut payload, "address", params.address);
    insert_string(&mut payload, "postcode", params.postcode);
    insert_number(&mut payload, "lat", params.lat);
    insert_number(&mut payload, "lng", params.lng);

    save(
        shared,
        params.id,
        CorrectionKind::Address,
        Value::Object(payload),
        params.note,
    )
}

pub fn epc(shared: &SharedArgs, params: EpcCorrectionParams) -> CommandResult {
    if params
        .certificate_url
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        && params
            .lmk_key
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && params
            .uprn
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && params
            .rating
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        && params.floor_area_sqm.is_none()
    {
        return Err(validation_error(
            "EPC correction requires at least one EPC field",
            "pass --certificate-url, --lmk-key, --uprn, --rating, or --floor-area-sqm",
        ));
    }
    let mut payload = serde_json::Map::new();
    insert_string(&mut payload, "certificateUrl", params.certificate_url);
    insert_string(&mut payload, "lmkKey", params.lmk_key);
    insert_string(&mut payload, "uprn", params.uprn);
    insert_string(&mut payload, "rating", params.rating);
    insert_number(&mut payload, "floorAreaSqm", params.floor_area_sqm);

    save(
        shared,
        params.id,
        CorrectionKind::Epc,
        Value::Object(payload),
        params.note,
    )
}

pub fn media(shared: &SharedArgs, params: MediaCorrectionParams) -> CommandResult {
    validate_lat_lng_pair(params.map_lat, params.map_lng, "--map-lat", "--map-lng")?;
    if params.map_lat.is_none() {
        return Err(validation_error(
            "media correction requires --map-lat and --map-lng",
            "pass exact map coordinates to regenerate map media",
        ));
    }
    save(
        shared,
        params.id,
        CorrectionKind::Media,
        json!({
            "mapLat": params.map_lat,
            "mapLng": params.map_lng,
        }),
        params.note,
    )
}

pub fn clear(shared: &SharedArgs, params: ClearCorrectionParams) -> CommandResult {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let response = let_sdk::intelligence::clear_correction(CorrectionClearParams {
        id: params.id,
        kind: params.kind,
        correction_id: params.correction_id,
        database_path: paths.derived.database,
    })?;
    Ok(CommandOutput::new(to_camel_json(&response)))
}

fn save(
    shared: &SharedArgs,
    id: String,
    kind: CorrectionKind,
    payload: Value,
    note: Option<String>,
) -> CommandResult {
    let paths = resolve_paths(Some(shared.overrides.clone()));
    let response = let_sdk::intelligence::save_correction(CorrectionSaveParams {
        id,
        kind,
        payload,
        note,
        database_path: paths.derived.database,
    })?;
    Ok(CommandOutput::new(to_camel_json(&response)))
}

fn validate_lat_lng_pair(
    lat: Option<f64>,
    lng: Option<f64>,
    lat_flag: &str,
    lng_flag: &str,
) -> Result<(), CommandError> {
    if lat.is_some() != lng.is_some() {
        return Err(validation_error(
            format!("{lat_flag} and {lng_flag} must be passed together"),
            "pass both latitude and longitude, or omit both",
        ));
    }
    if let Some(value) = lat
        && !(-90.0..=90.0).contains(&value)
    {
        return Err(validation_error(
            format!("{lat_flag} must be between -90 and 90"),
            "check the coordinate and retry",
        ));
    }
    if let Some(value) = lng
        && !(-180.0..=180.0).contains(&value)
    {
        return Err(validation_error(
            format!("{lng_flag} must be between -180 and 180"),
            "check the coordinate and retry",
        ));
    }
    Ok(())
}

fn insert_string(map: &mut serde_json::Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value.map(|item| item.trim().to_owned())
        && !value.is_empty()
    {
        map.insert(key.to_owned(), Value::String(value));
    }
}

fn insert_number(map: &mut serde_json::Map<String, Value>, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        map.insert(key.to_owned(), json!(value));
    }
}

fn validation_error(message: impl Into<String>, hint: impl Into<String>) -> CommandError {
    CommandError::runtime("VALIDATION_ERROR", message, hint)
}
