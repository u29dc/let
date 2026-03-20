#![forbid(unsafe_code)]

use std::cmp::Ordering;

use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use let_sdk::schema::listing::Listing;
use let_sdk::{ListingSummary, find_listing_by_id_from_db, load_listing_summaries};
use serde::Serialize;
use serde_json::{Value, json};

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
    let text = render_listing_list_text(&listings, filtered, total);

    Ok(CommandOutput::new(json!({
        "listings": projection,
        "total": total,
        "filtered": filtered,
    }))
    .with_count(filtered)
    .with_total(total)
    .with_has_more(filtered < total)
    .with_text(text.clone())
    .with_copy_text(text))
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

    let text = render_listing_detail_text(&listing);
    let listing_json = to_camel_json(&listing);

    Ok(
        CommandOutput::new(json!({ "listing": listing_json.clone() }))
            .with_text(text.clone())
            .with_copy_text(text)
            .with_copy_json(listing_json),
    )
}

fn render_listing_list_text(listings: &[ListingSummary], shown: usize, total: usize) -> String {
    if listings.is_empty() {
        return if total == 0 {
            "No listings available.".to_owned()
        } else {
            format!("No listings matched filters. Showing 0 of {total} listings.")
        };
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["ID", "Score", "Beds", "Price", "Region", "Address"]);

    for listing in listings {
        let id = listing
            .portal_rightmove
            .as_deref()
            .unwrap_or(listing.id.as_str());
        let final_score = listing.assessed_score.or(listing.score);
        table.add_row(vec![
            id.to_owned(),
            format_optional_f64(final_score),
            listing.bedrooms.to_string(),
            listing.price_display.clone(),
            listing.region.clone().unwrap_or_else(|| "-".to_owned()),
            truncate(&listing.address, 44),
        ]);
    }

    format!("Showing {shown} of {total} listings\n\n{table}")
}

fn render_listing_detail_text(listing: &Listing) -> String {
    let listing_id = listing
        .portal_ids
        .rightmove
        .as_deref()
        .unwrap_or(listing.id.as_str());

    let mut lines = Vec::new();
    lines.push(listing.address.clone());

    let mut headline = vec![
        listing.price_display.clone(),
        format!("{} bed", listing.bedrooms),
        format!("{} bath", listing.bathrooms),
        listing.property_type.clone(),
    ];
    if let Some(available) = &listing.lettings.available_date {
        headline.push(format!("Available {available}"));
    }
    lines.push(headline.join(" | "));

    let mut meta = vec![
        format!("ID: {listing_id}"),
        format!("Status: {}", humanize_serialized(&listing.status)),
    ];
    if let Some(region) = &listing.region {
        meta.push(format!("Region: {region}"));
    }
    if let Some(listed_date) = &listing.listed_date {
        meta.push(format!("Listed: {listed_date}"));
    }
    lines.push(meta.join(" | "));

    push_section(&mut lines, "Scores", render_scores_section(listing));
    push_section(&mut lines, "Property", render_property_section(listing));
    push_section(&mut lines, "Area", render_area_section(listing));
    push_section(&mut lines, "Assessment", render_assessment_section(listing));
    push_section(&mut lines, "Links", render_links_section(listing));

    lines.join("\n")
}

fn render_scores_section(listing: &Listing) -> Vec<String> {
    let mut lines = Vec::new();
    let mut overview = Vec::new();

    if let Some(scores) = &listing.scores {
        overview.push(format!("Algorithm: {:.1}", scores.overall));
        overview.push(format!("Confidence: {:.2}", scores.confidence));
    }
    if let Some(assessed_score) = listing.assessed_score {
        overview.push(format!("Assessed: {:.1}", assessed_score));
    }

    if !overview.is_empty() {
        lines.push(overview.join(" | "));
    }

    if let Some(scores) = &listing.scores {
        lines.push(format!(
            "Affordability: {:.1} | Location: {:.1} | Liveability: {:.1}",
            scores.affordability, scores.location, scores.liveability
        ));
    }

    lines
}

fn render_property_section(listing: &Listing) -> Vec<String> {
    let mut lines = Vec::new();

    let mut primary = Vec::new();
    if let Some(deposit) = listing.lettings.deposit {
        primary.push(format!("Deposit: {}", format_gbp(deposit)));
    }
    if let Some(epc) = &listing.epc_rating {
        primary.push(format!("EPC: {}", humanize_serialized(epc)));
    }
    if let Some(area) = listing.floor_area_sqm {
        primary.push(format!("Floor area: {:.1} sqm", area));
    }
    if !primary.is_empty() {
        lines.push(primary.join(" | "));
    }

    let mut agent = Vec::new();
    if let Some(name) = &listing.agent.name {
        agent.push(name.clone());
    }
    if let Some(phone) = &listing.agent.phone {
        agent.push(phone.clone());
    }
    if !agent.is_empty() {
        lines.push(format!("Agent: {}", agent.join(" | ")));
    }

    let mut assets = Vec::new();
    assets.push(format!("Photos: {}", listing.images.len()));
    assets.push(format!(
        "Floorplan: {}",
        yes_no(listing.floorplan.remote.is_some() || listing.floorplan.local.is_some())
    ));
    assets.push(format!(
        "EPC asset: {}",
        yes_no(listing.epc.remote.is_some() || listing.epc.local.is_some())
    ));
    lines.push(assets.join(" | "));

    if !listing.notes.is_empty() {
        lines.push(format!("Notes: {}", listing.notes.join(", ")));
    }

    lines
}

