#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::io::{self, Write};
use std::time::Duration;

use let_sdk::schema::listing::{EpcBand, Listing, ListingStatus};
use let_sdk::{
    DbMeta, close_listings_db, load_listings_file, open_listings_db, recalc_assessed_scores,
    score_listings_with_config, upsert_listings,
};
use rusqlite::{params, params_from_iter};
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

#[derive(Debug, Clone)]
pub struct PruneParams {
    pub min_score: f64,
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
    pub epc_rating: Option<String>,
    pub floor_area: Option<f64>,
    pub skip_re_enrich: bool,
    pub skip_images: bool,
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
    if params.address.is_none()
        && params.postcode.is_none()
        && params.lat.is_none()
        && params.lng.is_none()
        && params.epc_rating.is_none()
        && params.floor_area.is_none()
    {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "no overrides provided",
            "provide at least one override field",
        ));
    }

    if params.lat.is_some() ^ params.lng.is_some() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "--lat and --lng must be provided together",
            "provide both coordinates or neither",
        ));
    }

    if let Some(lat) = params.lat
        && !(-90.0..=90.0).contains(&lat)
    {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid latitude: {lat}"),
            "latitude must be between -90 and 90",
        ));
    }
    if let Some(lng) = params.lng
        && !(-180.0..=180.0).contains(&lng)
    {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid longitude: {lng}"),
            "longitude must be between -180 and 180",
        ));
    }
    if let Some(area) = params.floor_area
        && area <= 0.0
    {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid floor area: {area}"),
            "floor area must be a positive number",
        ));
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
        if let Some(address) = params.address.as_ref()
            && listing.address != *address
        {
            applied.insert(
                "address".to_owned(),
                json!({ "from": listing.address, "to": address }),
            );
            listing.address = address.clone();
        }
        if let Some(postcode) = params.postcode.as_ref()
            && listing.postcode != *postcode
        {
            applied.insert(
                "postcode".to_owned(),
                json!({ "from": listing.postcode, "to": postcode }),
            );
            listing.postcode = postcode.clone();
        }
        if let (Some(lat), Some(lng)) = (params.lat, params.lng) {
            if (listing.location.lat - lat).abs() > f64::EPSILON {
                applied.insert(
                    "lat".to_owned(),
                    json!({ "from": listing.location.lat, "to": lat }),
                );
                listing.location.lat = lat;
            }
            if (listing.location.lng - lng).abs() > f64::EPSILON {
                applied.insert(
                    "lng".to_owned(),
                    json!({ "from": listing.location.lng, "to": lng }),
                );
                listing.location.lng = lng;
            }
        }
        if let Some(epc_rating) = params.epc_rating.as_ref() {
            let parsed = parse_epc_rating(epc_rating)?;
            if listing.epc_rating.as_ref() != Some(&parsed) {
                applied.insert(
                    "epcRating".to_owned(),
                    json!({
                        "from": listing.epc_rating.as_ref().map(epc_band_to_text),
                        "to": epc_band_to_text(&parsed),
                    }),
                );
                listing.epc_rating = Some(parsed);
            }
        }
        if let Some(area) = params.floor_area
            && listing.floor_area_sqm != Some(area)
        {
            applied.insert(
                "floorArea".to_owned(),
                json!({ "from": listing.floor_area_sqm, "to": area }),
            );
            listing.floor_area_sqm = Some(area);
        }

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
        }
    }

    if applied.is_empty() {
        return Ok(CommandOutput::new(json!({
            "id": listing_id,
            "applied": {},
            "reEnriched": [],
            "rescored": data.listings.len(),
            "previousScore": previous_score,
            "newScore": previous_score,
            "skipReEnrich": params.skip_re_enrich,
            "skipImages": params.skip_images,
        }))
        .with_text("no changes needed"));
    }

    let config = let_sdk::config::load_config(Some(&config_path))?;
    let mut rescored = score_listings_with_config(&data.listings, &config);
    recalc_assessed_scores(&mut rescored);

    let new_score = rescored
        .iter()
        .find(|candidate| candidate.id == listing_id)
        .and_then(|candidate| candidate.scores.as_ref().map(|scores| scores.overall));

    let meta = DbMeta {
        updated_at: let_sdk::utils::time::now_iso(),
        last_search_total: data.last_search_total,
    };
    upsert_listings(
        &db_path,
        &[],
        &rescored,
        &rescored,
        &meta,
        &data.search_urls,
        &data.locations,
    )?;

    Ok(CommandOutput::new(json!({
        "id": listing_id,
        "applied": serde_json::Value::Object(applied),
        "reEnriched": [],
        "rescored": rescored.len(),
        "previousScore": previous_score,
        "newScore": new_score,
        "skipReEnrich": params.skip_re_enrich,
        "skipImages": params.skip_images,
    }))
    .with_text(format!(
        "patch applied for {} and rescored {} listings",
        params.id,
        rescored.len()
    )))
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
    let mut errors = 0usize;

    for (idx, listing) in targets.iter().enumerate() {
        let result = verify_one_listing(&runtime, &client, listing);
        if result.status == "inactive" && result.error.is_none() {
            inactive_ids.push(result.id.clone());
        }
        if result.error.is_some() {
            errors += 1;
        }
        results.push(result);

        if params.delay_ms > 0 && idx + 1 < targets.len() {
            std::thread::sleep(Duration::from_millis(params.delay_ms));
        }
    }

    if !params.dry_run && !inactive_ids.is_empty() {
        persist_inactive_status(&db_path, &inactive_ids)?;
    }

    let inactive = inactive_ids.len();
    let checked = results.len();
    let active = checked.saturating_sub(inactive + errors);

    Ok(CommandOutput::new(json!({
        "checked": checked,
        "active": active,
        "inactive": inactive,
        "errors": errors,
        "dryRun": params.dry_run,
        "results": results,
    }))
    .with_count(checked)
    .with_total(checked)
    .with_has_more(false)
    .with_text(format!(
        "verified {checked} listing(s): {inactive} inactive, {errors} errors"
    )))
}

