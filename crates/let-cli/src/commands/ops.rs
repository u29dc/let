#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use chrono::NaiveDate;
use let_sdk::schema::listing::{CrimeBand, CrimeTrend, EpcBand, Listing, ListingStatus};
use let_sdk::{
    EnrichmentMode, SourceEnricher, close_listings_db, load_listings_file, open_listings_db,
    recalc_assessed_scores, replace_listing_scores, replace_listings, score_listings_with_config,
};
use rusqlite::{params, params_from_iter};
use serde_json::{Map, Value, json};

use crate::commands::{CommandError, CommandOutput, CommandResult, ErrorDetail, SharedArgs};

#[derive(Debug, Clone)]
pub struct PruneParams {
    pub min_score: Option<f64>,
    pub bottom_percent: Option<u8>,
    pub region: Option<String>,
    pub inactive_only: bool,
    pub dry_run: bool,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct VerifyParams {
    pub dry_run: bool,
    pub region: Option<String>,
    pub limit: Option<usize>,
    pub delay_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PatchParams {
    pub id: String,
    pub address: Option<String>,
    pub postcode: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub region: Option<String>,
    pub epc_rating: Option<String>,
    pub floor_area: Option<f64>,
    pub gigabit_availability: Option<f64>,
    pub crime_rate_per_1k: Option<f64>,
    pub crime_count_12m: Option<i64>,
    pub crime_violent_12m: Option<i64>,
    pub crime_burglary_12m: Option<i64>,
    pub crime_robbery_12m: Option<i64>,
    pub imd_decile: Option<i64>,
    pub imd_rank: Option<i64>,
    pub imd_score: Option<f64>,
    pub lsoa_code: Option<String>,
    pub lsoa_name: Option<String>,
    pub msoa_code: Option<String>,
    pub msoa_name: Option<String>,
    pub income_bhc: Option<f64>,
    pub income_ahc: Option<f64>,
    pub social_housing_pct: Option<f64>,
    pub population: Option<i64>,
    pub flood_risk_level: Option<String>,
    pub flood_risk_source: Option<String>,
    pub crime_band: Option<String>,
    pub crime_trend: Option<String>,
    pub crime_updated_at: Option<String>,
    pub patch_json: Option<String>,
    pub skip_re_enrich: bool,
    pub skip_images: bool,
}

#[derive(Debug, Clone, Default)]
struct PatchUpdate {
    address: Option<String>,
    postcode: Option<String>,
    lat: Option<f64>,
    lng: Option<f64>,
    region: Option<Option<String>>,
    epc_rating: Option<Option<EpcBand>>,
    floor_area: Option<Option<f64>>,
    gigabit_availability: Option<Option<f64>>,
    crime_rate_per_1k: Option<Option<f64>>,
    crime_count_12m: Option<Option<i64>>,
    crime_violent_12m: Option<Option<i64>>,
    crime_burglary_12m: Option<Option<i64>>,
    crime_robbery_12m: Option<Option<i64>>,
    imd_decile: Option<Option<i64>>,
    imd_rank: Option<Option<i64>>,
    imd_score: Option<Option<f64>>,
    lsoa_code: Option<Option<String>>,
    lsoa_name: Option<Option<String>>,
    msoa_code: Option<Option<String>>,
    msoa_name: Option<Option<String>>,
    income_bhc: Option<Option<f64>>,
    income_ahc: Option<Option<f64>>,
    social_housing_pct: Option<Option<f64>>,
    population: Option<Option<i64>>,
    flood_risk_level: Option<Option<String>>,
    flood_risk_source: Option<Option<String>>,
    crime_band: Option<Option<CrimeBand>>,
    crime_trend: Option<Option<CrimeTrend>>,
    crime_updated_at: Option<Option<String>>,
}

#[derive(Debug, Clone, Default)]
struct JsonPatchParse {
    update: PatchUpdate,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifyResult {
    id: String,
    rightmove_id: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct VerifySummary {
    active: usize,
    inactive: usize,
    errors: usize,
}

pub fn prune(shared: &SharedArgs, params: &PruneParams) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let data = load_listings_file(&db_path)?;
    if data.listings.is_empty() {
        return Ok(CommandOutput::new(json!({
            "removed": 0,
            "remaining": 0,
            "mode": "none",
            "dryRun": params.dry_run,
        }))
        .with_text("no listings to prune"));
    }

    let (to_remove_ids, mode) = select_prune_ids(&data.listings, params)?;
    if to_remove_ids.is_empty() {
        return Ok(CommandOutput::new(json!({
            "removed": 0,
            "remaining": data.listings.len(),
            "mode": mode,
            "dryRun": params.dry_run,
        }))
        .with_text("nothing to prune"));
    }

    let remaining = data.listings.len().saturating_sub(to_remove_ids.len());

    if params.dry_run {
        return Ok(CommandOutput::new(json!({
            "removed": to_remove_ids.len(),
            "remaining": remaining,
            "mode": mode,
            "dryRun": true,
        }))
        .with_text(format!(
            "dry run: would remove {} listing(s)",
            to_remove_ids.len()
        )));
    }

    if !params.force && !confirm_delete(to_remove_ids.len())? {
        return Ok(CommandOutput::new(json!({
            "removed": 0,
            "remaining": data.listings.len(),
            "mode": mode,
            "dryRun": false,
            "aborted": true,
        }))
        .with_text("prune aborted"));
    }

    delete_listing_ids(&db_path, &to_remove_ids)?;

    Ok(CommandOutput::new(json!({
        "removed": to_remove_ids.len(),
        "remaining": remaining,
        "mode": mode,
        "dryRun": false,
    }))
    .with_text(format!(
        "pruned {} listing(s); {} remaining",
        to_remove_ids.len(),
        remaining
    )))
}

pub fn patch(shared: &SharedArgs, params: &PatchParams) -> CommandResult {
    let mut update = PatchUpdate::default();
    let mut warnings = Vec::new();

    if let Some(raw_json) = params.patch_json.as_deref().map(str::trim)
        && !raw_json.is_empty()
    {
        let parsed = parse_patch_json(raw_json)?;
        update = parsed.update;
        warnings.extend(parsed.warnings);
    }

    apply_scalar_overrides(params, &mut update, &mut warnings)?;
    if !has_any_patch(&update) {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "no overrides provided",
            "provide at least one override field or --patch-json payload",
        ));
    }

    let validation_errors = validate_patch_update(&update);
    if !validation_errors.is_empty() {
        return Err(CommandError::runtime(
            "PATCH_JSON_VALIDATION_ERROR",
            "patch validation failed",
            "inspect `error.details` and correct the invalid fields",
        )
        .with_details(validation_errors));
    }

    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;
    let config_path = paths.derived.config_file;
    let mut data = load_listings_file(&db_path)?;

    let Some(index) = data.listings.iter().position(|listing| {
        listing.id == params.id
            || listing.portal_ids.rightmove.as_deref() == Some(params.id.as_str())
    }) else {
        return Err(CommandError::runtime(
            "NOT_FOUND",
            format!("listing not found: {}", params.id),
            "check id with `let view list`",
        ));
    };

    let listing_id = data
        .listings
        .get(index)
        .expect("listing index should be valid")
        .id
        .clone();
    let previous_score = data
        .listings
        .get(index)
        .and_then(|listing| listing.scores.as_ref().map(|scores| scores.overall));

    let mut applied = serde_json::Map::new();
    {
        let listing = data
            .listings
            .get_mut(index)
            .expect("listing index should be valid");
        apply_update_to_listing(listing, &update, &mut applied);
    }

    if applied.is_empty() {
        return Ok(CommandOutput::new(json!({
            "id": listing_id,
            "applied": {},
            "reEnriched": [],
            "reEnrichMissing": [],
            "reEnrichUnavailableSources": [],
            "rescored": data.listings.len(),
            "previousScore": previous_score,
            "newScore": previous_score,
            "warnings": warnings,
            "skipReEnrich": params.skip_re_enrich,
            "skipImages": params.skip_images,
        }))
        .with_text("no changes needed"));
    }

    let mut re_enriched = Vec::new();
    let mut re_enrich_missing = Vec::new();
    let mut re_enrich_unavailable_sources = Vec::new();
    if !params.skip_re_enrich {
        let source_enricher = SourceEnricher::open(&paths.resolved.sources)?;
        let applied_keys = applied.keys().cloned().collect::<HashSet<_>>();
        let report = {
            let listing = data
                .listings
                .get_mut(index)
                .expect("listing index should be valid");
            let report =
                source_enricher.enrich_listing(listing, EnrichmentMode::ReplaceFromSources)?;
            // Re-apply manual patch values so explicit user edits always win in this invocation.
            let mut restored = serde_json::Map::new();
            apply_update_to_listing(listing, &update, &mut restored);
            report
        };
        re_enriched = report
            .applied_fields
            .into_iter()
            .filter(|field| !applied_keys.contains(field))
            .collect::<Vec<_>>();
        re_enriched.sort();
        re_enriched.dedup();
        re_enrich_missing = report.missing_categories;
        re_enrich_unavailable_sources = report.unavailable_sources;
    }

    let config = let_sdk::config::load_config(Some(&config_path))?;
    let mut rescored = score_listings_with_config(&data.listings, &config);
    recalc_assessed_scores(&mut rescored);

    let new_score = rescored
        .iter()
        .find(|candidate| candidate.id == listing_id)
        .and_then(|candidate| candidate.scores.as_ref().map(|scores| scores.overall));

    let updated_at = let_sdk::utils::time::now_iso();
    let patched_listing = rescored
        .iter()
        .find(|candidate| candidate.id == listing_id)
        .cloned()
        .expect("patched listing should still exist after rescoring");
    replace_listings(
        &db_path,
        std::slice::from_ref(&patched_listing),
        &updated_at,
    )?;
    replace_listing_scores(&db_path, &rescored, &updated_at)?;

    Ok(CommandOutput::new(json!({
        "id": listing_id,
        "applied": serde_json::Value::Object(applied),
        "reEnriched": re_enriched,
        "reEnrichMissing": re_enrich_missing,
        "reEnrichUnavailableSources": re_enrich_unavailable_sources,
        "rescored": rescored.len(),
        "previousScore": previous_score,
        "newScore": new_score,
        "warnings": warnings,
        "skipReEnrich": params.skip_re_enrich,
        "skipImages": params.skip_images,
    }))
    .with_text(format!(
        "patch applied for {} and rescored {} listings",
        params.id,
        rescored.len()
    )))
}

fn parse_patch_json(raw_json: &str) -> Result<JsonPatchParse, CommandError> {
    let value: Value = serde_json::from_str(raw_json).map_err(|error| {
        CommandError::runtime(
            "PATCH_JSON_PARSE_ERROR",
            format!("invalid --patch-json payload: {error}"),
            "provide a valid JSON object string for --patch-json",
        )
        .with_details(vec![error_detail(
            "$".to_owned(),
            "invalid_json",
            "patch payload is not valid JSON".to_owned(),
            Some("JSON object".to_owned()),
            Some(error.to_string()),
            Some("check commas, quotes, and braces in --patch-json".to_owned()),
        )])
    })?;

    let Some(object) = value.as_object() else {
        return Err(CommandError::runtime(
            "PATCH_JSON_SCHEMA_ERROR",
            "invalid --patch-json payload",
            "--patch-json must be a JSON object",
        )
        .with_details(vec![error_detail(
            "$".to_owned(),
            "invalid_type",
            "patch payload must be an object".to_owned(),
            Some("object".to_owned()),
            Some(value_type_name(&value).to_owned()),
            Some("wrap fields in { ... }".to_owned()),
        )]));
    };

    let mut parsed = JsonPatchParse::default();
    let mut details = Vec::new();

    const TOP_LEVEL_FIELDS: &[&str] = &[
        "address",
        "postcode",
        "lat",
        "lng",
        "region",
        "epcRating",
        "floorArea",
        "gigabitAvailability",
        "crimeRatePer1k",
        "crimeCount12m",
        "crimeViolent12m",
        "crimeBurglary12m",
        "crimeRobbery12m",
        "imdDecile",
        "imdRank",
        "imdScore",
        "lsoaCode",
        "lsoaName",
        "msoaCode",
        "msoaName",
        "incomeBhc",
        "incomeAhc",
        "socialHousingPct",
        "population",
        "floodRiskLevel",
        "floodRiskSource",
        "crimeBand",
        "crimeTrend",
        "crimeUpdatedAt",
        "area",
    ];

    for key in object.keys() {
        if !TOP_LEVEL_FIELDS.contains(&key.as_str()) {
            details.push(error_detail(
                format!("$.{key}"),
                "unknown_field",
                format!("unknown patch field `{key}`"),
                Some("one of the documented patch fields".to_owned()),
                None,
                Some("remove this field or move it to a supported path".to_owned()),
            ));
        }
    }

    if let Some(value) = object.get("area") {
        parse_area_patch(value, &mut parsed.update, &mut details);
    }

    if let Some(value) = object.get("address")
        && let Some(parsed_value) = parse_required_string("$.address", value, &mut details)
    {
        parsed.update.address = Some(parsed_value);
    }

    if let Some(value) = object.get("postcode")
        && let Some(parsed_value) = parse_required_string("$.postcode", value, &mut details)
    {
        parsed.update.postcode = Some(canonicalize_postcode(&parsed_value));
    }

    if let Some(value) = object.get("lat")
        && let Some(parsed_value) = parse_required_f64("$.lat", value, &mut details)
    {
        parsed.update.lat = Some(parsed_value);
    }
    if let Some(value) = object.get("lng")
        && let Some(parsed_value) = parse_required_f64("$.lng", value, &mut details)
    {
        parsed.update.lng = Some(parsed_value);
    }

    if let Some(value) = object.get("region")
        && let Some(parsed_value) = parse_nullable_string("$.region", value, &mut details)
    {
        parsed.update.region = Some(parsed_value);
    }

    if let Some(value) = object.get("epcRating")
        && let Some(parsed_value) = parse_nullable_epc_rating("$.epcRating", value, &mut details)
    {
        parsed.update.epc_rating = Some(parsed_value);
    }

    if let Some(value) = object.get("floorArea")
        && let Some(parsed_value) = parse_nullable_f64("$.floorArea", value, &mut details)
    {
        parsed.update.floor_area = Some(parsed_value);
    }

    if let Some(value) = object.get("gigabitAvailability")
        && let Some(parsed_value) = parse_nullable_f64("$.gigabitAvailability", value, &mut details)
    {
        parsed.update.gigabit_availability = Some(parsed_value);
    }
    if let Some(value) = object.get("crimeRatePer1k")
        && let Some(parsed_value) = parse_nullable_f64("$.crimeRatePer1k", value, &mut details)
    {
        parsed.update.crime_rate_per_1k = Some(parsed_value);
    }
    if let Some(value) = object.get("crimeCount12m")
        && let Some(parsed_value) = parse_nullable_i64("$.crimeCount12m", value, &mut details)
    {
        parsed.update.crime_count_12m = Some(parsed_value);
    }
    if let Some(value) = object.get("crimeViolent12m")
        && let Some(parsed_value) = parse_nullable_i64("$.crimeViolent12m", value, &mut details)
    {
        parsed.update.crime_violent_12m = Some(parsed_value);
    }
    if let Some(value) = object.get("crimeBurglary12m")
        && let Some(parsed_value) = parse_nullable_i64("$.crimeBurglary12m", value, &mut details)
    {
        parsed.update.crime_burglary_12m = Some(parsed_value);
    }
    if let Some(value) = object.get("crimeRobbery12m")
        && let Some(parsed_value) = parse_nullable_i64("$.crimeRobbery12m", value, &mut details)
    {
        parsed.update.crime_robbery_12m = Some(parsed_value);
    }

    if let Some(value) = object.get("imdDecile")
        && let Some(parsed_value) = parse_nullable_i64("$.imdDecile", value, &mut details)
    {
        parsed.update.imd_decile = Some(parsed_value);
    }
    if let Some(value) = object.get("imdRank")
        && let Some(parsed_value) = parse_nullable_i64("$.imdRank", value, &mut details)
    {
        parsed.update.imd_rank = Some(parsed_value);
    }
    if let Some(value) = object.get("imdScore")
        && let Some(parsed_value) = parse_nullable_f64("$.imdScore", value, &mut details)
    {
        parsed.update.imd_score = Some(parsed_value);
    }

    if let Some(value) = object.get("lsoaCode")
        && let Some(parsed_value) = parse_nullable_string("$.lsoaCode", value, &mut details)
    {
        parsed.update.lsoa_code = Some(parsed_value);
    }
    if let Some(value) = object.get("lsoaName")
        && let Some(parsed_value) = parse_nullable_string("$.lsoaName", value, &mut details)
    {
        parsed.update.lsoa_name = Some(parsed_value);
    }
    if let Some(value) = object.get("msoaCode")
        && let Some(parsed_value) = parse_nullable_string("$.msoaCode", value, &mut details)
    {
        parsed.update.msoa_code = Some(parsed_value);
    }
    if let Some(value) = object.get("msoaName")
        && let Some(parsed_value) = parse_nullable_string("$.msoaName", value, &mut details)
    {
        parsed.update.msoa_name = Some(parsed_value);
    }

    if let Some(value) = object.get("incomeBhc")
        && let Some(parsed_value) = parse_nullable_f64("$.incomeBhc", value, &mut details)
    {
        parsed.update.income_bhc = Some(parsed_value);
    }
    if let Some(value) = object.get("incomeAhc")
        && let Some(parsed_value) = parse_nullable_f64("$.incomeAhc", value, &mut details)
    {
        parsed.update.income_ahc = Some(parsed_value);
    }
    if let Some(value) = object.get("socialHousingPct")
        && let Some(parsed_value) = parse_nullable_f64("$.socialHousingPct", value, &mut details)
    {
        parsed.update.social_housing_pct = Some(parsed_value);
    }
    if let Some(value) = object.get("population")
        && let Some(parsed_value) = parse_nullable_i64("$.population", value, &mut details)
    {
        parsed.update.population = Some(parsed_value);
    }

    if let Some(value) = object.get("floodRiskLevel")
        && let Some(parsed_value) = parse_nullable_string("$.floodRiskLevel", value, &mut details)
    {
        parsed.update.flood_risk_level = Some(parsed_value);
    }
    if let Some(value) = object.get("floodRiskSource")
        && let Some(parsed_value) = parse_nullable_string("$.floodRiskSource", value, &mut details)
    {
        parsed.update.flood_risk_source = Some(parsed_value);
    }

    if let Some(value) = object.get("crimeBand")
        && let Some(parsed_value) = parse_nullable_crime_band("$.crimeBand", value, &mut details)
    {
        parsed.update.crime_band = Some(parsed_value);
    }
    if let Some(value) = object.get("crimeTrend")
        && let Some(parsed_value) = parse_nullable_crime_trend("$.crimeTrend", value, &mut details)
    {
        parsed.update.crime_trend = Some(parsed_value);
    }
    if let Some(value) = object.get("crimeUpdatedAt")
        && let Some(parsed_value) = parse_nullable_string("$.crimeUpdatedAt", value, &mut details)
    {
        parsed.update.crime_updated_at = Some(parsed_value);
    }

    if !details.is_empty() {
        return Err(CommandError::runtime(
            "PATCH_JSON_VALIDATION_ERROR",
            "invalid --patch-json payload",
            "inspect `error.details` and correct the patch fields",
        )
        .with_details(details));
    }

    Ok(parsed)
}

fn parse_area_patch(value: &Value, update: &mut PatchUpdate, details: &mut Vec<ErrorDetail>) {
    let path = "$.area";
    let Some(area_object) = value.as_object() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            "area must be an object".to_owned(),
            Some("object".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return;
    };

    const AREA_FIELDS: &[&str] = &[
        "lsoa",
        "msoa",
        "imd",
        "income",
        "socialHousingPct",
        "population",
        "floodRisk",
        "crime",
    ];
    for key in area_object.keys() {
        if !AREA_FIELDS.contains(&key.as_str()) {
            details.push(error_detail(
                format!("{path}.{key}"),
                "unknown_field",
                format!("unknown area field `{key}`"),
                Some("lsoa,msoa,imd,income,socialHousingPct,population,floodRisk,crime".to_owned()),
                None,
                None,
            ));
        }
    }

    if let Some(value) = area_object.get("socialHousingPct")
        && let Some(parsed_value) = parse_nullable_f64("$.area.socialHousingPct", value, details)
    {
        update.social_housing_pct = Some(parsed_value);
    }

    if let Some(value) = area_object.get("population")
        && let Some(parsed_value) = parse_nullable_i64("$.area.population", value, details)
    {
        update.population = Some(parsed_value);
    }

    if let Some(value) = area_object.get("lsoa") {
        parse_area_code_patch(
            "$.area.lsoa",
            value,
            details,
            &mut update.lsoa_code,
            &mut update.lsoa_name,
        );
    }
    if let Some(value) = area_object.get("msoa") {
        parse_area_code_patch(
            "$.area.msoa",
            value,
            details,
            &mut update.msoa_code,
            &mut update.msoa_name,
        );
    }
    if let Some(value) = area_object.get("imd") {
        parse_imd_patch(value, update, details);
    }
    if let Some(value) = area_object.get("income") {
        parse_income_patch(value, update, details);
    }
    if let Some(value) = area_object.get("floodRisk") {
        parse_flood_risk_patch(value, update, details);
    }
    if let Some(value) = area_object.get("crime") {
        parse_crime_patch(value, update, details);
    }
}

fn parse_area_code_patch(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
    code_target: &mut Option<Option<String>>,
    name_target: &mut Option<Option<String>>,
) {
    let Some(object) = value.as_object() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            format!("{path} must be an object"),
            Some("object".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return;
    };

    const FIELDS: &[&str] = &["code", "name"];
    for key in object.keys() {
        if !FIELDS.contains(&key.as_str()) {
            details.push(error_detail(
                format!("{path}.{key}"),
                "unknown_field",
                format!("unknown field `{key}` in {path}"),
                Some("code,name".to_owned()),
                None,
                None,
            ));
        }
    }

    if let Some(value) = object.get("code")
        && let Some(parsed_value) = parse_nullable_string(&format!("{path}.code"), value, details)
    {
        *code_target = Some(parsed_value);
    }
    if let Some(value) = object.get("name")
        && let Some(parsed_value) = parse_nullable_string(&format!("{path}.name"), value, details)
    {
        *name_target = Some(parsed_value);
    }
}

fn parse_imd_patch(value: &Value, update: &mut PatchUpdate, details: &mut Vec<ErrorDetail>) {
    let path = "$.area.imd";
    let Some(object) = value.as_object() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            "area.imd must be an object".to_owned(),
            Some("object".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return;
    };

    const FIELDS: &[&str] = &["decile", "rank", "score"];
    for key in object.keys() {
        if !FIELDS.contains(&key.as_str()) {
            details.push(error_detail(
                format!("{path}.{key}"),
                "unknown_field",
                format!("unknown field `{key}` in area.imd"),
                Some("decile,rank,score".to_owned()),
                None,
                None,
            ));
        }
    }

    if let Some(value) = object.get("decile")
        && let Some(parsed_value) = parse_nullable_i64("$.area.imd.decile", value, details)
    {
        update.imd_decile = Some(parsed_value);
    }
    if let Some(value) = object.get("rank")
        && let Some(parsed_value) = parse_nullable_i64("$.area.imd.rank", value, details)
    {
        update.imd_rank = Some(parsed_value);
    }
    if let Some(value) = object.get("score")
        && let Some(parsed_value) = parse_nullable_f64("$.area.imd.score", value, details)
    {
        update.imd_score = Some(parsed_value);
    }
}

fn parse_income_patch(value: &Value, update: &mut PatchUpdate, details: &mut Vec<ErrorDetail>) {
    let path = "$.area.income";
    let Some(object) = value.as_object() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            "area.income must be an object".to_owned(),
            Some("object".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return;
    };

    const FIELDS: &[&str] = &["bhc", "ahc"];
    for key in object.keys() {
        if !FIELDS.contains(&key.as_str()) {
            details.push(error_detail(
                format!("{path}.{key}"),
                "unknown_field",
                format!("unknown field `{key}` in area.income"),
                Some("bhc,ahc".to_owned()),
                None,
                None,
            ));
        }
    }

    if let Some(value) = object.get("bhc")
        && let Some(parsed_value) = parse_nullable_f64("$.area.income.bhc", value, details)
    {
        update.income_bhc = Some(parsed_value);
    }
    if let Some(value) = object.get("ahc")
        && let Some(parsed_value) = parse_nullable_f64("$.area.income.ahc", value, details)
    {
        update.income_ahc = Some(parsed_value);
    }
}

fn parse_flood_risk_patch(value: &Value, update: &mut PatchUpdate, details: &mut Vec<ErrorDetail>) {
    let path = "$.area.floodRisk";
    let Some(object) = value.as_object() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            "area.floodRisk must be an object".to_owned(),
            Some("object".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return;
    };

    const FIELDS: &[&str] = &["level", "source"];
    for key in object.keys() {
        if !FIELDS.contains(&key.as_str()) {
            details.push(error_detail(
                format!("{path}.{key}"),
                "unknown_field",
                format!("unknown field `{key}` in area.floodRisk"),
                Some("level,source".to_owned()),
                None,
                None,
            ));
        }
    }

    if let Some(value) = object.get("level")
        && let Some(parsed_value) = parse_nullable_string("$.area.floodRisk.level", value, details)
    {
        update.flood_risk_level = Some(parsed_value);
    }
    if let Some(value) = object.get("source")
        && let Some(parsed_value) = parse_nullable_string("$.area.floodRisk.source", value, details)
    {
        update.flood_risk_source = Some(parsed_value);
    }
}

fn parse_crime_patch(value: &Value, update: &mut PatchUpdate, details: &mut Vec<ErrorDetail>) {
    let path = "$.area.crime";
    let Some(object) = value.as_object() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            "area.crime must be an object".to_owned(),
            Some("object".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return;
    };

    const FIELDS: &[&str] = &[
        "ratePer1k",
        "count12m",
        "violent12m",
        "burglary12m",
        "robbery12m",
        "band",
        "trend",
        "updatedAt",
    ];
    for key in object.keys() {
        if !FIELDS.contains(&key.as_str()) {
            details.push(error_detail(
                format!("{path}.{key}"),
                "unknown_field",
                format!("unknown field `{key}` in area.crime"),
                Some(
                    "ratePer1k,count12m,violent12m,burglary12m,robbery12m,band,trend,updatedAt"
                        .to_owned(),
                ),
                None,
                None,
            ));
        }
    }

    if let Some(value) = object.get("ratePer1k")
        && let Some(parsed_value) = parse_nullable_f64("$.area.crime.ratePer1k", value, details)
    {
        update.crime_rate_per_1k = Some(parsed_value);
    }
    if let Some(value) = object.get("count12m")
        && let Some(parsed_value) = parse_nullable_i64("$.area.crime.count12m", value, details)
    {
        update.crime_count_12m = Some(parsed_value);
    }
    if let Some(value) = object.get("violent12m")
        && let Some(parsed_value) = parse_nullable_i64("$.area.crime.violent12m", value, details)
    {
        update.crime_violent_12m = Some(parsed_value);
    }
    if let Some(value) = object.get("burglary12m")
        && let Some(parsed_value) = parse_nullable_i64("$.area.crime.burglary12m", value, details)
    {
        update.crime_burglary_12m = Some(parsed_value);
    }
    if let Some(value) = object.get("robbery12m")
        && let Some(parsed_value) = parse_nullable_i64("$.area.crime.robbery12m", value, details)
    {
        update.crime_robbery_12m = Some(parsed_value);
    }
    if let Some(value) = object.get("band")
        && let Some(parsed_value) = parse_nullable_crime_band("$.area.crime.band", value, details)
    {
        update.crime_band = Some(parsed_value);
    }
    if let Some(value) = object.get("trend")
        && let Some(parsed_value) = parse_nullable_crime_trend("$.area.crime.trend", value, details)
    {
        update.crime_trend = Some(parsed_value);
    }
    if let Some(value) = object.get("updatedAt")
        && let Some(parsed_value) = parse_nullable_string("$.area.crime.updatedAt", value, details)
    {
        update.crime_updated_at = Some(parsed_value);
    }
}

fn parse_required_string(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
) -> Option<String> {
    if value.is_null() {
        details.push(error_detail(
            path.to_owned(),
            "null_not_allowed",
            format!("{path} cannot be null"),
            Some("string".to_owned()),
            Some("null".to_owned()),
            Some("provide a string value".to_owned()),
        ));
        return None;
    }

    match value.as_str() {
        Some(text) => Some(text.to_owned()),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_type",
                format!("{path} must be a string"),
                Some("string".to_owned()),
                Some(value_type_name(value).to_owned()),
                None,
            ));
            None
        }
    }
}

