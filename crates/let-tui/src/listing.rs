#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use let_sdk::intelligence::{EvidenceBundle, MediaItemEvidence};
use let_sdk::score::ScoreSummary;

#[derive(Debug, Clone, Default)]
pub(crate) struct ListingMedia {
    pub(crate) cache_dir: Option<PathBuf>,
    pub(crate) contact_sheet: Option<PathBuf>,
    pub(crate) images: Vec<PathBuf>,
    pub(crate) floorplan: Option<PathBuf>,
    pub(crate) satellite: Option<PathBuf>,
    pub(crate) street: Option<PathBuf>,
}

impl ListingMedia {
    pub(crate) fn primary_image(&self) -> Option<&PathBuf> {
        self.contact_sheet.as_ref().or_else(|| self.images.first())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TuiListingRow {
    pub(crate) id: String,
    pub(crate) entity_id: String,
    pub(crate) rightmove_id: String,
    pub(crate) url: String,
    pub(crate) google_maps_url: String,
    pub(crate) google_maps_street_view_url: String,
    pub(crate) address: String,
    pub(crate) postcode: String,
    pub(crate) region: Option<String>,
    pub(crate) price_pcm: i64,
    pub(crate) price_display: String,
    pub(crate) bedrooms: i64,
    pub(crate) bathrooms: i64,
    pub(crate) property_type: String,
    pub(crate) notes: Vec<String>,
    pub(crate) floor_area_sqm: Option<f64>,
    pub(crate) epc_rating: Option<String>,
    pub(crate) epc_remote: Option<String>,
    pub(crate) epc_address_match: Option<bool>,
    pub(crate) nearest_station_miles: Option<f64>,
    pub(crate) gigabit_availability: Option<f64>,
    pub(crate) crime_rate_per_1k: Option<f64>,
    pub(crate) imd_decile: Option<i64>,
    pub(crate) available_date: Option<String>,
    pub(crate) deposit: Option<i64>,
    pub(crate) agent_name: Option<String>,
    pub(crate) agent_phone: Option<String>,
    pub(crate) score_overall: Option<f64>,
    pub(crate) score_band: Option<String>,
    pub(crate) score_confidence: Option<String>,
    pub(crate) score_computed_at: Option<String>,
    pub(crate) generated_at: String,
    pub(crate) page_status: String,
    pub(crate) media: ListingMedia,
}

impl TuiListingRow {
    pub(crate) fn from_bundle(bundle: &EvidenceBundle, cache_root: &Path) -> Self {
        let selected_address = bundle.address.selected.as_ref();
        let address = selected_address
            .map(|candidate| candidate.label.clone())
            .or_else(|| bundle.rightmove.address.clone())
            .unwrap_or_else(|| bundle.rightmove_id.clone());
        let postcode = selected_address
            .and_then(|candidate| candidate.postcode.clone())
            .or_else(|| bundle.rightmove.postcode.clone())
            .unwrap_or_default();
        let lat = selected_address
            .and_then(|candidate| candidate.latitude)
            .or(bundle.rightmove.latitude)
            .unwrap_or_default();
        let lng = selected_address
            .and_then(|candidate| candidate.longitude)
            .or(bundle.rightmove.longitude)
            .unwrap_or_default();

        Self {
            id: bundle.rightmove_id.clone(),
            entity_id: bundle.entity_id.clone(),
            rightmove_id: bundle.rightmove_id.clone(),
            url: bundle.url.clone(),
            google_maps_url: format!("https://www.google.com/maps/search/?api=1&query={lat},{lng}"),
            google_maps_street_view_url: format!(
                "https://www.google.com/maps/@?api=1&map_action=pano&viewpoint={lat},{lng}"
            ),
            address,
            postcode,
            region: bundle.broadband.as_ref().and_then(|item| item.area.clone()),
            price_pcm: bundle.rightmove.price_pcm.unwrap_or_default(),
            price_display: bundle.rightmove.display_price.clone().unwrap_or_default(),
            bedrooms: bundle.rightmove.bedrooms.unwrap_or_default(),
            bathrooms: bundle.rightmove.bathrooms.unwrap_or_default(),
            property_type: bundle.rightmove.property_type.clone().unwrap_or_default(),
            notes: bundle.rightmove.description.key_features.clone(),
            floor_area_sqm: bundle.epc.as_ref().and_then(|epc| epc.floor_area_sqm),
            epc_rating: bundle.epc.as_ref().and_then(|epc| epc.rating.clone()),
            epc_remote: bundle
                .media
                .epc_graphs
                .first()
                .map(|item| item.remote_url.clone()),
            epc_address_match: bundle.epc.as_ref().map(|epc| epc.address_match),
            nearest_station_miles: bundle
                .rightmove
                .nearest_stations
                .first()
                .map(|station| station.distance),
            gigabit_availability: bundle
                .broadband
                .as_ref()
                .and_then(|broadband| broadband.gigabit_availability),
            crime_rate_per_1k: fact_f64(bundle, "crime", "ratePer1k"),
            imd_decile: fact_i64(bundle, "deprivation", "imdDecile"),
            available_date: bundle.rightmove.available_date.clone(),
            deposit: bundle.rightmove.deposit,
            agent_name: bundle.rightmove.agent_name.clone(),
            agent_phone: bundle.rightmove.agent_phone.clone(),
            score_overall: None,
            score_band: None,
            score_confidence: None,
            score_computed_at: None,
            generated_at: bundle.generated_at.clone(),
            page_status: bundle.rightmove.page_status.clone(),
            media: media_from_bundle(bundle, cache_root),
        }
    }

    pub(crate) fn with_score_summary(mut self, summary: Option<&ScoreSummary>) -> Self {
        if let Some(summary) = summary {
            self.score_overall = Some(summary.overall);
            self.score_band = Some(summary.band.as_str().to_owned());
            self.score_confidence = Some(summary.confidence.as_str().to_owned());
            self.score_computed_at = Some(summary.computed_at.clone());
        }
        self
    }

    pub(crate) fn matches_requested_id(&self, requested_id: &str) -> bool {
        self.id == requested_id
            || self.entity_id == requested_id
            || self.rightmove_id == requested_id
            || requested_id
                .strip_prefix("rightmove:")
                .is_some_and(|id| self.rightmove_id == id)
    }
}

fn fact_f64(bundle: &EvidenceBundle, category: &str, name: &str) -> Option<f64> {
    bundle
        .facts
        .iter()
        .find(|fact| fact.category == category && fact.name == name)
        .and_then(|fact| fact.value.as_f64())
}

fn fact_i64(bundle: &EvidenceBundle, category: &str, name: &str) -> Option<i64> {
    bundle
        .facts
        .iter()
        .find(|fact| fact.category == category && fact.name == name)
        .and_then(|fact| fact.value.as_i64())
}

fn media_from_bundle(bundle: &EvidenceBundle, cache_root: &Path) -> ListingMedia {
    let cache_dir = resolve_cache_dir(cache_root, bundle);
    let cache_dir_ref = cache_dir.as_deref();

    let contact_sheet = bundle
        .media
        .contact_sheet
        .as_ref()
        .and_then(|sheet| sheet.local_path.as_deref())
        .and_then(|path| resolve_local_asset(cache_root, cache_dir_ref, Some(path)))
        .or_else(|| cache_dir_ref.and_then(find_contact_sheet));

    let mut images = bundle
        .media
        .photos
        .iter()
        .filter_map(|item| resolve_media_item(cache_root, cache_dir_ref, item))
        .collect::<Vec<_>>();
    images.sort();
    images.dedup();

    let floorplan = bundle
        .media
        .floorplans
        .first()
        .and_then(|item| resolve_media_item(cache_root, cache_dir_ref, item));
    let satellite = bundle
        .media
        .maps
        .iter()
        .find(|item| item.kind == "mapSatellite")
        .and_then(|item| resolve_media_item(cache_root, cache_dir_ref, item));
    let street = bundle
        .media
        .maps
        .iter()
        .find(|item| item.kind == "mapStreet")
        .and_then(|item| resolve_media_item(cache_root, cache_dir_ref, item));

    ListingMedia {
        cache_dir,
        contact_sheet,
        images,
        floorplan,
        satellite,
        street,
    }
}

fn resolve_cache_dir(cache_root: &Path, bundle: &EvidenceBundle) -> Option<PathBuf> {
    [
        cache_root.join(&bundle.rightmove_id),
        cache_root.join(&bundle.entity_id),
    ]
    .into_iter()
    .find(|path| path.exists())
}

fn resolve_media_item(
    cache_root: &Path,
    cache_dir: Option<&Path>,
    item: &MediaItemEvidence,
) -> Option<PathBuf> {
    resolve_local_asset(cache_root, cache_dir, item.local_path.as_deref())
}

fn resolve_local_asset(
    cache_root: &Path,
    cache_dir: Option<&Path>,
    local: Option<&str>,
) -> Option<PathBuf> {
    let raw = local?.trim();
    if raw.is_empty() {
        return None;
    }

    let direct = PathBuf::from(raw);
    if direct.is_absolute() {
        return direct.exists().then_some(direct);
    }

    let mut candidates = Vec::new();
    if let Some(dir) = cache_dir {
        candidates.push(dir.join(raw));
    }
    candidates.push(cache_root.join(raw));

    candidates.into_iter().find(|path| path.exists())
}

fn find_contact_sheet(cache_dir: &Path) -> Option<PathBuf> {
    let mut sheets = fs::read_dir(cache_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains("-contact-sheet-") && name.ends_with(".jpg"))
        })
        .collect::<Vec<_>>();
    sheets.sort();
    sheets.pop()
}