fn select_prune_ids(
    listings: &[Listing],
    params: &PruneParams,
) -> Result<(Vec<String>, String), CommandError> {
    if params.inactive_only {
        let ids = listings
            .iter()
            .filter(|listing| matches!(listing.status, ListingStatus::Inactive))
            .map(|listing| listing.id.clone())
            .collect::<Vec<_>>();
        return Ok((ids, "inactive".to_owned()));
    }

    if let Some(region) = params.region.as_deref() {
        let patterns = region_patterns(region);
        let ids = listings
            .iter()
            .filter(|listing| matches_region(listing.region.as_deref(), &patterns))
            .map(|listing| listing.id.clone())
            .collect::<Vec<_>>();
        return Ok((ids, "region".to_owned()));
    }

    if let Some(percent) = params.bottom_percent {
        if percent == 0 || percent > 100 {
            return Err(CommandError::runtime(
                "VALIDATION_ERROR",
                format!("invalid --bottom value: {percent}"),
                "provide an integer between 1 and 100",
            ));
        }

        let mut scored = listings
            .iter()
            .map(|listing| {
                (
                    listing.id.clone(),
                    listing.scores.as_ref().map_or(0.0, |scores| scores.overall),
                )
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

        let cutoff_idx = ((scored.len() as f64) * (percent as f64 / 100.0)).floor() as usize;
        let clamped_idx = cutoff_idx.min(scored.len().saturating_sub(1));
        let cutoff = scored.get(clamped_idx).map_or(0.0, |item| item.1);

        let ids = scored
            .into_iter()
            .filter(|(_, score)| *score < cutoff)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        return Ok((ids, format!("bottom {percent}%")));
    }

    if !(0.0..=100.0).contains(&params.min_score) {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid --min-score value: {}", params.min_score),
            "provide a score between 0 and 100",
        ));
    }

    let ids = listings
        .iter()
        .filter(|listing| {
            listing.scores.as_ref().map_or(0.0, |scores| scores.overall) < params.min_score
        })
        .map(|listing| listing.id.clone())
        .collect::<Vec<_>>();

    Ok((ids, format!("score < {}", params.min_score)))
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
    print!("Remove {count} listing(s)? (y/N) ");
    io::stdout().flush().map_err(|error| {
        CommandError::runtime(
            "IO_ERROR",
            format!("failed to flush stdout for confirmation: {error}"),
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
            status: "active".to_owned(),
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
                    status: "active".to_owned(),
                    error: Some(format!("http {}", status.as_u16())),
                };
            }

            let html = runtime
                .block_on(async { resp.text().await })
                .unwrap_or_default();
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
            status: "active".to_owned(),
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

fn parse_epc_rating(raw: &str) -> Result<EpcBand, CommandError> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "A" => Ok(EpcBand::A),
        "B" => Ok(EpcBand::B),
        "C" => Ok(EpcBand::C),
        "D" => Ok(EpcBand::D),
        "E" => Ok(EpcBand::E),
        "F" => Ok(EpcBand::F),
        "G" => Ok(EpcBand::G),
        _ => Err(CommandError::runtime(
            "VALIDATION_ERROR",
            format!("invalid epc rating: {raw}"),
            "epc rating must be one of A,B,C,D,E,F,G",
        )),
    }
}

fn epc_band_to_text(band: &EpcBand) -> &'static str {
    match band {
        EpcBand::A => "A",
        EpcBand::B => "B",
        EpcBand::C => "C",
        EpcBand::D => "D",
        EpcBand::E => "E",
        EpcBand::F => "F",
        EpcBand::G => "G",
    }
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

    use super::{PruneParams, detect_inactive_html, matches_region, select_prune_ids};

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
            min_score: 50.0,
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
    fn inactive_detector_matches_known_markers() {
        assert!(detect_inactive_html(
            "This property has been removed from the market."
        ));
        assert!(detect_inactive_html("LET AGREED"));
        assert!(!detect_inactive_html("Beautiful apartment available now."));
    }
}