fn parse_required_f64(path: &str, value: &Value, details: &mut Vec<ErrorDetail>) -> Option<f64> {
    if value.is_null() {
        details.push(error_detail(
            path.to_owned(),
            "null_not_allowed",
            format!("{path} cannot be null"),
            Some("number".to_owned()),
            Some("null".to_owned()),
            Some("provide a numeric value".to_owned()),
        ));
        return None;
    }

    match value.as_f64() {
        Some(number) => Some(number),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_type",
                format!("{path} must be a number"),
                Some("number".to_owned()),
                Some(value_type_name(value).to_owned()),
                None,
            ));
            None
        }
    }
}

fn parse_nullable_string(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
) -> Option<Option<String>> {
    if value.is_null() {
        return Some(None);
    }

    match value.as_str() {
        Some(text) => Some(Some(text.to_owned())),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_type",
                format!("{path} must be a string or null"),
                Some("string|null".to_owned()),
                Some(value_type_name(value).to_owned()),
                None,
            ));
            None
        }
    }
}

fn parse_nullable_f64(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
) -> Option<Option<f64>> {
    if value.is_null() {
        return Some(None);
    }

    match value.as_f64() {
        Some(number) => Some(Some(number)),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_type",
                format!("{path} must be a number or null"),
                Some("number|null".to_owned()),
                Some(value_type_name(value).to_owned()),
                None,
            ));
            None
        }
    }
}

