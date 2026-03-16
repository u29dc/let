#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use let_sdk::config::load_config;
use let_sdk::pipeline::epc::{EpcCredentials, EpcLookup, lookup_domestic_epc};
use let_sdk::pipeline::fetch::media::{
    MediaNormalizationConfig, MediaStageStats, populate_listing_media,
};
use let_sdk::pipeline::fetch::rightmove::{
    build_google_maps_street_view_url, build_google_maps_url, fetch_listing,
};
use let_sdk::pipeline::fetch::{carry_over_persistent_fields, is_newer_listing};
use let_sdk::pipeline::geocode::{GeocodeSource, GeocodedCoordinates, mapbox_forward_geocode};
use let_sdk::pipeline::naptan::resolve_listing_stations;
use let_sdk::pipeline::uprn::{UprnResolution, resolve_listing_uprn};
use let_sdk::schema::listing::{
    Listing, ListingsFile, MapViews, PinType, UprnConfidence, UprnSource,
};
use let_sdk::{
    DbMeta, EnrichmentMode, ErrorCode, SourceEnricher, load_listings_file, recalc_assessed_scores,
    score_listings_with_config, upsert_listings,
};
use serde::Serialize;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};
use crate::env::resolve_env_var;

#[derive(Debug, Clone)]
pub struct FetchParams {
    pub ids: String,
    pub region: Option<String>,
    pub override_postcode: Option<String>,
    pub override_address: Option<String>,
    pub skip_images: bool,
    pub skip_epc: bool,
    pub min_score: Option<f64>,
    pub keep_below_min: bool,
}

