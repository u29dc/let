#![forbid(unsafe_code)]

use std::cmp::Ordering;

use let_sdk::{ListingSummary, find_listing_by_id_from_db, load_listing_summaries};
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Score,
    Price,
    Bedrooms,
    Date,
}

impl SortField {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "price" => Self::Price,
            "bedrooms" => Self::Bedrooms,
            "date" => Self::Date,
            _ => Self::Score,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ViewListParams {
    pub top: usize,
    pub min_score: Option<f64>,
    pub sort: SortField,
    pub asc: bool,
    pub region: Option<String>,
    pub property_type: Option<String>,
}

pub fn list(shared: &SharedArgs, params: &ViewListParams) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let mut listings = load_listing_summaries(&db_path)?;
    let total = listings.len();

    if let Some(region) = &params.region {
        let region_lower = region.to_lowercase();
        listings.retain(|listing| {
            listing
                .region
                .as_deref()
                .is_some_and(|value| value.to_lowercase().contains(&region_lower))
        });
    }

    if let Some(property_type) = &params.property_type {
        let types = property_type
            .split(',')
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();

        if !types.is_empty() {
            listings.retain(|listing| {
                let candidate = listing.property_type.to_lowercase();
                types.iter().any(|value| candidate.contains(value))
            });
        }
    }

    if let Some(min_score) = params.min_score {
        listings.retain(|listing| listing.score.is_some_and(|score| score >= min_score));
    }

    listings.sort_by(|a, b| compare_listings(a, b, params.sort, params.asc));

    if listings.len() > params.top {
        listings.truncate(params.top);
    }

    let projection = listings
        .iter()
        .map(|listing| {
            let score_change = listing
                .assessed_score
                .zip(listing.score)
                .map(|(assessed, algo)| {
                    let delta = assessed - algo;
                    (delta * 10.0).round() / 10.0
                });
            json!({
                "id": listing.portal_rightmove.clone().unwrap_or_else(|| listing.id.clone()),
                "portalId": listing.portal_rightmove,
                "address": listing.address,
                "price": listing.price,
                "priceDisplay": listing.price_display,
                "bedrooms": listing.bedrooms,
                "score": listing.score,
                "assessedScore": listing.assessed_score,
                "scoreChange": score_change,
                "region": listing.region,
                "station": format_station_summary(listing),
                "url": listing.url,
            })
        })
        .collect::<Vec<_>>();

    let filtered = projection.len();
    Ok(CommandOutput::new(json!({
        "listings": projection,
        "total": total,
        "filtered": filtered,
    }))
    .with_count(filtered)
    .with_total(total)
    .with_has_more(filtered < total))
}

pub fn detail(shared: &SharedArgs, id: &str) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let Some(listing) = find_listing_by_id_from_db(&db_path, id)? else {
        return Err(CommandError::new(
            "NOT_FOUND",
            format!("listing not found: {id}"),
            "check id using `let view list`",
            1,
        ));
    };

    let listing_json = to_camel_json(&listing);

    Ok(CommandOutput::new(json!({ "listing": listing_json.clone() })).with_copy_json(listing_json))
}

fn compare_listings(
    a: &ListingSummary,
    b: &ListingSummary,
    sort: SortField,
    asc: bool,
) -> Ordering {
    let ordering = match sort {
        SortField::Score => {
            let a_score = a.assessed_score.or(a.score).unwrap_or(0.0);
            let b_score = b.assessed_score.or(b.score).unwrap_or(0.0);
            a_score.partial_cmp(&b_score).unwrap_or(Ordering::Equal)
        }
        SortField::Price => a.price.cmp(&b.price),
        SortField::Bedrooms => a.bedrooms.cmp(&b.bedrooms),
        SortField::Date => a.listed_date.cmp(&b.listed_date),
    };

    if asc { ordering } else { ordering.reverse() }
}

fn format_station_summary(listing: &ListingSummary) -> Option<String> {
    let name = listing.first_station_name.as_deref()?;
    let distance = listing.first_station_distance?;
    let name = truncate(name, 25);
    Some(format!("{name} ({distance:.1}mi)"))
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        value.to_owned()
    } else if max_len <= 3 {
        value.chars().take(max_len).collect()
    } else {
        let head = value.chars().take(max_len - 3).collect::<String>();
        format!("{head}...")
    }
}