fn parse_nullable_i64(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
) -> Option<Option<i64>> {
    if value.is_null() {
        return Some(None);
    }

    match value.as_i64() {
        Some(number) => Some(Some(number)),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_type",
                format!("{path} must be an integer or null"),
                Some("integer|null".to_owned()),
                Some(value_type_name(value).to_owned()),
                None,
            ));
            None
        }
    }
}

fn parse_nullable_epc_rating(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
) -> Option<Option<EpcBand>> {
    if value.is_null() {
        return Some(None);
    }

    let Some(raw) = value.as_str() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            format!("{path} must be a string or null"),
            Some("string|null".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return None;
    };

    match parse_epc_band(raw) {
        Some(parsed) => Some(Some(parsed)),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_enum",
                format!("invalid EPC rating `{raw}`"),
                Some("A|B|C|D|E|F|G".to_owned()),
                Some(raw.to_owned()),
                None,
            ));
            None
        }
    }
}

fn parse_nullable_crime_band(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
) -> Option<Option<CrimeBand>> {
    if value.is_null() {
        return Some(None);
    }

    let Some(raw) = value.as_str() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            format!("{path} must be a string or null"),
            Some("string|null".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return None;
    };

    match parse_crime_band(raw) {
        Some(parsed) => Some(Some(parsed)),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_enum",
                format!("invalid crime band `{raw}`"),
                Some("excellent|good|mixed|concerning".to_owned()),
                Some(raw.to_owned()),
                None,
            ));
            None
        }
    }
}

