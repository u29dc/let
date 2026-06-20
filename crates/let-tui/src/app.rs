#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use let_sdk::intelligence::{EvidenceBundle, IntelligenceDb};
use let_sdk::load_listings_file;
use let_sdk::schema::listing::{
    Agent, AreaMetrics, ExtractionStatus, GeoLocation, Lettings, Listing, ListingImage,
    ListingStatus, MapViews, PortalIds, RemoteLocalAsset,
};
use ratatui::layout::Rect;
use ratatui_image::thread::ThreadProtocol;

use crate::preview::{PreviewAssetKind, PreviewController, PreviewTarget, PreviewView};
use crate::theme::{HEADER_CONTRACT, HeaderContract};

const SOURCE_NAMES: [&str; 10] = [
    "broadband",
    "postcodes",
    "deprivation",
    "census",
    "population",
    "income",
    "flood",
    "naptan",
    "uprn",
    "crime",
];

#[derive(Debug, Clone)]
pub(crate) struct SourceStatus {
    pub(crate) name: String,
    pub(crate) exists: bool,
    pub(crate) size_mb: f64,
}

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
    fn primary_image(&self) -> Option<&PathBuf> {
        self.contact_sheet.as_ref().or_else(|| self.images.first())
    }
}

#[derive(Debug, Clone)]
struct PaletteAction {
    label: String,
    kind: PaletteActionKind,
}