#[derive(Debug, Clone)]
struct FetchOverride {
    postcode: Option<String>,
    address: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoordinateResolution {
    source: String,
    coords_changed: bool,
    lat: f64,
    lng: f64,
    original_lat: f64,
    original_lng: f64,
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    below_min_score: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    dropped_by_min_score: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    media_processed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FailedItem {
    id: String,
    error: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaSummary {
    photos_downloaded: usize,
    photos_skipped: usize,
    photos_failed: usize,
    floorplan_downloaded: usize,
    floorplan_skipped: usize,
    floorplan_failed: usize,
    epc_downloaded: usize,
    epc_skipped: usize,
    epc_failed: usize,
    maps_downloaded: usize,
    maps_skipped: usize,
    maps_failed: usize,
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
    min_score_applied: Option<f64>,
    keep_below_min: bool,
    below_min_count: usize,
    dropped_below_min_count: usize,
    media_candidates: usize,
    media_summary: MediaSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    override_applied: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    override_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coordinate_resolution: Option<CoordinateResolution>,
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
    let existing = match load_listings_file(&db_path) {
        Ok(data) => data,
        Err(error) if error.code == ErrorCode::NotFound => ListingsFile {
            updated_at: let_sdk::utils::time::now_iso(),
            search_urls: Vec::new(),
            locations: Vec::new(),
            last_search_total: 0,
            listings: Vec::new(),
        },
        Err(error) => return Err(error.into()),
    };
    let source_enricher = SourceEnricher::open(&paths.resolved.sources)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
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

    let mapbox_token = resolve_env_var("MAPBOX_ACCESS_TOKEN", &paths.derived.env_file)
        .map(|(token, _)| token)
        .filter(|token| !token.trim().is_empty());
    let epc_credentials = if params.skip_epc {
        None
    } else {
        resolve_epc_credentials(&paths.derived.env_file)
    };

    let mut fetched = Vec::new();
    let mut failed = Vec::new();
    let mut fetched_listings = Vec::new();
    let mut coord_resolution: Option<CoordinateResolution> = None;

    for (index, portal_id) in input_ids.iter().enumerate() {
        match fetch_listing(
            &runtime,
            &client,
            portal_id,
            config.fetch.max_retries.max(1),
            params.region.clone(),
            true,
        ) {
            Ok(mut listing) => {
                if let Some(override_input) = fetch_override.as_ref() {
                    apply_fetch_override(&mut listing, override_input);

                    let original_lat = listing.location.lat;
                    let original_lng = listing.location.lng;

                    let resolved = resolve_override_coordinates(
                        override_input,
                        &source_enricher,
                        &runtime,
                        &client,
                        mapbox_token.as_deref(),
                    );

                    match resolved {
                        Some(coords) => {
                            let changed = (coords.lat - original_lat).abs() > 1e-6
                                || (coords.lng - original_lng).abs() > 1e-6;

                            if changed {
                                listing.location.lat = coords.lat;
                                listing.location.lng = coords.lng;
                                listing.location.pin_type = Some(PinType::ApproximatePoint);

                                listing.google_maps_url = build_google_maps_url(
                                    listing.location.lat,
                                    listing.location.lng,
                                    &listing.address,
                                    &listing.postcode,
                                );
                                listing.google_maps_street_view_url =
                                    build_google_maps_street_view_url(
                                        listing.location.lat,
                                        listing.location.lng,
                                    );

                                listing.map_views = MapViews::default();
                            }

                            coord_resolution = Some(CoordinateResolution {
                                source: coords.source.as_str().to_owned(),
                                coords_changed: changed,
                                lat: coords.lat,
                                lng: coords.lng,
                                original_lat,
                                original_lng,
                            });
                        }
                        None => {
                            eprintln!(
                                "[fetch] coordinate resolution failed for override, keeping original coords"
                            );
                            coord_resolution = Some(CoordinateResolution {
                                source: GeocodeSource::FallbackOriginal.as_str().to_owned(),
                                coords_changed: false,
                                lat: original_lat,
                                lng: original_lng,
                                original_lat,
                                original_lng,
                            });
                        }
                    }
                }

                let enrichment = source_enricher
                    .enrich_listing(&mut listing, EnrichmentMode::ReplaceFromSources)?;
                let mut enrichment_applied = enrichment.applied_fields;
                let mut enrichment_missing = enrichment.missing_categories;
                let mut enrichment_unavailable_sources = enrichment.unavailable_sources;

                if let Some(credentials) = epc_credentials.as_ref() {
                    match runtime.block_on(lookup_domestic_epc(
                        &client,
                        credentials,
                        &listing.address,
                        &listing.postcode,
                    )) {
                        Ok(Some(epc_lookup)) => {
                            for field in apply_epc_lookup(&mut listing, &epc_lookup) {
                                push_unique(&mut enrichment_applied, field);
                            }
                        }
                        Ok(None) => push_unique(&mut enrichment_missing, "epc".to_owned()),
                        Err(error) => {
                            eprintln!(
                                "[fetch] epc lookup failed for {portal_id}: {}",
                                error.message
                            );
                            push_unique(&mut enrichment_unavailable_sources, "epc".to_owned());
                        }
                    }
                } else if !params.skip_epc {
                    push_unique(&mut enrichment_unavailable_sources, "epc".to_owned());
                }

                if listing.uprn.is_none()
                    && !enrichment_unavailable_sources
                        .iter()
                        .any(|source| source == "uprn")
                {
                    match resolve_listing_uprn(&source_enricher, &listing) {
                        Ok(Some(uprn_resolution)) => {
                            for field in apply_uprn_resolution(&mut listing, &uprn_resolution) {
                                push_unique(&mut enrichment_applied, field);
                            }
                        }
                        Ok(None) => push_unique(&mut enrichment_missing, "uprn".to_owned()),
                        Err(error) => {
                            eprintln!(
                                "[fetch] uprn lookup failed for {portal_id}: {}",
                                error.message
                            );
                            push_unique(&mut enrichment_unavailable_sources, "uprn".to_owned());
                        }
                    }
                }

                if listing.nearest_stations.is_empty()
                    && !enrichment_unavailable_sources
                        .iter()
                        .any(|source| source == "naptan")
                {
                    match resolve_listing_stations(&source_enricher, &listing) {
                        Ok(Some(stations)) => {
                            listing.nearest_stations = stations;
                            push_unique(&mut enrichment_applied, "nearestStations".to_owned());
                        }
                        Ok(None) => push_unique(&mut enrichment_missing, "naptan".to_owned()),
                        Err(error) => {
                            eprintln!(
                                "[fetch] naptan lookup failed for {portal_id}: {}",
                                error.message
                            );
                            push_unique(&mut enrichment_unavailable_sources, "naptan".to_owned());
                        }
                    }
                }

                enrichment_applied.sort();
                enrichment_missing.sort();
                enrichment_unavailable_sources.sort();

                fetched.push(FetchedItem {
                    id: portal_id.clone(),
                    address: listing.address.clone(),
                    score: None,
                    enrichment_applied,
                    enrichment_missing,
                    enrichment_unavailable_sources,
                    below_min_score: false,
                    dropped_by_min_score: false,
                    media_processed: false,
                });
                fetched_listings.push(listing);
            }
            Err(error) => failed.push(FailedItem {
                id: portal_id.clone(),
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
        .chain(fetched_listings)
        .collect::<Vec<_>>();
    let unique_listings = deduplicate_listings(merged);

    let mut scored = score_listings_with_config(&unique_listings, &config);
    recalc_assessed_scores(&mut scored);
    let pre_stage_scores = scored
        .iter()
        .filter_map(|listing| {
            let portal_id = listing.portal_ids.rightmove.as_ref()?;
            let score = listing.scores.as_ref()?.overall;
            Some((portal_id.clone(), score))
        })
        .collect::<HashMap<_, _>>();

    let existing_ids = existing
        .listings
        .iter()
        .map(|listing| listing.id.clone())
        .collect::<HashSet<_>>();
    let input_id_set = input_ids.iter().cloned().collect::<HashSet<_>>();

    let threshold = effective_min_score(params, &config, input_ids.len());
    let mut below_min_portal_ids = HashSet::new();
    let mut dropped_portal_ids = HashSet::new();

    if let Some(min_score) = threshold {
        for listing in &scored {
            let Some(portal_id) = listing.portal_ids.rightmove.as_deref() else {
                continue;
            };
            if !input_id_set.contains(portal_id) {
                continue;
            }
            let score = listing.scores.as_ref().map_or(0.0, |scores| scores.overall);
            if score < min_score {
                below_min_portal_ids.insert(portal_id.to_owned());
                if config.fetch.drop_new_below_min_score
                    && !params.keep_below_min
                    && !existing_ids.contains(&listing.id)
                {
                    dropped_portal_ids.insert(portal_id.to_owned());
                }
            }
        }
    }

    if !dropped_portal_ids.is_empty() {
        scored.retain(|listing| {
            listing
                .portal_ids
                .rightmove
                .as_ref()
                .is_none_or(|id| !dropped_portal_ids.contains(id))
        });
    }

    let media_candidates = if params.skip_images {
        Vec::new()
    } else {
        scored
            .iter()
            .enumerate()
            .filter(|(_, listing)| {
                listing
                    .portal_ids
                    .rightmove
                    .as_ref()
                    .is_some_and(|id| input_id_set.contains(id))
            })
            .filter(|(_, listing)| {
                threshold.is_none_or(|min_score| {
                    listing.scores.as_ref().map_or(0.0, |scores| scores.overall) >= min_score
                })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };

    let mut media_stats_total = MediaStageStats::default();
    let mut media_portal_ids = HashSet::new();
    if !media_candidates.is_empty() {
        let media_config = MediaNormalizationConfig {
            photo_landscape_width: config.fetch.media_photo_landscape_width,
            photo_landscape_height: config.fetch.media_photo_landscape_height,
            photo_portrait_width: config.fetch.media_photo_portrait_width,
            photo_portrait_height: config.fetch.media_photo_portrait_height,
            aux_width: config.fetch.media_aux_width,
            aux_height: config.fetch.media_aux_height,
            map_width: config.fetch.media_map_width,
            map_height: config.fetch.media_map_height,
            photo_quality: config.fetch.media_quality_photo,
            aux_quality: config.fetch.media_quality_aux,
            map_quality: config.fetch.media_quality_map,
            timeout: Duration::from_millis(config.fetch.media_timeout_ms),
            max_retries: config.fetch.max_retries.max(1),
            download_concurrency: config.fetch.media_download_concurrency,
            process_concurrency: config.fetch.media_process_concurrency,
            download_maps: config.fetch.download_maps,
            download_floorplan: config.fetch.download_floorplan,
            download_epc_asset: !params.skip_epc && config.fetch.download_epc_asset,
            mapbox_access_token: mapbox_token,
        };

        for listing_index in media_candidates {
            let listing = scored
                .get_mut(listing_index)
                .expect("media candidate index should be valid");
            let listing_stats = runtime.block_on(populate_listing_media(
                &client,
                listing,
                &paths.resolved.cache,
                &media_config,
            ));
            merge_media_stats(&mut media_stats_total, &listing_stats);
            if let Some(portal_id) = listing.portal_ids.rightmove.as_ref() {
                media_portal_ids.insert(portal_id.clone());
            }
        }

        scored = score_listings_with_config(&scored, &config);
        recalc_assessed_scores(&mut scored);
    }

    update_fetched_items(
        &mut fetched,
        &scored,
        &pre_stage_scores,
        &below_min_portal_ids,
        &dropped_portal_ids,
        &media_portal_ids,
    );

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

    upsert_listings(
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
    .map_err(|error| {
        CommandError::runtime(
            "DB_ERROR",
            format!("failed to persist fetched listings: {error}"),
            "check database integrity, restore from backup if needed, and retry",
        )
    })?;

    let payload = FetchOutput {
        fetched,
        failed,
        total: input_ids.len(),
        skip_images: params.skip_images,
        skip_epc: params.skip_epc,
        min_score_applied: threshold,
        keep_below_min: params.keep_below_min,
        below_min_count: below_min_portal_ids.len(),
        dropped_below_min_count: dropped_portal_ids.len(),
        media_candidates: media_portal_ids.len(),
        media_summary: MediaSummary {
            photos_downloaded: media_stats_total.photos_downloaded,
            photos_skipped: media_stats_total.photos_skipped,
            photos_failed: media_stats_total.photos_failed,
            floorplan_downloaded: media_stats_total.floorplan_downloaded,
            floorplan_skipped: media_stats_total.floorplan_skipped,
            floorplan_failed: media_stats_total.floorplan_failed,
            epc_downloaded: media_stats_total.epc_downloaded,
            epc_skipped: media_stats_total.epc_skipped,
            epc_failed: media_stats_total.epc_failed,
            maps_downloaded: media_stats_total.maps_downloaded,
            maps_skipped: media_stats_total.maps_skipped,
            maps_failed: media_stats_total.maps_failed,
        },
        override_applied: fetch_override.as_ref().map(|_| true),
        override_fields: fetch_override_fields(fetch_override.as_ref()),
        coordinate_resolution: coord_resolution,
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

fn effective_min_score(
    params: &FetchParams,
    config: &let_sdk::config::AppConfig,
    input_count: usize,
) -> Option<f64> {
    if input_count == 1 && params.min_score.is_none() {
        return None;
    }

    params
        .min_score
        .or_else(|| Some(f64::from(config.fetch.min_score)))
}

fn resolve_epc_credentials(env_file: &Path) -> Option<EpcCredentials> {
    let email = resolve_env_var("EPC_API_EMAIL", env_file)
        .map(|(value, _)| value)
        .filter(|value| !value.trim().is_empty())?;
    let api_key = resolve_env_var("EPC_API_KEY", env_file)
        .map(|(value, _)| value)
        .filter(|value| !value.trim().is_empty())?;
    Some(EpcCredentials { email, api_key })
}

fn apply_epc_lookup(listing: &mut Listing, lookup: &EpcLookup) -> Vec<String> {
    let mut applied = Vec::new();

    if listing.epc_rating != lookup.epc_rating {
        listing.epc_rating = lookup.epc_rating.clone();
        push_unique(&mut applied, "epcRating".to_owned());
    }
    if listing.floor_area_sqm != lookup.floor_area_sqm {
        listing.floor_area_sqm = lookup.floor_area_sqm;
        push_unique(&mut applied, "floorAreaSqm".to_owned());
    }
    if listing.epc_lodgement_date != lookup.lodgement_date {
        listing.epc_lodgement_date = lookup.lodgement_date.clone();
        push_unique(&mut applied, "epcLodgementDate".to_owned());
    }

    let address_match = Some(lookup.address_match);
    if listing.epc_address_match != address_match {
        listing.epc_address_match = address_match;
        push_unique(&mut applied, "epcAddressMatch".to_owned());
    }

    if let Some(uprn) = lookup.uprn.as_ref() {
        for field in apply_uprn_fields(listing, uprn, UprnSource::Epc, UprnConfidence::Exact) {
            push_unique(&mut applied, field);
        }
    }

    applied
}

fn apply_uprn_resolution(listing: &mut Listing, resolution: &UprnResolution) -> Vec<String> {
    apply_uprn_fields(
        listing,
        &resolution.uprn,
        resolution.source.clone(),
        resolution.confidence.clone(),
    )
}

fn apply_uprn_fields(
    listing: &mut Listing,
    uprn: &str,
    source: UprnSource,
    confidence: UprnConfidence,
) -> Vec<String> {
    let mut applied = Vec::new();

    if listing.uprn.as_deref() != Some(uprn) {
        listing.uprn = Some(uprn.to_owned());
        push_unique(&mut applied, "uprn".to_owned());
    }
    if listing.uprn_source != Some(source.clone()) {
        listing.uprn_source = Some(source);
        push_unique(&mut applied, "uprnSource".to_owned());
    }
    if listing.uprn_confidence != Some(confidence.clone()) {
        listing.uprn_confidence = Some(confidence);
        push_unique(&mut applied, "uprnConfidence".to_owned());
    }

    applied
}

fn push_unique(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|item| item == &value) {
        items.push(value);
    }
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
    listing.google_maps_street_view_url =
        build_google_maps_street_view_url(listing.location.lat, listing.location.lng);
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

fn resolve_override_coordinates(
    override_input: &FetchOverride,
    source_enricher: &SourceEnricher,
    runtime: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    mapbox_token: Option<&str>,
) -> Option<GeocodedCoordinates> {
    if let Some(postcode) = override_input.postcode.as_ref() {
        let normalized = let_sdk::utils::text::normalize_postcode(postcode);
        if !normalized.is_empty()
            && let Ok(Some(coords)) = source_enricher.lookup_postcode_coordinates(&normalized)
        {
            return Some(GeocodedCoordinates {
                lat: coords.lat,
                lng: coords.lng,
                source: GeocodeSource::PostcodesDb,
            });
        }
    }

    if let Some(token) = mapbox_token {
        let query = match (
            override_input.address.as_ref(),
            override_input.postcode.as_ref(),
        ) {
            (Some(address), Some(postcode)) => format!("{address}, {postcode}"),
            (Some(address), None) => address.clone(),
            (None, Some(postcode)) => postcode.clone(),
            (None, None) => return None,
        };

        match runtime.block_on(mapbox_forward_geocode(client, &query, token)) {
            Ok(Some(coords)) => return Some(coords),
            Ok(None) => {
                eprintln!("[fetch] mapbox geocode returned no results for {query:?}");
            }
            Err(err) => {
                eprintln!("[fetch] mapbox geocode error: {err}");
            }
        }
    }

    None
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

fn merge_media_stats(total: &mut MediaStageStats, delta: &MediaStageStats) {
    total.photos_downloaded += delta.photos_downloaded;
    total.photos_skipped += delta.photos_skipped;
    total.photos_failed += delta.photos_failed;
    total.floorplan_downloaded += delta.floorplan_downloaded;
    total.floorplan_skipped += delta.floorplan_skipped;
    total.floorplan_failed += delta.floorplan_failed;
    total.epc_downloaded += delta.epc_downloaded;
    total.epc_skipped += delta.epc_skipped;
    total.epc_failed += delta.epc_failed;
    total.maps_downloaded += delta.maps_downloaded;
    total.maps_skipped += delta.maps_skipped;
    total.maps_failed += delta.maps_failed;
}

fn update_fetched_items(
    items: &mut [FetchedItem],
    listings: &[Listing],
    fallback_scores: &HashMap<String, f64>,
    below_min_ids: &HashSet<String>,
    dropped_ids: &HashSet<String>,
    media_processed_ids: &HashSet<String>,
) {
    for item in items.iter_mut() {
        item.score = listings
            .iter()
            .find(|listing| listing.portal_ids.rightmove.as_deref() == Some(item.id.as_str()))
            .and_then(|listing| listing.scores.as_ref().map(|scores| scores.overall))
            .or_else(|| fallback_scores.get(&item.id).copied());
        item.below_min_score = below_min_ids.contains(&item.id);
        item.dropped_by_min_score = dropped_ids.contains(&item.id);
        item.media_processed = media_processed_ids.contains(&item.id);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use let_sdk::config::{
        AppConfig, FetchConfig, Location, SearchConfig, SearchFilters, default_scoring_config,
    };
    use let_sdk::schema::listing::{
        Agent, AreaMetrics, EpcBand, ExtractionStatus, GeoLocation, Lettings, Listing,
        ListingStatus, MapViews, PortalIds, RemoteLocalAsset, UprnConfidence, UprnSource,
    };
    use tempfile::TempDir;

    use super::{
        EpcLookup, FetchParams, apply_epc_lookup, effective_min_score, parse_ids,
        resolve_epc_credentials,
    };

    #[test]
    fn parse_ids_ignores_empty_segments() {
        let ids = parse_ids("1701, ,1702,,1703");
        assert_eq!(ids, vec!["1701", "1702", "1703"]);
    }

    #[test]
    fn effective_min_score_disabled_for_single_id_without_override() {
        let config = sample_config();
        let params = FetchParams {
            ids: "1701".to_owned(),
            region: None,
            override_postcode: None,
            override_address: None,
            skip_images: false,
            skip_epc: false,
            min_score: None,
            keep_below_min: false,
        };

        assert_eq!(effective_min_score(&params, &config, 1), None);
    }

    #[test]
    fn effective_min_score_uses_config_for_batch() {
        let config = sample_config();
        let params = FetchParams {
            ids: "1701,1702".to_owned(),
            region: None,
            override_postcode: None,
            override_address: None,
            skip_images: false,
            skip_epc: false,
            min_score: None,
            keep_below_min: false,
        };

        assert_eq!(effective_min_score(&params, &config, 2), Some(70.0));
    }

    #[test]
    fn effective_min_score_prefers_cli_override() {
        let config = sample_config();
        let params = FetchParams {
            ids: "1701,1702".to_owned(),
            region: None,
            override_postcode: None,
            override_address: None,
            skip_images: false,
            skip_epc: false,
            min_score: Some(62.5),
            keep_below_min: false,
        };

        assert_eq!(effective_min_score(&params, &config, 2), Some(62.5));
    }

    #[test]
    fn resolve_epc_credentials_requires_email_and_key() {
        let temp = TempDir::new().expect("temp dir");
        let env_file = temp.path().join(".env");

        fs::write(&env_file, "EPC_API_KEY=secret\n").expect("write env file");
        assert!(resolve_epc_credentials(&env_file).is_none());

        fs::write(
            &env_file,
            "EPC_API_EMAIL=user@example.com\nEPC_API_KEY=secret\n",
        )
        .expect("write full env file");
        let credentials = resolve_epc_credentials(&env_file).expect("credentials");
        assert_eq!(credentials.email, "user@example.com");
        assert_eq!(credentials.api_key, "secret");
    }

    #[test]
    fn apply_epc_lookup_updates_listing_and_uprn_fields() {
        let mut listing = sample_listing();
        let applied = apply_epc_lookup(
            &mut listing,
            &EpcLookup {
                lmk_key: "cert-1".to_owned(),
                epc_rating: Some(EpcBand::B),
                floor_area_sqm: Some(78.4),
                lodgement_date: Some("2025-01-10".to_owned()),
                address_match: true,
                matched_address: "10 Example Street, M1 1AA".to_owned(),
                uprn: Some("100021234567".to_owned()),
                uprn_source: Some("Address Matched".to_owned()),
            },
        );

        assert_eq!(listing.epc_rating, Some(EpcBand::B));
        assert_eq!(listing.floor_area_sqm, Some(78.4));
        assert_eq!(listing.epc_lodgement_date.as_deref(), Some("2025-01-10"));
        assert_eq!(listing.epc_address_match, Some(true));
        assert_eq!(listing.uprn.as_deref(), Some("100021234567"));
        assert_eq!(listing.uprn_source, Some(UprnSource::Epc));
        assert_eq!(listing.uprn_confidence, Some(UprnConfidence::Exact));
        assert!(applied.iter().any(|field| field == "epcRating"));
        assert!(applied.iter().any(|field| field == "uprnConfidence"));
    }

    fn sample_config() -> AppConfig {
        AppConfig {
            search: SearchConfig {
                use_api: true,
                locations: vec![Location {
                    id: "REGION^123".to_owned(),
                    name: "Sample".to_owned(),
                }],
                filters: SearchFilters {
                    min_bedrooms: 1,
                    max_bedrooms: 4,
                    min_price: 500,
                    max_price: 2500,
                    property_types: vec!["flat".to_owned()],
                    include_let_agreed: false,
                    radius: 1.0,
                    dont_show: Vec::new(),
                    must_have: Vec::new(),
                },
            },
            fetch: FetchConfig {
                min_score: 70,
                ..FetchConfig::default()
            },
            scoring: default_scoring_config(),
        }
    }

    fn sample_listing() -> Listing {
        Listing {
            id: "uuid-1".to_owned(),
            portal_ids: PortalIds {
                rightmove: Some("170448131".to_owned()),
                zoopla: None,
                onthemarket: None,
            },
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: "https://www.rightmove.co.uk/properties/170448131".to_owned(),
            location: GeoLocation {
                lat: 53.0,
                lng: -2.0,
                pin_type: None,
            },
            postcode: "M1 1AA".to_owned(),
            address: "10 Example Street".to_owned(),
            region: Some("Manchester".to_owned()),
            google_maps_url: "https://maps.example.com".to_owned(),
            google_maps_street_view_url: "https://maps.example.com/street".to_owned(),
            area: AreaMetrics::default(),
            price: 1200,
            price_display: "£1,200 pcm".to_owned(),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "Flat".to_owned(),
            description: "desc".to_owned(),
            notes: vec![],
            images: vec![],
            floorplan: RemoteLocalAsset::default(),
            epc: RemoteLocalAsset::default(),
            map_views: MapViews::default(),
            epc_rating: None,
            floor_area_sqm: None,
            epc_lodgement_date: None,
            epc_address_match: None,
            epc_search_url: None,
            nearest_stations: vec![],
            gigabit_availability: None,
            listed_date: None,
            lettings: Lettings::default(),
            agent: Agent::default(),
            assessment: None,
            assessed_at: None,
            assessed_score: None,
            scores: None,
            fetched_at: "2026-01-01T00:00:00.000Z".to_owned(),
            extraction_status: ExtractionStatus::Partial,
            status: ListingStatus::Active,
            notion_page_id: None,
        }
    }
}