fn parse_nullable_crime_trend(
    path: &str,
    value: &Value,
    details: &mut Vec<ErrorDetail>,
) -> Option<Option<CrimeTrend>> {
    if value.is_null() {
        return Some(None);
    }

    let Some(raw) = value.as_str() else {
        details.push(error_detail(
            path.to_owned(),
            "invalid_type",
            format!("{path} must be a string or null"),
            Some("string|null".to_owned()),
            Some(value_type_name(value).to_owned()),
            None,
        ));
        return None;
    };

    match parse_crime_trend(raw) {
        Some(parsed) => Some(Some(parsed)),
        None => {
            details.push(error_detail(
                path.to_owned(),
                "invalid_enum",
                format!("invalid crime trend `{raw}`"),
                Some("improving|stable|worsening".to_owned()),
                Some(raw.to_owned()),
                None,
            ));
            None
        }
    }
}

fn apply_scalar_overrides(
    params: &PatchParams,
    update: &mut PatchUpdate,
    warnings: &mut Vec<String>,
) -> Result<(), CommandError> {
    if let Some(address) = params.address.as_ref() {
        overwrite_required_field("address", update.address.is_some(), warnings);
        update.address = Some(address.trim().to_owned());
    }
    if let Some(postcode) = params.postcode.as_ref() {
        overwrite_required_field("postcode", update.postcode.is_some(), warnings);
        update.postcode = Some(canonicalize_postcode(postcode));
    }
    if let Some(lat) = params.lat {
        overwrite_required_field("lat", update.lat.is_some(), warnings);
        update.lat = Some(lat);
    }
    if let Some(lng) = params.lng {
        overwrite_required_field("lng", update.lng.is_some(), warnings);
        update.lng = Some(lng);
    }
    if let Some(region) = params.region.as_ref() {
        overwrite_nullable_field("region", update.region.is_some(), warnings);
        update.region = Some(Some(region.trim().to_owned()));
    }
    if let Some(epc_rating) = params.epc_rating.as_ref() {
        let parsed = parse_epc_rating(epc_rating)?;
        overwrite_nullable_field("epcRating", update.epc_rating.is_some(), warnings);
        update.epc_rating = Some(Some(parsed));
    }
    if let Some(floor_area) = params.floor_area {
        overwrite_nullable_field("floorArea", update.floor_area.is_some(), warnings);
        update.floor_area = Some(Some(floor_area));
    }
    if let Some(value) = params.gigabit_availability {
        overwrite_nullable_field(
            "gigabitAvailability",
            update.gigabit_availability.is_some(),
            warnings,
        );
        update.gigabit_availability = Some(Some(value));
    }
    if let Some(value) = params.crime_rate_per_1k {
        overwrite_nullable_field(
            "crimeRatePer1k",
            update.crime_rate_per_1k.is_some(),
            warnings,
        );
        update.crime_rate_per_1k = Some(Some(value));
    }
    if let Some(value) = params.crime_count_12m {
        overwrite_nullable_field("crimeCount12m", update.crime_count_12m.is_some(), warnings);
        update.crime_count_12m = Some(Some(value));
    }
    if let Some(value) = params.crime_violent_12m {
        overwrite_nullable_field(
            "crimeViolent12m",
            update.crime_violent_12m.is_some(),
            warnings,
        );
        update.crime_violent_12m = Some(Some(value));
    }
    if let Some(value) = params.crime_burglary_12m {
        overwrite_nullable_field(
            "crimeBurglary12m",
            update.crime_burglary_12m.is_some(),
            warnings,
        );
        update.crime_burglary_12m = Some(Some(value));
    }
    if let Some(value) = params.crime_robbery_12m {
        overwrite_nullable_field(
            "crimeRobbery12m",
            update.crime_robbery_12m.is_some(),
            warnings,
        );
        update.crime_robbery_12m = Some(Some(value));
    }
    if let Some(value) = params.imd_decile {
        overwrite_nullable_field("imdDecile", update.imd_decile.is_some(), warnings);
        update.imd_decile = Some(Some(value));
    }
    if let Some(value) = params.imd_rank {
        overwrite_nullable_field("imdRank", update.imd_rank.is_some(), warnings);
        update.imd_rank = Some(Some(value));
    }
    if let Some(value) = params.imd_score {
        overwrite_nullable_field("imdScore", update.imd_score.is_some(), warnings);
        update.imd_score = Some(Some(value));
    }
    if let Some(value) = params.lsoa_code.as_ref() {
        overwrite_nullable_field("lsoaCode", update.lsoa_code.is_some(), warnings);
        update.lsoa_code = Some(Some(value.trim().to_owned()));
    }
    if let Some(value) = params.lsoa_name.as_ref() {
        overwrite_nullable_field("lsoaName", update.lsoa_name.is_some(), warnings);
        update.lsoa_name = Some(Some(value.trim().to_owned()));
    }
    if let Some(value) = params.msoa_code.as_ref() {
        overwrite_nullable_field("msoaCode", update.msoa_code.is_some(), warnings);
        update.msoa_code = Some(Some(value.trim().to_owned()));
    }
    if let Some(value) = params.msoa_name.as_ref() {
        overwrite_nullable_field("msoaName", update.msoa_name.is_some(), warnings);
        update.msoa_name = Some(Some(value.trim().to_owned()));
    }
    if let Some(value) = params.income_bhc {
        overwrite_nullable_field("incomeBhc", update.income_bhc.is_some(), warnings);
        update.income_bhc = Some(Some(value));
    }
    if let Some(value) = params.income_ahc {
        overwrite_nullable_field("incomeAhc", update.income_ahc.is_some(), warnings);
        update.income_ahc = Some(Some(value));
    }
    if let Some(value) = params.social_housing_pct {
        overwrite_nullable_field(
            "socialHousingPct",
            update.social_housing_pct.is_some(),
            warnings,
        );
        update.social_housing_pct = Some(Some(value));
    }
    if let Some(value) = params.population {
        overwrite_nullable_field("population", update.population.is_some(), warnings);
        update.population = Some(Some(value));
    }
    if let Some(value) = params.flood_risk_level.as_ref() {
        overwrite_nullable_field(
            "floodRiskLevel",
            update.flood_risk_level.is_some(),
            warnings,
        );
        update.flood_risk_level = Some(Some(value.trim().to_owned()));
    }
    if let Some(value) = params.flood_risk_source.as_ref() {
        overwrite_nullable_field(
            "floodRiskSource",
            update.flood_risk_source.is_some(),
            warnings,
        );
        update.flood_risk_source = Some(Some(value.trim().to_owned()));
    }
    if let Some(value) = params.crime_band.as_ref() {
        let Some(parsed) = parse_crime_band(value) else {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                format!("invalid crime band: {value}"),
                "crime band must be one of excellent|good|mixed|concerning",
            ));
        };
        overwrite_nullable_field("crimeBand", update.crime_band.is_some(), warnings);
        update.crime_band = Some(Some(parsed));
    }
    if let Some(value) = params.crime_trend.as_ref() {
        let Some(parsed) = parse_crime_trend(value) else {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                format!("invalid crime trend: {value}"),
                "crime trend must be one of improving|stable|worsening",
            ));
        };
        overwrite_nullable_field("crimeTrend", update.crime_trend.is_some(), warnings);
        update.crime_trend = Some(Some(parsed));
    }
    if let Some(value) = params.crime_updated_at.as_ref() {
        overwrite_nullable_field(
            "crimeUpdatedAt",
            update.crime_updated_at.is_some(),
            warnings,
        );
        update.crime_updated_at = Some(Some(value.trim().to_owned()));
    }

    Ok(())
}