#[derive(Debug, Clone)]
enum PaletteActionKind {
    OpenUrl(String),
    QuickLook(PathBuf),
    RevealInFinder(PathBuf),
    Refresh,
    BuildSources(&'static str),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusPane {
    Listings,
    Context,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextMediaItem {
    pub(crate) kind: String,
    pub(crate) asset: String,
    pub(crate) path: PathBuf,
    pub(crate) asset_kind: PreviewAssetKind,
}

pub(crate) struct App {
    running: bool,
    listings: Vec<Listing>,
    selected: usize,
    focus: FocusPane,
    context_selected: usize,
    context_offset: usize,
    selected_media: ListingMedia,
    context_items: Vec<ContextMediaItem>,
    status: String,
    header: HeaderContract,
    palette_open: bool,
    palette_query: String,
    palette_selected: usize,
    palette_actions: Vec<PaletteAction>,
    palette_filtered: Vec<usize>,
    source_status: Vec<SourceStatus>,
    source_build: Option<SourceBuildJob>,
    preview: PreviewController,
}

struct SourceBuildJob {
    target: String,
    receiver: Receiver<String>,
}

impl App {
    pub(crate) fn with_preview(preview: PreviewController) -> Self {
        let (listings, status) = load_ranked_listings();
        let selected = initial_selection(&listings, env::var("LET_START_ID").ok().as_deref());
        let focus = if env::var("LET_START_SECTIONS")
            .ok()
            .is_some_and(|sections| sections.split(',').any(|section| section == "media"))
        {
            FocusPane::Context
        } else {
            FocusPane::Listings
        };
        let mut app = Self {
            running: true,
            listings,
            selected,
            focus,
            context_selected: 0,
            context_offset: 0,
            selected_media: ListingMedia::default(),
            context_items: Vec::new(),
            status,
            header: HEADER_CONTRACT,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            palette_actions: Vec::new(),
            palette_filtered: Vec::new(),
            source_status: collect_source_status(),
            source_build: None,
            preview,
        };
        app.rebuild_context_cache(true);
        app
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running
    }

    pub(crate) fn tick(&mut self) {
        self.preview.tick();
        self.poll_source_build();
    }

    pub(crate) fn poll_timeout_ms(&self) -> u64 {
        if self.preview.needs_fast_tick() {
            60
        } else {
            200
        }
    }

    pub(crate) fn listings(&self) -> &[Listing] {
        &self.listings
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected
    }

    pub(crate) fn focus(&self) -> FocusPane {
        self.focus
    }

    pub(crate) fn status(&self) -> &str {
        &self.status
    }

    pub(crate) fn selected_listing(&self) -> Option<&Listing> {
        self.listings.get(self.selected)
    }

    pub(crate) fn selected_media(&self) -> &ListingMedia {
        &self.selected_media
    }

    pub(crate) fn source_health_counts(&self) -> (usize, usize) {
        let ready = self.source_status.iter().filter(|item| item.exists).count();
        (ready, self.source_status.len())
    }

    pub(crate) fn source_summary(&self) -> String {
        self.source_status
            .iter()
            .map(|item| {
                if item.exists {
                    format!("{}:{:.0}MB", item.name, item.size_mb)
                } else {
                    format!("{}:missing", item.name)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(crate) fn header_text(&self) -> String {
        self.header.render()
    }

    pub(crate) fn route_context(&self) -> String {
        let total = self.listings.len();
        if total == 0 {
            return "search/listings 0/0".to_owned();
        }
        format!("search/listings {}/{}", self.selected + 1, total)
    }

    pub(crate) fn palette_open(&self) -> bool {
        self.palette_open
    }

    pub(crate) fn palette_query(&self) -> &str {
        &self.palette_query
    }

    pub(crate) fn palette_rows(&self) -> Vec<String> {
        self.palette_filtered
            .iter()
            .filter_map(|index| self.palette_actions.get(*index))
            .map(|action| action.label.clone())
            .collect()
    }

    pub(crate) fn palette_selected_index(&self) -> usize {
        self.palette_selected
    }

    pub(crate) fn context_items(&self) -> &[ContextMediaItem] {
        &self.context_items
    }

    pub(crate) fn context_selected_index(&self) -> usize {
        self.context_selected
    }

    pub(crate) fn context_scroll_offset(&self) -> usize {
        self.context_offset
    }

    pub(crate) fn set_context_scroll_offset(&mut self, offset: usize) {
        self.context_offset = offset;
    }

    pub(crate) fn preview_mode_label(&self) -> &'static str {
        self.preview.mode().label()
    }

    pub(crate) fn preview_preferred_block_height(&self, block_width: u16) -> u16 {
        self.preview.preferred_block_height(block_width)
    }

    pub(crate) fn sync_preview(&mut self, area: Rect) {
        let (target, empty_message) = self.preview_target();
        self.preview.sync(target, area, empty_message);
    }

    pub(crate) fn preview_view(&self) -> PreviewView {
        self.preview.view()
    }

    pub(crate) fn preview_protocol_mut(&mut self) -> Option<&mut ThreadProtocol> {
        self.preview.protocol_mut()
    }

    pub(crate) fn on_key(&mut self, key: KeyEvent) {
        if self.palette_open {
            self.on_palette_key(key);
            return;
        }
        if is_palette_trigger(key) {
            self.open_palette();
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.running = false;
            }
            KeyCode::Tab => self.toggle_focus(),
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                FocusPane::Listings => self.select_next(),
                FocusPane::Context => self.context_next(),
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                FocusPane::Listings => self.select_prev(),
                FocusPane::Context => self.context_prev(),
            },
            KeyCode::Char('g') | KeyCode::Home => match self.focus {
                FocusPane::Listings => self.select_first(),
                FocusPane::Context => self.context_first(),
            },
            KeyCode::Char('G') | KeyCode::End => match self.focus {
                FocusPane::Listings => self.select_last(),
                FocusPane::Context => self.context_last(),
            },
            KeyCode::Char('m') | KeyCode::Char('M') => self.cycle_preview_mode(),
            KeyCode::Char('r') | KeyCode::Char('R') => self.refresh_all(),
            KeyCode::Char(':') => self.open_palette(),
            KeyCode::Enter => match self.focus {
                FocusPane::Listings => self.open_selected_on_rightmove(),
                FocusPane::Context => self.quicklook_selected_context_media(),
            },
            _ => {}
        }
    }

    fn on_palette_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_palette(),
            KeyCode::Enter => self.execute_selected_palette_action(),
            KeyCode::Up => self.palette_prev(),
            KeyCode::Down => self.palette_next(),
            KeyCode::Home => self.palette_selected = 0,
            KeyCode::End => {
                if !self.palette_filtered.is_empty() {
                    self.palette_selected = self.palette_filtered.len().saturating_sub(1);
                }
            }
            KeyCode::Backspace => {
                self.palette_query.pop();
                self.rebuild_palette_actions();
            }
            KeyCode::Char(ch) if !ch.is_control() => {
                self.palette_query.push(ch);
                self.rebuild_palette_actions();
            }
            _ => {}
        }
    }

    fn open_palette(&mut self) {
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.rebuild_palette_actions();
        self.status = "palette opened".to_owned();
    }

    fn close_palette(&mut self) {
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.palette_actions.clear();
        self.palette_filtered.clear();
        self.status = "palette closed".to_owned();
    }

    fn rebuild_palette_actions(&mut self) {
        self.palette_actions = self.build_palette_actions();
        self.palette_filtered = filtered_action_indices(&self.palette_actions, &self.palette_query);
        self.clamp_palette_selection();
    }

    fn clamp_palette_selection(&mut self) {
        if self.palette_filtered.is_empty() {
            self.palette_selected = 0;
            return;
        }
        if self.palette_selected >= self.palette_filtered.len() {
            self.palette_selected = self.palette_filtered.len().saturating_sub(1);
        }
    }

    fn palette_next(&mut self) {
        if self.palette_filtered.is_empty() {
            self.palette_selected = 0;
            return;
        }
        self.palette_selected =
            (self.palette_selected + 1).min(self.palette_filtered.len().saturating_sub(1));
    }

    fn palette_prev(&mut self) {
        self.palette_selected = self.palette_selected.saturating_sub(1);
    }

    fn build_palette_actions(&self) -> Vec<PaletteAction> {
        let mut actions = Vec::new();

        if let Some(listing) = self.selected_listing() {
            actions.push(PaletteAction {
                label: "open on rightmove".to_owned(),
                kind: PaletteActionKind::OpenUrl(listing.url.clone()),
            });
            actions.push(PaletteAction {
                label: "open on google maps".to_owned(),
                kind: PaletteActionKind::OpenUrl(listing.google_maps_url.clone()),
            });
            actions.push(PaletteAction {
                label: "open on google street view".to_owned(),
                kind: PaletteActionKind::OpenUrl(listing.google_maps_street_view_url.clone()),
            });

            let media = self.selected_media();
            if let Some(path) = media.primary_image().cloned() {
                actions.push(PaletteAction {
                    label: if media.contact_sheet.is_some() {
                        "quick look contact sheet".to_owned()
                    } else {
                        "quick look first image".to_owned()
                    },
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.floorplan.clone() {
                actions.push(PaletteAction {
                    label: "quick look floorplan".to_owned(),
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.satellite.clone() {
                actions.push(PaletteAction {
                    label: "quick look satellite map".to_owned(),
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.street.clone() {
                actions.push(PaletteAction {
                    label: "quick look street map".to_owned(),
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.cache_dir.clone() {
                actions.push(PaletteAction {
                    label: "reveal cache folder".to_owned(),
                    kind: PaletteActionKind::RevealInFinder(path),
                });
            }
        }

        actions.push(PaletteAction {
            label: "refresh".to_owned(),
            kind: PaletteActionKind::Refresh,
        });
        actions.push(PaletteAction {
            label: "sources build all".to_owned(),
            kind: PaletteActionKind::BuildSources("all"),
        });
        actions.push(PaletteAction {
            label: "sources build broadband".to_owned(),
            kind: PaletteActionKind::BuildSources("broadband"),
        });
        actions.push(PaletteAction {
            label: "sources build crime".to_owned(),
            kind: PaletteActionKind::BuildSources("crime"),
        });
        actions.push(PaletteAction {
            label: "sources build income".to_owned(),
            kind: PaletteActionKind::BuildSources("income"),
        });
        actions.push(PaletteAction {
            label: "quit".to_owned(),
            kind: PaletteActionKind::Quit,
        });

        actions
    }

    fn execute_selected_palette_action(&mut self) {
        let Some(source_index) = self.palette_filtered.get(self.palette_selected).copied() else {
            self.status = "no palette command selected".to_owned();
            return;
        };
        let Some(action) = self.palette_actions.get(source_index).cloned() else {
            self.status = "no palette command selected".to_owned();
            return;
        };

        match action.kind {
            PaletteActionKind::OpenUrl(url) => self.open_url(&url),
            PaletteActionKind::QuickLook(path) => self.quicklook_path(&path),
            PaletteActionKind::RevealInFinder(path) => self.reveal_path(&path),
            PaletteActionKind::Refresh => self.refresh_all(),
            PaletteActionKind::BuildSources(target) => self.build_sources(target),
            PaletteActionKind::Quit => {
                self.running = false;
                self.status = "quitting".to_owned();
            }
        }

        if self.running {
            let action_status = self.status.clone();
            self.close_palette();
            self.status = action_status;
        }
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Listings => FocusPane::Context,
            FocusPane::Context => FocusPane::Listings,
        };
        self.clamp_context_selection();
        self.status = match self.focus {
            FocusPane::Listings => "focus: listings".to_owned(),
            FocusPane::Context => "focus: context".to_owned(),
        };
    }

    fn context_next(&mut self) {
        let len = self.context_items.len();
        if len == 0 {
            self.context_selected = 0;
            return;
        }
        self.context_selected = (self.context_selected + 1).min(len.saturating_sub(1));
    }

    fn context_prev(&mut self) {
        self.context_selected = self.context_selected.saturating_sub(1);
    }

    fn context_first(&mut self) {
        self.context_selected = 0;
    }

    fn context_last(&mut self) {
        let len = self.context_items.len();
        if len == 0 {
            self.context_selected = 0;
            return;
        }
        self.context_selected = len.saturating_sub(1);
    }

    fn clamp_context_selection(&mut self) {
        let len = self.context_items.len();
        if len == 0 {
            self.context_selected = 0;
            self.context_offset = 0;
            return;
        }
        if self.context_selected >= len {
            self.context_selected = len.saturating_sub(1);
        }
    }

    fn open_selected_on_rightmove(&mut self) {
        let Some(listing) = self.selected_listing() else {
            self.status = "no listing selected".to_owned();
            return;
        };
        let url = listing.url.clone();
        self.open_url(&url);
    }

    fn quicklook_selected_context_media(&mut self) {
        let Some(item) = self.context_items.get(self.context_selected) else {
            self.status = "no media selected".to_owned();
            return;
        };
        let path = item.path.clone();
        self.quicklook_path(&path);
    }

    fn open_url(&mut self, url: &str) {
        match open_url_with_system(url) {
            Ok(()) => {
                self.status = format!("opened url: {url}");
            }
            Err(error) => {
                self.status = format!("open url failed: {error}");
            }
        }
    }

    fn quicklook_path(&mut self, path: &Path) {
        if !path.exists() {
            self.status = format!("file not found: {}", path.display());
            return;
        }

        match quicklook_with_system(path) {
            Ok(()) => {
                self.status = format!("quick look: {}", path.display());
            }
            Err(error) => {
                self.status = format!("quick look failed: {error}");
            }
        }
    }

    fn reveal_path(&mut self, path: &Path) {
        match reveal_in_finder(path) {
            Ok(()) => {
                self.status = format!("revealed: {}", path.display());
            }
            Err(error) => {
                self.status = format!("reveal failed: {error}");
            }
        }
    }

    fn build_sources(&mut self, target: &str) {
        if let Some(job) = &self.source_build {
            self.status = format!("sources build {} already running", job.target);
            return;
        }

        let binary = resolve_cli_binary();
        let target = target.to_owned();
        let (sender, receiver) = mpsc::channel();
        self.status = format!("sources build {target} started via {}", binary.display());
        self.source_build = Some(SourceBuildJob {
            target: target.clone(),
            receiver,
        });

        thread::spawn(move || {
            let status = run_source_build(&binary, &target);
            let _ = sender.send(status);
        });
    }

    fn poll_source_build(&mut self) {
        let completed = match self.source_build.as_ref() {
            Some(job) => match job.receiver.try_recv() {
                Ok(status) => Some(status),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(format!(
                    "sources build {} failed (worker disconnected)",
                    job.target
                )),
            },
            None => None,
        };

        if let Some(status) = completed {
            self.source_build = None;
            self.status = status;
            self.refresh_sources();
        }
    }

    fn select_next(&mut self) {
        if self.listings.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.listings.len().saturating_sub(1));
        self.rebuild_context_cache(true);
    }

    fn select_prev(&mut self) {
        if self.listings.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
        self.rebuild_context_cache(true);
    }

    fn select_first(&mut self) {
        if !self.listings.is_empty() {
            self.selected = 0;
            self.rebuild_context_cache(true);
        }
    }

    fn select_last(&mut self) {
        if !self.listings.is_empty() {
            self.selected = self.listings.len().saturating_sub(1);
            self.rebuild_context_cache(true);
        }
    }

    fn refresh_all(&mut self) {
        let (listings, status) = load_ranked_listings();
        self.listings = listings;
        if self.selected >= self.listings.len() {
            self.selected = self.listings.len().saturating_sub(1);
        }
        self.rebuild_context_cache(true);
        self.status = status;
        self.refresh_sources();
    }

    fn refresh_sources(&mut self) {
        self.source_status = collect_source_status();
    }

    fn rebuild_context_cache(&mut self, reset_selection: bool) {
        let (selected_media, context_items) = if let Some(listing) = self.selected_listing() {
            let paths = let_sdk::paths::paths();
            let cache_root = paths.resolved.cache.as_path();
            let selected_media = build_media_index(cache_root, listing);
            let context_items = build_context_media_items(listing, &selected_media);
            (selected_media, context_items)
        } else {
            (ListingMedia::default(), Vec::new())
        };

        self.selected_media = selected_media;
        self.context_items = context_items;

        if reset_selection {
            self.context_selected = 0;
            self.context_offset = 0;
        }
        self.clamp_context_selection();
    }

    fn preview_target(&self) -> (Option<PreviewTarget>, &'static str) {
        if self.selected_listing().is_none() {
            return (None, "select a listing to preview");
        }
        if self.context_items.is_empty() {
            return (None, "no cached media for this listing");
        }

        let item = match self.focus {
            FocusPane::Listings => select_primary_preview_item(&self.context_items),
            FocusPane::Context => self
                .context_items
                .get(self.context_selected)
                .or_else(|| select_primary_preview_item(&self.context_items)),
        };

        let Some(item) = item else {
            return (None, "no cached media for this listing");
        };

        (
            Some(PreviewTarget::new(
                item.path.clone(),
                item.asset_kind,
                item.kind.clone(),
            )),
            "no cached media for this listing",
        )
    }

    fn cycle_preview_mode(&mut self) {
        self.preview.cycle_mode();
        self.status = format!("preview mode: {}", self.preview_mode_label());
    }
}

impl Default for App {
    fn default() -> Self {
        Self::with_preview(PreviewController::disabled("preview unavailable in tests"))
    }
}

fn select_primary_preview_item(items: &[ContextMediaItem]) -> Option<&ContextMediaItem> {
    items
        .iter()
        .find(|item| item.asset_kind == PreviewAssetKind::Photo)
        .or_else(|| items.first())
}

fn build_context_media_items(listing: &Listing, media: &ListingMedia) -> Vec<ContextMediaItem> {
    let listing_key = listing
        .portal_ids
        .rightmove
        .as_deref()
        .unwrap_or(listing.id.as_str())
        .to_owned();
    let mut items = Vec::new();

    if let Some(path) = &media.contact_sheet {
        items.push(ContextMediaItem {
            kind: "contact-sheet".to_owned(),
            asset: compact_media_asset(&listing_key, path),
            path: path.clone(),
            asset_kind: PreviewAssetKind::Photo,
        });
    }

    for (index, path) in media.images.iter().enumerate() {
        items.push(ContextMediaItem {
            kind: format!("img_{:02}", index + 1),
            asset: compact_media_asset(&listing_key, path),
            path: path.clone(),
            asset_kind: PreviewAssetKind::Photo,
        });
    }
    if let Some(path) = &media.floorplan {
        items.push(ContextMediaItem {
            kind: "floorplan".to_owned(),
            asset: compact_media_asset(&listing_key, path),
            path: path.clone(),
            asset_kind: PreviewAssetKind::Floorplan,
        });
    }
    if let Some(path) = &media.satellite {
        items.push(ContextMediaItem {
            kind: "satellite-map".to_owned(),
            asset: compact_media_asset(&listing_key, path),
            path: path.clone(),
            asset_kind: PreviewAssetKind::Satellite,
        });
    }
    if let Some(path) = &media.street {
        items.push(ContextMediaItem {
            kind: "street-map".to_owned(),
            asset: compact_media_asset(&listing_key, path),
            path: path.clone(),
            asset_kind: PreviewAssetKind::Street,
        });
    }

    items
}

fn load_ranked_listings() -> (Vec<Listing>, String) {
    let paths = let_sdk::paths::paths();
    let db_path = paths.derived.database;

    match IntelligenceDb::open_readonly(&db_path).and_then(|db| db.load_bundles()) {
        Ok(bundles) if !bundles.is_empty() => {
            let mut listings = bundles.iter().map(listing_from_bundle).collect::<Vec<_>>();
            let listing_count = listings.len();
            listings.sort_by(|a, b| {
                let left = a.assessed_score.unwrap_or(0.0);
                let right = b.assessed_score.unwrap_or(0.0);
                right
                    .partial_cmp(&left)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            return (
                listings,
                format!(
                    "loaded {} evidence bundles from {}",
                    listing_count,
                    db_path.display()
                ),
            );
        }
        Ok(_) => {}
        Err(_) => {}
    }

    match load_listings_file(&db_path) {
        Ok(data) => {
            let mut listings = data.listings;
            let listing_count = listings.len();
            listings.sort_by(|a, b| {
                let left = a
                    .assessed_score
                    .or_else(|| a.scores.as_ref().map(|scores| scores.overall))
                    .unwrap_or(0.0);
                let right = b
                    .assessed_score
                    .or_else(|| b.scores.as_ref().map(|scores| scores.overall))
                    .unwrap_or(0.0);
                right
                    .partial_cmp(&left)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            (
                listings,
                format!(
                    "loaded {} listings from {}",
                    listing_count,
                    db_path.display()
                ),
            )
        }
        Err(error) => (
            Vec::new(),
            format!("load failed: {} ({})", error.message, db_path.display()),
        ),
    }
}

fn initial_selection(listings: &[Listing], requested_id: Option<&str>) -> usize {
    let Some(requested_id) = requested_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return 0;
    };
    listings
        .iter()
        .position(|listing| {
            listing.id == requested_id
                || listing.portal_ids.rightmove.as_deref() == Some(requested_id)
                || requested_id
                    .strip_prefix("rightmove:")
                    .is_some_and(|id| listing.portal_ids.rightmove.as_deref() == Some(id))
        })
        .unwrap_or(0)
}

fn listing_from_bundle(bundle: &EvidenceBundle) -> Listing {
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
    let assessed_score = bundle
        .assessment
        .as_ref()
        .and_then(|assessment| assessment.assessment.get("score"))
        .and_then(serde_json::Value::as_f64);

    Listing {
        id: bundle.rightmove_id.clone(),
        portal_ids: PortalIds {
            rightmove: Some(bundle.rightmove_id.clone()),
            ..PortalIds::default()
        },
        uprn: bundle.epc.as_ref().and_then(|epc| epc.uprn.clone()),
        uprn_source: None,
        uprn_confidence: None,
        url: bundle.url.clone(),
        location: GeoLocation {
            lat,
            lng,
            pin_type: None,
        },
        postcode,
        address,
        region: None,
        google_maps_url: format!("https://www.google.com/maps/search/?api=1&query={lat},{lng}"),
        google_maps_street_view_url: format!(
            "https://www.google.com/maps/@?api=1&map_action=pano&viewpoint={lat},{lng}"
        ),
        area: AreaMetrics::default(),
        price: bundle.rightmove.price_pcm.unwrap_or_default(),
        price_display: bundle.rightmove.display_price.clone().unwrap_or_default(),
        bedrooms: bundle.rightmove.bedrooms.unwrap_or_default(),
        bathrooms: bundle.rightmove.bathrooms.unwrap_or_default(),
        property_type: bundle.rightmove.property_type.clone().unwrap_or_default(),
        description: bundle.rightmove.description.text.clone(),
        notes: bundle.rightmove.description.key_features.clone(),
        images: bundle_media_images(bundle),
        floorplan: bundle
            .media
            .floorplans
            .first()
            .map(remote_local_asset_from_media)
            .unwrap_or_default(),
        epc: bundle
            .media
            .epc_graphs
            .first()
            .map(remote_local_asset_from_media)
            .unwrap_or_default(),
        map_views: MapViews {
            satellite: bundle
                .media
                .maps
                .iter()
                .find(|item| item.kind == "mapSatellite")
                .map(remote_local_asset_from_media)
                .unwrap_or_default(),
            street: bundle
                .media
                .maps
                .iter()
                .find(|item| item.kind == "mapStreet")
                .map(remote_local_asset_from_media)
                .unwrap_or_default(),
        },
        epc_rating: None,
        floor_area_sqm: bundle.epc.as_ref().and_then(|epc| epc.floor_area_sqm),
        epc_lodgement_date: bundle
            .epc
            .as_ref()
            .and_then(|epc| epc.lodgement_date.clone()),
        epc_address_match: bundle.epc.as_ref().map(|epc| epc.address_match),
        epc_search_url: None,
        nearest_stations: Vec::new(),
        gigabit_availability: bundle
            .broadband
            .as_ref()
            .and_then(|broadband| broadband.gigabit_availability),
        listed_date: bundle.rightmove.listed_date.clone(),
        lettings: Lettings {
            available_date: bundle.rightmove.available_date.clone(),
            deposit: bundle.rightmove.deposit,
        },
        agent: Agent {
            name: bundle.rightmove.agent_name.clone(),
            phone: bundle.rightmove.agent_phone.clone(),
        },
        assessment: None,
        assessed_at: bundle
            .assessment
            .as_ref()
            .map(|assessment| assessment.saved_at.clone()),
        assessed_score,
        scores: None,
        fetched_at: bundle.generated_at.clone(),
        extraction_status: ExtractionStatus::Success,
        status: ListingStatus::Active,
        notion_page_id: None,
    }
}

fn remote_local_asset_from_media(
    item: &let_sdk::intelligence::MediaItemEvidence,
) -> RemoteLocalAsset {
    RemoteLocalAsset {
        remote: Some(item.remote_url.clone()),
        local: item.local_path.clone(),
    }
}

fn bundle_media_images(bundle: &EvidenceBundle) -> Vec<ListingImage> {
    let mut images = Vec::new();
    if let Some(sheet) = bundle.media.contact_sheet.as_ref()
        && sheet.status == "generated"
        && let Some(local_path) = sheet.local_path.clone()
    {
        images.push(ListingImage {
            remote: format!("local://contact-sheet/{}", bundle.entity_id),
            local: Some(local_path),
        });
    }
    images.extend(bundle.media.photos.iter().map(|item| ListingImage {
        remote: item.remote_url.clone(),
        local: item.local_path.clone(),
    }));
    images
}

fn filtered_action_indices(actions: &[PaletteAction], query: &str) -> Vec<usize> {
    let normalized = query.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return (0..actions.len()).collect();
    }

    actions
        .iter()
        .enumerate()
        .filter_map(|(index, action)| {
            let label = action.label.to_ascii_lowercase();
            label.contains(&normalized).then_some(index)
        })
        .collect()
}

fn build_media_index(cache_root: &Path, listing: &Listing) -> ListingMedia {
    let cache_dir = resolve_cache_dir(cache_root, listing);
    let cache_dir_ref = cache_dir.as_deref();

    let mut images = listing
        .images
        .iter()
        .filter(|image| !image.remote.starts_with("local://contact-sheet/"))
        .filter_map(|image| {
            resolve_local_asset(cache_root, cache_dir_ref, image.local.as_deref(), listing)
        })
        .collect::<Vec<_>>();
    images.sort();
    images.dedup();
    let contact_sheet = listing
        .images
        .iter()
        .find(|image| image.remote.starts_with("local://contact-sheet/"))
        .and_then(|image| {
            resolve_local_asset(cache_root, cache_dir_ref, image.local.as_deref(), listing)
        })
        .or_else(|| cache_dir_ref.and_then(find_contact_sheet));

    let floorplan = resolve_local_asset(
        cache_root,
        cache_dir_ref,
        listing.floorplan.local.as_deref(),
        listing,
    );
    let satellite = resolve_local_asset(
        cache_root,
        cache_dir_ref,
        listing.map_views.satellite.local.as_deref(),
        listing,
    );
    let street = resolve_local_asset(
        cache_root,
        cache_dir_ref,
        listing.map_views.street.local.as_deref(),
        listing,
    );
    ListingMedia {
        cache_dir,
        contact_sheet,
        images,
        floorplan,
        satellite,
        street,
    }
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

fn resolve_cache_dir(cache_root: &Path, listing: &Listing) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(rightmove) = listing.portal_ids.rightmove.as_deref() {
        candidates.push(cache_root.join(rightmove));
    }
    candidates.push(cache_root.join(&listing.id));

    candidates.into_iter().find(|path| path.exists())
}

fn resolve_local_asset(
    cache_root: &Path,
    cache_dir: Option<&Path>,
    local: Option<&str>,
    listing: &Listing,
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
    if let Some(rightmove) = listing.portal_ids.rightmove.as_deref() {
        candidates.push(cache_root.join(rightmove).join(raw));
    }
    candidates.push(cache_root.join(raw));

    candidates.into_iter().find(|path| path.exists())
}

fn compact_media_asset(listing_key: &str, path: &Path) -> String {
    let file = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown");
    format!("{listing_key}/{file}")
}

fn collect_source_status() -> Vec<SourceStatus> {
    let paths = let_sdk::paths::paths();
    SOURCE_NAMES
        .iter()
        .map(|name| {
            let path = paths.derived.source_db(&paths.resolved.sources, name);
            let exists = path.exists();
            let size_mb = if exists {
                fs_size_mb(path.as_path()).unwrap_or(0.0)
            } else {
                0.0
            };

            SourceStatus {
                name: (*name).to_owned(),
                exists,
                size_mb,
            }
        })
        .collect()
}

fn resolve_cli_binary() -> PathBuf {
    if let Ok(path) = env::var("LET_CLI_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return candidate;
        }
    }

    if let Ok(current_exe) = env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        let sibling = parent.join(cli_binary_name());
        if sibling.is_file() {
            return sibling;
        }

        let unix_sibling = parent.join("let");
        if unix_sibling.is_file() {
            return unix_sibling;
        }
    }

    PathBuf::from("let")
}

fn cli_binary_name() -> &'static str {
    if cfg!(windows) { "let.exe" } else { "let" }
}

fn run_source_build(binary: &Path, target: &str) -> String {
    let status = Command::new(binary)
        .args(["sources", "build", target, "--jobs", "3"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(exit) if exit.success() => format!("sources build {target} completed"),
        Ok(exit) => format!("sources build {target} failed (exit {:?})", exit.code()),
        Err(error) => format!("sources build {target} failed ({error})"),
    }
}

fn fs_size_mb(path: &Path) -> Option<f64> {
    std::fs::metadata(path)
        .ok()
        .map(|meta| (meta.len() as f64) / (1024.0 * 1024.0))
}

fn is_palette_trigger(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(character) => {
            character.eq_ignore_ascii_case(&'p')
                && (key.modifiers.contains(KeyModifiers::SUPER)
                    || key.modifiers.contains(KeyModifiers::CONTROL))
        }
        _ => false,
    }
}

fn open_url_with_system(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Command::new("xdg-open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

fn quicklook_with_system(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("qlmanage")
            .arg("-p")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Command::new("xdg-open")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

fn reveal_in_finder(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("-R")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let target = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        Command::new("xdg-open")
            .arg(target)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use let_sdk::schema::listing::{
        Agent, AreaMetrics, ExtractionStatus, GeoLocation, Lettings, Listing, ListingStatus,
        MapViews, PortalIds, RemoteLocalAsset,
    };
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    use super::{
        App, ContextMediaItem, FocusPane, ListingMedia, build_context_media_items,
        build_media_index,
    };
    use crate::preview::{PreviewAssetKind, PreviewController};
    use crate::theme::Theme;

    fn down_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
    }

    fn up_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
    }

    fn sample_listing(index: usize) -> Listing {
        let rightmove_id = format!("900{index:03}");
        Listing {
            id: format!("rightmove:{rightmove_id}"),
            portal_ids: PortalIds {
                rightmove: Some(rightmove_id.clone()),
                ..PortalIds::default()
            },
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: format!("https://www.rightmove.co.uk/properties/{rightmove_id}"),
            location: GeoLocation {
                lat: 51.0,
                lng: -0.1,
                pin_type: None,
            },
            postcode: "SW1A 1AA".to_owned(),
            address: format!("{index} Test Street"),
            region: Some("test".to_owned()),
            google_maps_url: String::new(),
            google_maps_street_view_url: String::new(),
            area: AreaMetrics::default(),
            price: 1200 + index as i64,
            price_display: format!("£{}", 1200 + index as i64),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "Flat".to_owned(),
            description: String::new(),
            notes: Vec::new(),
            images: Vec::new(),
            floorplan: RemoteLocalAsset::default(),
            epc: RemoteLocalAsset::default(),
            map_views: MapViews::default(),
            epc_rating: None,
            floor_area_sqm: None,
            epc_lodgement_date: None,
            epc_address_match: None,
            epc_search_url: None,
            nearest_stations: Vec::new(),
            gigabit_availability: None,
            listed_date: None,
            lettings: Lettings::default(),
            agent: Agent::default(),
            assessment: None,
            assessed_at: None,
            assessed_score: None,
            scores: None,
            fetched_at: "2026-06-19T00:00:00Z".to_owned(),
            extraction_status: ExtractionStatus::Success,
            status: ListingStatus::Active,
            notion_page_id: None,
        }
    }

    fn app_with_listings(count: usize) -> App {
        let mut app = App::with_preview(PreviewController::disabled("test preview"));
        app.listings = (0..count).map(sample_listing).collect();
        app.selected = 0;
        app.rebuild_context_cache(true);
        app
    }

    fn app_with_context_media(count: usize) -> App {
        let mut app = App::with_preview(PreviewController::disabled("test preview"));
        app.focus = FocusPane::Context;
        app.context_items = (0..count)
            .map(|index| ContextMediaItem {
                kind: format!("img_{index:02}"),
                asset: format!("asset-{index:02}.jpg"),
                path: PathBuf::from(format!("/tmp/asset-{index:02}.jpg")),
                asset_kind: PreviewAssetKind::Photo,
            })
            .collect();
        app.context_selected = 0;
        app.context_offset = 0;
        app
    }

    #[test]
    fn contact_sheet_is_first_context_media_item() {
        let listing = sample_listing(1);
        let media = ListingMedia {
            cache_dir: Some(PathBuf::from("/tmp/cache")),
            contact_sheet: Some(PathBuf::from("/tmp/cache/contact-sheet.jpg")),
            images: vec![PathBuf::from("/tmp/cache/photo.jpg")],
            floorplan: None,
            satellite: None,
            street: None,
        };

        let items = build_context_media_items(&listing, &media);

        assert_eq!(items[0].kind, "contact-sheet");
        assert_eq!(items[1].kind, "img_01");
        assert_eq!(items[0].asset_kind, PreviewAssetKind::Photo);
    }

    #[test]
    fn bundle_contact_sheet_path_becomes_primary_media() {
        let test_dir = std::env::temp_dir().join(format!(
            "let-tui-media-index-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&test_dir).expect("create test media dir");
        let sheet_path = test_dir.join("exact-contact-sheet.jpg");
        let photo_path = test_dir.join("photo.jpg");
        fs::write(&sheet_path, b"sheet").expect("write contact sheet");
        fs::write(&photo_path, b"photo").expect("write photo");

        let mut listing = sample_listing(1);
        listing.images = vec![
            let_sdk::schema::listing::ListingImage {
                remote: "local://contact-sheet/rightmove:900001".to_owned(),
                local: Some(sheet_path.display().to_string()),
            },
            let_sdk::schema::listing::ListingImage {
                remote: "https://media.rightmove.co.uk/photo.jpg".to_owned(),
                local: Some(photo_path.display().to_string()),
            },
        ];

        let media = build_media_index(&test_dir, &listing);

        assert_eq!(media.contact_sheet.as_deref(), Some(sheet_path.as_path()));
        assert_eq!(media.images, vec![photo_path]);
        fs::remove_dir_all(test_dir).expect("remove test media dir");
    }

    #[test]
    fn quits_on_q_keypress() {
        let mut app = App::default();
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.is_running());
    }

    #[test]
    fn navigation_is_bounded_for_empty_lists() {
        let mut app = App::with_preview(PreviewController::disabled("test preview"));
        app.listings.clear();
        app.selected = 0;
        app.rebuild_context_cache(true);

        app.on_key(down_key());
        assert_eq!(app.selected_index(), 0);
        app.on_key(up_key());
        assert_eq!(app.selected_index(), 0);
    }

    #[test]
    fn holding_down_arrow_advances_listing_selection() {
        let mut app = app_with_listings(30);

        for _ in 0..200 {
            app.on_key(down_key());
        }
        assert_eq!(app.selected_index(), 29);

        for _ in 0..200 {
            app.on_key(up_key());
        }
        assert_eq!(app.selected_index(), 0);
    }

    #[test]
    fn holding_down_arrow_advances_context_media_selection() {
        let mut app = app_with_context_media(14);

        for _ in 0..100 {
            app.on_key(down_key());
        }
        assert_eq!(app.context_selected_index(), 13);

        for _ in 0..100 {
            app.on_key(up_key());
        }
        assert_eq!(app.context_selected_index(), 0);
    }

    #[test]
    fn repeated_media_navigation_renders_one_selected_row() {
        let mut app = app_with_context_media(14);
        let theme = Theme::default();
        let backend = TestBackend::new(180, 48);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        for _ in 0..12 {
            app.on_key(down_key());
            terminal
                .draw(|frame| crate::ui::render(frame, &mut app, &theme))
                .expect("render media navigation");
            assert_eq!(selected_background_rows(&terminal), 1);
        }

        for _ in 0..12 {
            app.on_key(up_key());
            terminal
                .draw(|frame| crate::ui::render(frame, &mut app, &theme))
                .expect("render media navigation");
            assert_eq!(selected_background_rows(&terminal), 1);
        }
    }

    #[test]
    fn opens_and_closes_palette() {
        let mut app = App::default();
        app.on_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        assert!(app.palette_open());
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.palette_open());
    }

    #[test]
    fn palette_query_filters_commands() {
        let mut app = App::default();
        app.on_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        let rows = app.palette_rows();
        assert_eq!(rows, vec!["sources build income".to_owned()]);
    }

    #[test]
    fn palette_enter_executes_quit() {
        let mut app = App::default();
        app.on_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.is_running());
    }

    #[test]
    fn ctrl_p_opens_palette() {
        let mut app = App::default();
        app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert!(app.palette_open());
    }

    fn selected_background_rows(terminal: &Terminal<TestBackend>) -> usize {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .filter(|y| (0..buffer.area.width).any(|x| buffer[(x, *y)].bg == Color::Cyan))
            .count()
    }
}
