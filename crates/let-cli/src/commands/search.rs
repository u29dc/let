#![forbid(unsafe_code)]

use std::collections::HashSet;

use let_sdk::load_listings_file;
use serde_json::json;

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs};

pub fn diff(shared: &SharedArgs, ids_raw: &str) -> CommandResult {
    let input_ids = parse_ids(ids_raw);
    if input_ids.is_empty() {
        return Err(CommandError::runtime(
            "VALIDATION_ERROR",
            "no ids provided",
            "provide comma-separated portal ids",
        ));
    }

    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;

    let known_ids = load_listings_file(&db_path)
        .map(|data| known_portal_ids(&data.listings))
        .unwrap_or_default();

    let mut known = Vec::new();
    let mut new_ids = Vec::new();
    for id in &input_ids {
        if known_ids.contains(id) {
            known.push(id.clone());
        } else {
            new_ids.push(id.clone());
        }
    }

    Ok(CommandOutput::new(json!({
        "new": new_ids,
        "known": known,
        "total": input_ids.len(),
    }))
    .with_count(input_ids.len())
    .with_total(input_ids.len())
    .with_has_more(false)
    .with_text(format!(
        "{} ids checked: {} new, {} known",
        input_ids.len(),
        input_ids.len().saturating_sub(known.len()),
        known.len()
    )))
}

fn parse_ids(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn known_portal_ids(listings: &[let_sdk::schema::listing::Listing]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for listing in listings {
        if let Some(id) = listing.portal_ids.rightmove.as_ref() {
            ids.insert(id.clone());
        }
        if let Some(id) = listing.portal_ids.zoopla.as_ref() {
            ids.insert(id.clone());
        }
        if let Some(id) = listing.portal_ids.onthemarket.as_ref() {
            ids.insert(id.clone());
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::parse_ids;

    #[test]
    fn parse_ids_filters_empty_items() {
        let ids = parse_ids("1701, 1702, ,1703");
        assert_eq!(ids, vec!["1701", "1702", "1703"]);
    }
}