fn overwrite_required_field(field: &str, already_present: bool, warnings: &mut Vec<String>) {
    if already_present {
        warnings.push(format!(
            "scalar `{field}` override replaced value provided in --patch-json"
        ));
    }
}

fn overwrite_nullable_field(field: &str, already_present: bool, warnings: &mut Vec<String>) {
    if already_present {
        warnings.push(format!(
            "scalar `{field}` override replaced value provided in --patch-json"
        ));
    }
}

fn has_any_patch(update: &PatchUpdate) -> bool {
    update.address.is_some()
        || update.postcode.is_some()
        || update.lat.is_some()
        || update.lng.is_some()
        || update.region.is_some()
        || update.epc_rating.is_some()
        || update.floor_area.is_some()
        || update.gigabit_availability.is_some()
        || update.crime_rate_per_1k.is_some()
        || update.crime_count_12m.is_some()
        || update.crime_violent_12m.is_some()
        || update.crime_burglary_12m.is_some()
        || update.crime_robbery_12m.is_some()
        || update.imd_decile.is_some()
        || update.imd_rank.is_some()
        || update.imd_score.is_some()
        || update.lsoa_code.is_some()
        || update.lsoa_name.is_some()
        || update.msoa_code.is_some()
        || update.msoa_name.is_some()
        || update.income_bhc.is_some()
        || update.income_ahc.is_some()
        || update.social_housing_pct.is_some()
        || update.population.is_some()
        || update.flood_risk_level.is_some()
        || update.flood_risk_source.is_some()
        || update.crime_band.is_some()
        || update.crime_trend.is_some()
        || update.crime_updated_at.is_some()
}

fn validate_patch_update(update: &PatchUpdate) -> Vec<ErrorDetail> {
    let mut details = Vec::new();

    if update.lat.is_some() ^ update.lng.is_some() {
        details.push(error_detail(
            "$.lat/$.lng".to_owned(),
            "missing_pair",
            "lat and lng must be provided together".to_owned(),
            Some("both lat and lng".to_owned()),
            None,
            Some("set both coordinates or remove both".to_owned()),
        ));
    }

    if let Some(lat) = update.lat
        && !(-90.0..=90.0).contains(&lat)
    {
        details.push(range_error("$.lat", "-90..90", lat));
    }

    if let Some(lng) = update.lng
        && !(-180.0..=180.0).contains(&lng)
    {
        details.push(range_error("$.lng", "-180..180", lng));
    }

    if let Some(Some(value)) = update.floor_area
        && value <= 0.0
    {
        details.push(range_error("$.floorArea", "> 0", value));
    }
    if let Some(Some(value)) = update.gigabit_availability
        && !(0.0..=100.0).contains(&value)
    {
        details.push(range_error("$.gigabitAvailability", "0..100", value));
    }
    if let Some(Some(value)) = update.crime_rate_per_1k
        && value < 0.0
    {
        details.push(range_error("$.crimeRatePer1k", ">= 0", value));
    }
    if let Some(Some(value)) = update.imd_decile
        && !(1..=10).contains(&value)
    {
        details.push(range_error("$.imdDecile", "1..10", value));
    }
    if let Some(Some(value)) = update.imd_rank
        && value <= 0
    {
        details.push(range_error("$.imdRank", "> 0", value));
    }
    if let Some(Some(value)) = update.imd_score
        && value < 0.0
    {
        details.push(range_error("$.imdScore", ">= 0", value));
    }
    if let Some(Some(value)) = update.income_bhc
        && value < 0.0
    {
        details.push(range_error("$.incomeBhc", ">= 0", value));
    }
    if let Some(Some(value)) = update.income_ahc
        && value < 0.0
    {
        details.push(range_error("$.incomeAhc", ">= 0", value));
    }
    if let Some(Some(value)) = update.social_housing_pct
        && !(0.0..=100.0).contains(&value)
    {
        details.push(range_error("$.socialHousingPct", "0..100", value));
    }
    if let Some(Some(value)) = update.population
        && value < 0
    {
        details.push(range_error("$.population", ">= 0", value));
    }
    if let Some(Some(value)) = update.crime_count_12m
        && value < 0
    {
        details.push(range_error("$.crimeCount12m", ">= 0", value));
    }
    if let Some(Some(value)) = update.crime_violent_12m
        && value < 0
    {
        details.push(range_error("$.crimeViolent12m", ">= 0", value));
    }
    if let Some(Some(value)) = update.crime_burglary_12m
        && value < 0
    {
        details.push(range_error("$.crimeBurglary12m", ">= 0", value));
    }
    if let Some(Some(value)) = update.crime_robbery_12m
        && value < 0
    {
        details.push(range_error("$.crimeRobbery12m", ">= 0", value));
    }

    if let Some(address) = update.address.as_ref()
        && address.trim().is_empty()
    {
        details.push(error_detail(
            "$.address".to_owned(),
            "empty_string",
            "address cannot be empty".to_owned(),
            Some("non-empty string".to_owned()),
            Some("empty string".to_owned()),
            None,
        ));
    }
    if let Some(postcode) = update.postcode.as_ref()
        && postcode.trim().is_empty()
    {
        details.push(error_detail(
            "$.postcode".to_owned(),
            "empty_string",
            "postcode cannot be empty".to_owned(),
            Some("non-empty string".to_owned()),
            Some("empty string".to_owned()),
            None,
        ));
    }
    validate_nullable_non_empty("$.region", update.region.as_ref(), &mut details);
    validate_nullable_non_empty("$.lsoaCode", update.lsoa_code.as_ref(), &mut details);
    validate_nullable_non_empty("$.lsoaName", update.lsoa_name.as_ref(), &mut details);
    validate_nullable_non_empty("$.msoaCode", update.msoa_code.as_ref(), &mut details);
    validate_nullable_non_empty("$.msoaName", update.msoa_name.as_ref(), &mut details);
    validate_nullable_non_empty(
        "$.floodRiskLevel",
        update.flood_risk_level.as_ref(),
        &mut details,
    );
    validate_nullable_non_empty(
        "$.floodRiskSource",
        update.flood_risk_source.as_ref(),
        &mut details,
    );
    validate_nullable_non_empty(
        "$.crimeUpdatedAt",
        update.crime_updated_at.as_ref(),
        &mut details,
    );

    if let Some(Some(date_raw)) = update.crime_updated_at.as_ref()
        && NaiveDate::parse_from_str(date_raw, "%Y-%m-%d").is_err()
    {
        details.push(error_detail(
            "$.crimeUpdatedAt".to_owned(),
            "invalid_date",
            "crimeUpdatedAt must be in YYYY-MM-DD format".to_owned(),
            Some("YYYY-MM-DD".to_owned()),
            Some(date_raw.clone()),
            None,
        ));
    }

    details
}

