#![forbid(unsafe_code)]

use serde_json::json;

use let_sdk::{
    DbMeta, find_listing_by_id_from_db, load_listings_file, recalc_assessed_scores,
    score_listings_with_config, upsert_listings,
};

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

pub fn compute(shared: &SharedArgs) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;
    let config_path = paths.derived.config_file;

    let config = let_sdk::config::load_config(Some(&config_path))?;
    let data = load_listings_file(&db_path)?;

    if data.listings.is_empty() {
        return Ok(CommandOutput::new(json!({
            "total": 0,
            "scored": 0,
            "avgScore": 0,
            "avgConfidence": 0,
        }))
        .with_text("no listings to score"));
    }

    let mut scored = score_listings_with_config(&data.listings, &config);
    recalc_assessed_scores(&mut scored);

    let total_score = scored
        .iter()
        .map(|listing| listing.scores.as_ref().map_or(0.0, |scores| scores.overall))
        .sum::<f64>();
    let total_confidence = scored
        .iter()
        .map(|listing| {
            listing
                .scores
                .as_ref()
                .map_or(0.0, |scores| scores.confidence)
        })
        .sum::<f64>();

    let total = scored.len();
    let stats = json!({
        "total": total,
        "scored": scored.iter().filter(|listing| listing.scores.is_some()).count(),
        "avgScore": round_to(total_score / total as f64, 1),
        "avgConfidence": round_to(total_confidence / total as f64, 2),
    });

    upsert_listings(
        &db_path,
        &[],
        &scored,
        &scored,
        &DbMeta {
            updated_at: let_sdk::utils::time::now_iso(),
            last_search_total: data.last_search_total,
        },
        &data.search_urls,
        &data.locations,
    )?;

    Ok(CommandOutput::new(stats).with_text(format!("rescored {total} listings")))
}

pub fn explain(shared: &SharedArgs, id: &str) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let Some(listing) = find_listing_by_id_from_db(&db_path, id)? else {
        return Err(CommandError::new(
            "NOT_FOUND",
            format!("listing not found: {id}"),
            "check identifier with `let view list`",
            1,
        ));
    };

    let Some(scores) = listing.scores else {
        return Err(CommandError::new(
            "NOT_SCORED",
            format!("listing has no scores: {id}"),
            "run `let score compute` first",
            1,
        ));
    };

    let data = json!({
        "id": listing.id,
        "overall": scores.overall,
        "assessedScore": listing.assessed_score,
        "confidence": scores.confidence,
        "composites": {
            "affordability": {
                "score": scores.affordability,
                "factors": {
                    "trueMonthlyCost": scores.factors.true_monthly_cost,
                    "trueCostPercentile": scores.factors.true_cost_percentile,
                    "epcBand": scores.factors.epc_band,
                    "epcNumeric": scores.factors.epc_numeric,
                }
            },
            "location": {
                "score": scores.location,
                "factors": {
                    "stationMiles": scores.factors.station_miles,
                    "stationPercentile": scores.factors.station_percentile,
                    "gigabitPct": scores.factors.gigabit_pct,
                    "regionName": scores.factors.region_name,
                    "priorityScore": scores.factors.priority_score,
                    "imdDecile": scores.factors.imd_decile,
                    "crimeRatePer1k": scores.factors.crime_rate_per_1k,
                    "crimeRatePercentile": scores.factors.crime_rate_percentile,
                }
            },
            "liveability": {
                "score": scores.liveability,
                "factors": {
                    "gardenType": scores.factors.garden_type,
                    "heatingType": scores.factors.heating_type,
                    "petPolicy": scores.factors.pet_policy,
                    "propertyType": scores.factors.property_type,
                    "bedrooms": scores.factors.bedrooms,
                }
            }
        },
        "penalties": scores.penalties,
    });

    Ok(CommandOutput::new(data).with_text(format!("score breakdown ready for {id}")))
}

fn round_to(value: f64, decimals: usize) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    (value * factor).round() / factor
}