fn render_area_section(listing: &Listing) -> Vec<String> {
    let mut lines = Vec::new();

    let mut transport = Vec::new();
    if let Some(gigabit) = listing.gigabit_availability {
        transport.push(format!("Broadband: {:.1}%", gigabit));
    }
    if let Some(station) = listing.nearest_stations.first() {
        transport.push(format!(
            "Nearest station: {} ({:.1} {})",
            station.name, station.distance, station.unit
        ));
    }
    if !transport.is_empty() {
        lines.push(transport.join(" | "));
    }

    let mut crime = Vec::new();
    if let Some(rate) = listing.area.crime.rate_per_1k {
        crime.push(format!("Crime: {:.2}/1k", rate));
    }
    if let Some(count) = listing.area.crime.count_12m {
        crime.push(format!("12m count: {}", format_int(count)));
    }
    if let Some(updated_at) = &listing.area.crime.updated_at {
        crime.push(format!("Updated: {updated_at}"));
    }
    if !crime.is_empty() {
        lines.push(crime.join(" | "));
    }

    let mut imd = Vec::new();
    if let Some(decile) = listing.area.imd.decile {
        imd.push(format!("IMD decile: {decile}"));
    }
    if let Some(rank) = listing.area.imd.rank {
        imd.push(format!("IMD rank: {}", format_int(rank)));
    }
    if let Some(score) = listing.area.imd.score {
        imd.push(format!("IMD score: {:.2}", score));
    }
    if !imd.is_empty() {
        lines.push(imd.join(" | "));
    }

    if let Some(level) = &listing.area.flood_risk.level {
        lines.push(format!("Flood risk: {level}"));
    }

    lines
}

fn render_assessment_section(listing: &Listing) -> Vec<String> {
    let Some(assessment) = &listing.assessment else {
        return Vec::new();
    };

    let mut lines = Vec::new();
    lines.push(format!(
        "Recommendation: {} ({})",
        humanize_serialized(&assessment.recommendation),
        format_signed_f64(assessment.score_adjustment)
    ));
    lines.push(format!(
        "Family suitability: {} | Maintenance: {}",
        humanize_serialized(&assessment.family_suitability),
        humanize_serialized(&assessment.maintenance)
    ));
    lines.push(format!("Light and space: {}", assessment.light_and_space));
    lines.push(format!("Photos: {}", assessment.photo_analysis));
    if let Some(neighborhood) = &assessment.neighborhood_analysis {
        lines.push(format!("Neighborhood: {neighborhood}"));
    }
    if let Some(tradeoffs) = &assessment.tradeoffs {
        lines.push(format!("Tradeoffs: {tradeoffs}"));
    }
    lines.push(format!("Reasoning: {}", assessment.reasoning));

    lines
}

fn render_links_section(listing: &Listing) -> Vec<String> {
    let mut lines = vec![format!("Listing: {}", listing.url)];
    lines.push(format!("Maps: {}", listing.google_maps_url));
    lines.push(format!(
        "Street view: {}",
        listing.google_maps_street_view_url
    ));
    if let Some(floorplan) = &listing.floorplan.remote {
        lines.push(format!("Floorplan: {floorplan}"));
    }
    if let Some(epc_search_url) = &listing.epc_search_url {
        lines.push(format!("EPC search: {epc_search_url}"));
    }
    lines
}

fn push_section(lines: &mut Vec<String>, title: &str, section_lines: Vec<String>) {
    if section_lines.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(title.to_owned());
    lines.extend(section_lines);
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

fn format_optional_f64(value: Option<f64>) -> String {
    value
        .map(|inner| format!("{inner:.1}"))
        .unwrap_or_else(|| "-".to_owned())
}

fn format_signed_f64(value: f64) -> String {
    if value >= 0.0 {
        format!("+{value:.1}")
    } else {
        format!("{value:.1}")
    }
}

fn format_gbp(value: i64) -> String {
    format!("£{}", format_int(value))
}

fn format_int<T>(value: T) -> String
where
    T: Into<i64>,
{
    let raw = value.into().to_string();
    let (sign, digits) = if let Some(rest) = raw.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", raw.as_str())
    };

    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index != 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }

    let grouped = out.chars().rev().collect::<String>();
    format!("{sign}{grouped}")
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn humanize_serialized<T>(value: &T) -> String
where
    T: Serialize,
{
    match serde_json::to_value(value) {
        Ok(Value::String(raw)) => humanize_token(&raw),
        _ => "-".to_owned(),
    }
}

fn humanize_token(raw: &str) -> String {
    raw.replace(['-', '_'], " ")
        .split_whitespace()
        .map(title_case_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_case_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let rest = chars.collect::<String>().to_lowercase();
    format!("{}{}", first.to_uppercase(), rest)
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