fn validate_nullable_non_empty(
    path: &str,
    value: Option<&Option<String>>,
    details: &mut Vec<ErrorDetail>,
) {
    if let Some(Some(text)) = value
        && text.trim().is_empty()
    {
        details.push(error_detail(
            path.to_owned(),
            "empty_string",
            format!("{path} cannot be empty"),
            Some("non-empty string or null".to_owned()),
            Some("empty string".to_owned()),
            Some("set null to clear this field".to_owned()),
        ));
    }
}

fn apply_update_to_listing(
    listing: &mut Listing,
    update: &PatchUpdate,
    applied: &mut Map<String, Value>,
) {
    apply_required_field(applied, "address", &mut listing.address, &update.address);
    apply_required_field(applied, "postcode", &mut listing.postcode, &update.postcode);
    apply_required_field(applied, "lat", &mut listing.location.lat, &update.lat);
    apply_required_field(applied, "lng", &mut listing.location.lng, &update.lng);

    apply_nullable_field(applied, "region", &mut listing.region, &update.region);
    apply_nullable_field(
        applied,
        "epcRating",
        &mut listing.epc_rating,
        &update.epc_rating,
    );
    apply_nullable_field(
        applied,
        "floorArea",
        &mut listing.floor_area_sqm,
        &update.floor_area,
    );
    apply_nullable_field(
        applied,
        "gigabitAvailability",
        &mut listing.gigabit_availability,
        &update.gigabit_availability,
    );
    apply_nullable_field(
        applied,
        "crimeRatePer1k",
        &mut listing.area.crime.rate_per_1k,
        &update.crime_rate_per_1k,
    );
    apply_nullable_field(
        applied,
        "crimeCount12m",
        &mut listing.area.crime.count_12m,
        &update.crime_count_12m,
    );
    apply_nullable_field(
        applied,
        "crimeViolent12m",
        &mut listing.area.crime.violent_12m,
        &update.crime_violent_12m,
    );
    apply_nullable_field(
        applied,
        "crimeBurglary12m",
        &mut listing.area.crime.burglary_12m,
        &update.crime_burglary_12m,
    );
    apply_nullable_field(
        applied,
        "crimeRobbery12m",
        &mut listing.area.crime.robbery_12m,
        &update.crime_robbery_12m,
    );
    apply_nullable_field(
        applied,
        "imdDecile",
        &mut listing.area.imd.decile,
        &update.imd_decile,
    );
    apply_nullable_field(
        applied,
        "imdRank",
        &mut listing.area.imd.rank,
        &update.imd_rank,
    );
    apply_nullable_field(
        applied,
        "imdScore",
        &mut listing.area.imd.score,
        &update.imd_score,
    );
    apply_nullable_field(
        applied,
        "lsoaCode",
        &mut listing.area.lsoa.code,
        &update.lsoa_code,
    );
    apply_nullable_field(
        applied,
        "lsoaName",
        &mut listing.area.lsoa.name,
        &update.lsoa_name,
    );
    apply_nullable_field(
        applied,
        "msoaCode",
        &mut listing.area.msoa.code,
        &update.msoa_code,
    );
    apply_nullable_field(
        applied,
        "msoaName",
        &mut listing.area.msoa.name,
        &update.msoa_name,
    );
    apply_nullable_field(
        applied,
        "incomeBhc",
        &mut listing.area.income.bhc,
        &update.income_bhc,
    );
    apply_nullable_field(
        applied,
        "incomeAhc",
        &mut listing.area.income.ahc,
        &update.income_ahc,
    );
    apply_nullable_field(
        applied,
        "socialHousingPct",
        &mut listing.area.social_housing_pct,
        &update.social_housing_pct,
    );
    apply_nullable_field(
        applied,
        "population",
        &mut listing.area.population,
        &update.population,
    );
    apply_nullable_field(
        applied,
        "floodRiskLevel",
        &mut listing.area.flood_risk.level,
        &update.flood_risk_level,
    );
    apply_nullable_field(
        applied,
        "floodRiskSource",
        &mut listing.area.flood_risk.source,
        &update.flood_risk_source,
    );
    apply_nullable_field(
        applied,
        "crimeBand",
        &mut listing.area.crime.band,
        &update.crime_band,
    );
    apply_nullable_field(
        applied,
        "crimeTrend",
        &mut listing.area.crime.trend,
        &update.crime_trend,
    );
    apply_nullable_field(
        applied,
        "crimeUpdatedAt",
        &mut listing.area.crime.updated_at,
        &update.crime_updated_at,
    );

    if applied.contains_key("address")
        || applied.contains_key("postcode")
        || applied.contains_key("lat")
        || applied.contains_key("lng")
    {
        listing.google_maps_url = build_google_maps_url(
            listing.location.lat,
            listing.location.lng,
            &listing.address,
            &listing.postcode,
        );
        listing.google_maps_street_view_url =
            build_google_maps_street_view_url(listing.location.lat, listing.location.lng);
        listing.epc_search_url = if listing.postcode.is_empty() {
            None
        } else {
            let encoded_postcode =
                url::form_urlencoded::byte_serialize(listing.postcode.as_bytes())
                    .collect::<String>();
            Some(format!(
                "https://find-energy-certificate.service.gov.uk/find-a-certificate/search-by-postcode?postcode={}",
                encoded_postcode
            ))
        };
    }
}

fn apply_required_field<T>(
    applied: &mut Map<String, Value>,
    key: &str,
    current: &mut T,
    incoming: &Option<T>,
) where
    T: Clone + PartialEq + serde::Serialize,
{
    let Some(next) = incoming else {
        return;
    };
    if *current == *next {
        return;
    }
    applied.insert(
        key.to_owned(),
        json!({
            "from": current,
            "to": next,
        }),
    );
    *current = next.clone();
}

fn apply_nullable_field<T>(
    applied: &mut Map<String, Value>,
    key: &str,
    current: &mut Option<T>,
    incoming: &Option<Option<T>>,
) where
    T: Clone + PartialEq + serde::Serialize,
{
    let Some(next) = incoming else {
        return;
    };
    if *current == *next {
        return;
    }
    applied.insert(
        key.to_owned(),
        json!({
            "from": current,
            "to": next,
        }),
    );
    *current = next.clone();
}

fn error_detail(
    path: String,
    code: &str,
    message: String,
    expected: Option<String>,
    actual: Option<String>,
    suggestion: Option<String>,
) -> ErrorDetail {
    ErrorDetail {
        path,
        code: code.to_owned(),
        message,
        expected,
        actual,
        suggestion,
    }
}

fn range_error<T>(path: &str, expected: &str, actual: T) -> ErrorDetail
where
    T: ToString,
{
    error_detail(
        path.to_owned(),
        "out_of_range",
        format!("{path} value is out of range"),
        Some(expected.to_owned()),
        Some(actual.to_string()),
        None,
    )
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn canonicalize_postcode(raw: &str) -> String {
    let compact = let_sdk::utils::text::normalize_postcode(raw);
    if compact.len() <= 3 {
        return compact;
    }
    let split = compact.len() - 3;
    format!("{} {}", &compact[..split], &compact[split..])
}

pub fn verify(shared: &SharedArgs, params: &VerifyParams) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;
    let data = load_listings_file(&db_path)?;

    if data.listings.is_empty() {
        return Ok(CommandOutput::new(json!({
            "checked": 0,
            "active": 0,
            "inactive": 0,
            "errors": 0,
            "dryRun": params.dry_run,
            "results": [],
        }))
        .with_text("no listings to verify"));
    }

    let patterns = params
        .region
        .as_deref()
        .map(region_patterns)
        .unwrap_or_default();
    let mut targets = data
        .listings
        .iter()
        .filter(|listing| !matches!(listing.status, ListingStatus::Inactive))
        .filter(|listing| {
            if patterns.is_empty() {
                true
            } else {
                matches_region(listing.region.as_deref(), &patterns)
            }
        })
        .cloned()
        .collect::<Vec<_>>();

    if let Some(limit) = params.limit
        && targets.len() > limit
    {
        targets.truncate(limit);
    }

    if targets.is_empty() {
        return Ok(CommandOutput::new(json!({
            "checked": 0,
            "active": 0,
            "inactive": 0,
            "errors": 0,
            "dryRun": params.dry_run,
            "results": [],
        }))
        .with_text("no listings matched verify filter"));
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|error| {
            CommandError::runtime(
                "PROCESS_ERROR",
                format!("failed to initialize async runtime: {error}"),
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
                format!("failed to create http client: {error}"),
                "check TLS/certificate configuration",
            )
        })?;

    let mut results = Vec::with_capacity(targets.len());
    let mut inactive_ids = Vec::new();

    for (idx, listing) in targets.iter().enumerate() {
        let result = verify_one_listing(&runtime, &client, listing);
        if result.status == "inactive" && result.error.is_none() {
            inactive_ids.push(result.id.clone());
        }
        results.push(result);

        if params.delay_ms > 0 && idx + 1 < targets.len() {
            std::thread::sleep(Duration::from_millis(params.delay_ms));
        }
    }

    if !params.dry_run && !inactive_ids.is_empty() {
        persist_inactive_status(&db_path, &inactive_ids)?;
    }

    let summary = summarize_verify_results(&results);
    let checked = results.len();

    Ok(CommandOutput::new(json!({
        "checked": checked,
        "active": summary.active,
        "inactive": summary.inactive,
        "errors": summary.errors,
        "dryRun": params.dry_run,
        "results": results,
    }))
    .with_count(checked)
    .with_total(checked)
    .with_has_more(false)
    .with_text(format!(
        "verified {checked} listing(s): {} inactive, {} errors",
        summary.inactive, summary.errors
    )))
}

fn summarize_verify_results(results: &[VerifyResult]) -> VerifySummary {
    let mut summary = VerifySummary::default();
    for result in results {
        match result.status.as_str() {
            "inactive" => summary.inactive += 1,
            "error" => summary.errors += 1,
            _ => summary.active += 1,
        }
    }
    summary
}

fn select_prune_ids(
    listings: &[Listing],
    params: &PruneParams,
) -> Result<(Vec<String>, String), CommandError> {
    if params.inactive_only && (params.bottom_percent.is_some() || params.min_score.is_some()) {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "cannot combine --inactive with --bottom or --min-score",
            "use --inactive alone (optionally with --region), or remove --inactive",
        ));
    }

    if params.bottom_percent.is_some() && params.min_score.is_some() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "cannot combine --bottom with --min-score",
            "choose one pruning selector: --bottom or --min-score",
        ));
    }

    if let Some(percent) = params.bottom_percent
        && (percent == 0 || percent > 100)
    {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid --bottom value: {percent}"),
            "provide an integer between 1 and 100",
        ));
    }

    if let Some(min_score) = params.min_score
        && !(0.0..=100.0).contains(&min_score)
    {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid --min-score value: {min_score}"),
            "provide a score between 0 and 100",
        ));
    }

    let region_patterns = params
        .region
        .as_deref()
        .map(region_patterns)
        .unwrap_or_default();

    let candidate_listings = listings
        .iter()
        .filter(|listing| {
            region_patterns.is_empty()
                || matches_region(listing.region.as_deref(), &region_patterns)
        })
        .collect::<Vec<_>>();

    if params.inactive_only {
        let ids = candidate_listings
            .iter()
            .filter(|listing| matches!(listing.status, ListingStatus::Inactive))
            .map(|listing| listing.id.clone())
            .collect::<Vec<_>>();
        let mode = if region_patterns.is_empty() {
            "inactive".to_owned()
        } else {
            "region+inactive".to_owned()
        };
        return Ok((ids, mode));
    }

    if let Some(percent) = params.bottom_percent {
        let mut scored = candidate_listings
            .iter()
            .map(|listing| {
                (
                    listing.id.clone(),
                    listing.scores.as_ref().map_or(0.0, |scores| scores.overall),
                )
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

        let target_count = ((scored.len() as f64) * (percent as f64 / 100.0)).floor() as usize;
        let target_count = target_count.min(scored.len());

        let ids = scored
            .into_iter()
            .take(target_count)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let mode = if region_patterns.is_empty() {
            format!("bottom {percent}%")
        } else {
            format!("region+bottom {percent}%")
        };
        return Ok((ids, mode));
    }

    if let Some(min_score) = params.min_score {
        let ids = candidate_listings
            .iter()
            .filter(|listing| {
                listing.scores.as_ref().map_or(0.0, |scores| scores.overall) < min_score
            })
            .map(|listing| listing.id.clone())
            .collect::<Vec<_>>();
        let mode = if region_patterns.is_empty() {
            format!("score < {min_score}")
        } else {
            format!("region+score < {min_score}")
        };
        return Ok((ids, mode));
    }

    if !region_patterns.is_empty() {
        let ids = candidate_listings
            .iter()
            .map(|listing| listing.id.clone())
            .collect::<Vec<_>>();
        return Ok((ids, "region".to_owned()));
    }

    let default_min_score = 50.0;
    let ids = listings
        .iter()
        .filter(|listing| {
            listing.scores.as_ref().map_or(0.0, |scores| scores.overall) < default_min_score
        })
        .map(|listing| listing.id.clone())
        .collect::<Vec<_>>();

    Ok((ids, format!("score < {default_min_score}")))
}

fn region_patterns(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .map(str::to_lowercase)
        .filter(|value| !value.is_empty())
        .collect()
}

fn matches_region(region: Option<&str>, patterns: &[String]) -> bool {
    let Some(region) = region else {
        return false;
    };
    let lower = region.to_lowercase();
    let city = lower.split(',').next().map(str::trim).unwrap_or("");

    patterns
        .iter()
        .any(|pattern| city == pattern || city.starts_with(pattern) || lower == *pattern)
}

fn confirm_delete(count: usize) -> Result<bool, CommandError> {
    if !io::stdin().is_terminal() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "confirmation prompt requires interactive terminal input",
            "rerun with --force to skip prompt in non-interactive mode",
        ));
    }

    eprint!("Remove {count} listing(s)? (y/N) ");
    io::stderr().flush().map_err(|error| {
        CommandError::runtime(
            "IO_ERROR",
            format!("failed to flush stderr for confirmation: {error}"),
            "retry with --force to skip prompt",
        )
    })?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).map_err(|error| {
        CommandError::runtime(
            "IO_ERROR",
            format!("failed to read confirmation input: {error}"),
            "retry with --force to skip prompt",
        )
    })?;

    Ok(answer.trim().eq_ignore_ascii_case("y"))
}

fn verify_one_listing(
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    listing: &Listing,
) -> VerifyResult {
    let rightmove_id = listing.portal_ids.rightmove.clone();
    let Some(rightmove_id_value) = rightmove_id.as_ref() else {
        return VerifyResult {
            id: listing.id.clone(),
            rightmove_id,
            status: "error".to_owned(),
            error: Some("missing rightmove id".to_owned()),
        };
    };

    let url = format!("https://www.rightmove.co.uk/properties/{rightmove_id_value}");
    let response = runtime.block_on(async { client.get(url).send().await });

    match response {
        Ok(resp) => {
            let status = resp.status();
            if status.as_u16() == 404 {
                return VerifyResult {
                    id: listing.id.clone(),
                    rightmove_id,
                    status: "inactive".to_owned(),
                    error: None,
                };
            }
            if !status.is_success() {
                return VerifyResult {
                    id: listing.id.clone(),
                    rightmove_id,
                    status: "error".to_owned(),
                    error: Some(format!("http {}", status.as_u16())),
                };
            }

            let html = match runtime.block_on(async { resp.text().await }) {
                Ok(body) => body,
                Err(error) => {
                    return VerifyResult {
                        id: listing.id.clone(),
                        rightmove_id,
                        status: "error".to_owned(),
                        error: Some(format!("failed to read response body: {error}")),
                    };
                }
            };
            let status_value = if detect_inactive_html(&html) {
                "inactive"
            } else {
                "active"
            };

            VerifyResult {
                id: listing.id.clone(),
                rightmove_id,
                status: status_value.to_owned(),
                error: None,
            }
        }
        Err(error) => VerifyResult {
            id: listing.id.clone(),
            rightmove_id,
            status: "error".to_owned(),
            error: Some(error.to_string()),
        },
    }
}

fn detect_inactive_html(html: &str) -> bool {
    let lower = html.to_lowercase();
    lower.contains("let agreed")
        || lower.contains("letagreed")
        || lower.contains("no longer on the market")
        || lower.contains("no longer available")
        || lower.contains("this property has been removed")
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

fn parse_crime_band(raw: &str) -> Option<CrimeBand> {
    match normalize_enum_token(raw).as_str() {
        "excellent" => Some(CrimeBand::Excellent),
        "good" => Some(CrimeBand::Good),
        "mixed" => Some(CrimeBand::Mixed),
        "concerning" => Some(CrimeBand::Concerning),
        _ => None,
    }
}

fn parse_crime_trend(raw: &str) -> Option<CrimeTrend> {
    match normalize_enum_token(raw).as_str() {
        "improving" => Some(CrimeTrend::Improving),
        "stable" => Some(CrimeTrend::Stable),
        "worsening" => Some(CrimeTrend::Worsening),
        _ => None,
    }
}

fn normalize_enum_token(raw: &str) -> String {
    let mut token = String::with_capacity(raw.len());
    let mut previous_dash = false;
    for character in raw.trim().chars() {
        if character.is_ascii_whitespace() || character == '_' {
            if !previous_dash && !token.is_empty() {
                token.push('-');
                previous_dash = true;
            }
            continue;
        }

        let normalized = character.to_ascii_lowercase();
        token.push(normalized);
        previous_dash = normalized == '-';
    }
    token.trim_matches('-').to_owned()
}

fn parse_epc_rating(raw: &str) -> Result<EpcBand, CommandError> {
    parse_epc_band(raw).ok_or_else(|| {
        CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid epc rating: {raw}"),
            "epc rating must be one of A,B,C,D,E,F,G",
        )
    })
}

fn build_google_maps_url(lat: f64, lng: f64, address: &str, postcode: &str) -> String {
    let place = url::form_urlencoded::byte_serialize(format!("{address}, {postcode}").as_bytes())
        .collect::<String>();
    format!("https://www.google.com/maps/place/{place}/@{lat},{lng},17z/data=!3m1!1e3")
}

fn build_google_maps_street_view_url(lat: f64, lng: f64) -> String {
    format!("https://www.google.com/maps/@?api=1&map_action=pano&viewpoint={lat},{lng}")
}

fn persist_inactive_status(
    db_path: &std::path::Path,
    listing_ids: &[String],
) -> Result<(), CommandError> {
    if listing_ids.is_empty() {
        return Ok(());
    }

    let connection = open_listings_db(db_path)?;
    let result: std::result::Result<(), rusqlite::Error> = (|| {
        let tx = connection.unchecked_transaction()?;

        for chunk in listing_ids.chunks(500) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql =
                format!("UPDATE listings SET status = 'inactive' WHERE id IN ({placeholders})");
            tx.execute(&sql, params_from_iter(chunk.iter().map(String::as_str)))?;
        }

        tx.execute(
            "UPDATE meta SET updated_at = ?1 WHERE id = 1",
            params![let_sdk::utils::time::now_iso()],
        )?;
        tx.commit()?;
        Ok(())
    })();

    let close_result = close_listings_db(connection);
    match (result, close_result) {
        (Err(error), _) => Err(CommandError::runtime(
            "DB_ERROR",
            format!("failed to persist verify status: {error}"),
            "check database integrity and retry",
        )),
        (Ok(()), Err(error)) => Err(error.into()),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn delete_listing_ids(
    db_path: &std::path::Path,
    listing_ids: &[String],
) -> Result<(), CommandError> {
    if listing_ids.is_empty() {
        return Ok(());
    }

    let connection = open_listings_db(db_path)?;
    let result: std::result::Result<(), rusqlite::Error> = (|| {
        let tx = connection.unchecked_transaction()?;

        for chunk in listing_ids.chunks(500) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("DELETE FROM listings WHERE id IN ({placeholders})");
            tx.execute(&sql, params_from_iter(chunk.iter().map(String::as_str)))?;
        }

        tx.execute(
            "UPDATE meta SET updated_at = ?1 WHERE id = 1",
            params![let_sdk::utils::time::now_iso()],
        )?;
        tx.commit()?;
        Ok(())
    })();

    let close_result = close_listings_db(connection);
    match (result, close_result) {
        (Err(error), _) => Err(CommandError::runtime(
            "DB_ERROR",
            format!("failed to prune listings: {error}"),
            "check database integrity and retry",
        )),
        (Ok(()), Err(error)) => Err(error.into()),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use let_sdk::schema::listing::{ListingStatus, PortalIds};

    use super::{
        PruneParams, VerifyResult, detect_inactive_html, matches_region, select_prune_ids,
        summarize_verify_results,
    };

    #[test]
    fn region_pattern_matches_city_prefix() {
        assert!(matches_region(
            Some("Shrewsbury, Shropshire"),
            &["shrew".to_owned()]
        ));
        assert!(!matches_region(
            Some("York, North Yorkshire"),
            &["shrew".to_owned()]
        ));
    }

    #[test]
    fn prune_selects_inactive_only() {
        let listing = let_sdk::schema::listing::Listing {
            id: "id-1".to_owned(),
            portal_ids: PortalIds::default(),
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: "https://example.com".to_owned(),
            location: let_sdk::schema::listing::GeoLocation {
                lat: 0.0,
                lng: 0.0,
                pin_type: None,
            },
            postcode: "AA1 1AA".to_owned(),
            address: "Address".to_owned(),
            region: Some("City".to_owned()),
            google_maps_url: "https://maps.example.com".to_owned(),
            google_maps_street_view_url: "https://maps.example.com/street".to_owned(),
            area: let_sdk::schema::listing::AreaMetrics::default(),
            price: 1000,
            price_display: "£1,000 pcm".to_owned(),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "Flat".to_owned(),
            description: "desc".to_owned(),
            notes: vec![],
            images: vec![],
            floorplan: let_sdk::schema::listing::RemoteLocalAsset::default(),
            epc: let_sdk::schema::listing::RemoteLocalAsset::default(),
            map_views: let_sdk::schema::listing::MapViews::default(),
            epc_rating: None,
            floor_area_sqm: None,
            epc_lodgement_date: None,
            epc_address_match: None,
            epc_search_url: None,
            nearest_stations: vec![],
            gigabit_availability: None,
            listed_date: None,
            lettings: let_sdk::schema::listing::Lettings::default(),
            agent: let_sdk::schema::listing::Agent::default(),
            assessment: None,
            assessed_at: None,
            assessed_score: None,
            scores: None,
            fetched_at: "2026-03-01T00:00:00.000Z".to_owned(),
            extraction_status: let_sdk::schema::listing::ExtractionStatus::Success,
            status: ListingStatus::Inactive,
            notion_page_id: None,
        };

        let params = PruneParams {
            min_score: None,
            bottom_percent: None,
            region: None,
            inactive_only: true,
            dry_run: true,
            force: true,
        };

        let (ids, mode) = select_prune_ids(&[listing], &params).expect("select ids");
        assert_eq!(ids, vec!["id-1"]);
        assert_eq!(mode, "inactive");
    }

    #[test]
    fn prune_rejects_conflicting_selectors() {
        let params = PruneParams {
            min_score: Some(55.0),
            bottom_percent: Some(10),
            region: None,
            inactive_only: false,
            dry_run: true,
            force: true,
        };

        let error = select_prune_ids(&[], &params).expect_err("expected validation error");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    fn prune_region_and_min_score_combines_filters() {
        let base_listing = let_sdk::schema::listing::Listing {
            id: "id-1".to_owned(),
            portal_ids: PortalIds::default(),
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: "https://example.com".to_owned(),
            location: let_sdk::schema::listing::GeoLocation {
                lat: 0.0,
                lng: 0.0,
                pin_type: None,
            },
            postcode: "AA1 1AA".to_owned(),
            address: "Address".to_owned(),
            region: Some("Sheffield".to_owned()),
            google_maps_url: "https://maps.example.com".to_owned(),
            google_maps_street_view_url: "https://maps.example.com/street".to_owned(),
            area: let_sdk::schema::listing::AreaMetrics::default(),
            price: 1000,
            price_display: "£1,000 pcm".to_owned(),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "Flat".to_owned(),
            description: "desc".to_owned(),
            notes: vec![],
            images: vec![],
            floorplan: let_sdk::schema::listing::RemoteLocalAsset::default(),
            epc: let_sdk::schema::listing::RemoteLocalAsset::default(),
            map_views: let_sdk::schema::listing::MapViews::default(),
            epc_rating: None,
            floor_area_sqm: None,
            epc_lodgement_date: None,
            epc_address_match: None,
            epc_search_url: None,
            nearest_stations: vec![],
            gigabit_availability: None,
            listed_date: None,
            lettings: let_sdk::schema::listing::Lettings::default(),
            agent: let_sdk::schema::listing::Agent::default(),
            assessment: None,
            assessed_at: None,
            assessed_score: None,
            scores: None,
            fetched_at: "2026-03-01T00:00:00.000Z".to_owned(),
            extraction_status: let_sdk::schema::listing::ExtractionStatus::Success,
            status: ListingStatus::Active,
            notion_page_id: None,
        };

        let mut second_region_listing = base_listing.clone();
        second_region_listing.id = "id-2".to_owned();

        let mut low_score_other_region = base_listing.clone();
        low_score_other_region.id = "id-3".to_owned();
        low_score_other_region.region = Some("Leeds".to_owned());

        let params = PruneParams {
            min_score: Some(50.0),
            bottom_percent: None,
            region: Some("Sheffield".to_owned()),
            inactive_only: false,
            dry_run: true,
            force: true,
        };

        let (ids, mode) = select_prune_ids(
            &[base_listing, second_region_listing, low_score_other_region],
            &params,
        )
        .expect("select ids");
        assert_eq!(mode, "region+score < 50");
        assert_eq!(ids, vec!["id-1", "id-2"]);
    }

    #[test]
    fn inactive_detector_matches_known_markers() {
        assert!(detect_inactive_html(
            "This property has been removed from the market."
        ));
        assert!(detect_inactive_html("LET AGREED"));
        assert!(!detect_inactive_html("Beautiful apartment available now."));
    }

    #[test]
    fn verify_summary_counts_error_rows_separately() {
        let summary = summarize_verify_results(&[
            VerifyResult {
                id: "id-1".to_owned(),
                rightmove_id: Some("1".to_owned()),
                status: "active".to_owned(),
                error: None,
            },
            VerifyResult {
                id: "id-2".to_owned(),
                rightmove_id: Some("2".to_owned()),
                status: "inactive".to_owned(),
                error: None,
            },
            VerifyResult {
                id: "id-3".to_owned(),
                rightmove_id: Some("3".to_owned()),
                status: "error".to_owned(),
                error: Some("http 500".to_owned()),
            },
        ]);

        assert_eq!(summary.active, 1);
        assert_eq!(summary.inactive, 1);
        assert_eq!(summary.errors, 1);
    }
}
