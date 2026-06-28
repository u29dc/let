#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use let_sdk::intelligence::{EvidenceBundle, IntelligenceDb};
use let_sdk::score::ScoreSummary;
use ratatui::layout::{Position, Rect};

use crate::listing::{ListingMedia, TuiListingRow};
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

struct LoadedListings {
    listings: Vec<TuiListingRow>,
    bundle_context: HashMap<String, EvidenceBundle>,
    status: String,
}

pub(crate) struct App {
    running: bool,
    listings: Vec<TuiListingRow>,
    selected: usize,
    focus: FocusPane,
    context_selected: usize,
    context_offset: usize,
    context_summary_offset: usize,
    context_summary_max_offset: usize,
    context_summary_page_size: usize,
    context_summary_area: Option<Rect>,
    bundle_context: HashMap<String, EvidenceBundle>,
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
        let loaded = load_ranked_listings();
        let listings = loaded.listings;
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
            context_summary_offset: 0,
            context_summary_max_offset: 0,
            context_summary_page_size: 1,
            context_summary_area: None,
            bundle_context: loaded.bundle_context,
            selected_media: ListingMedia::default(),
            context_items: Vec::new(),
            status: loaded.status,
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

    pub(crate) fn listings(&self) -> &[TuiListingRow] {
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

    pub(crate) fn selected_listing(&self) -> Option<&TuiListingRow> {
        self.listings.get(self.selected)
    }

    pub(crate) fn selected_bundle(&self) -> Option<&EvidenceBundle> {
        let listing = self.selected_listing()?;
        [
            listing.entity_id.as_str(),
            listing.rightmove_id.as_str(),
            listing.id.as_str(),
        ]
        .into_iter()
        .find_map(|key| self.bundle_context.get(key))
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

    pub(crate) fn context_summary_scroll_offset(&self) -> u16 {
        self.context_summary_offset.min(u16::MAX as usize) as u16
    }

    pub(crate) fn context_summary_scroll_position(&self) -> (usize, usize) {
        (self.context_summary_offset, self.context_summary_max_offset)
    }

    pub(crate) fn set_context_summary_viewport(
        &mut self,
        area: Rect,
        content_rows: usize,
        viewport_rows: usize,
    ) {
        self.context_summary_area = Some(area);
        let visible_rows = viewport_rows.saturating_sub(2).max(1);
        self.context_summary_page_size = visible_rows.saturating_sub(2).max(1);
        self.context_summary_max_offset = content_rows.saturating_sub(viewport_rows);
        self.clamp_context_summary_scroll();
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

    pub(crate) fn preview_view(&self) -> PreviewView<'_> {
        self.preview.view()
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
            KeyCode::PageDown => {
                self.scroll_context_summary_down(self.context_summary_page_size);
            }
            KeyCode::PageUp => {
                self.scroll_context_summary_up(self.context_summary_page_size);
            }
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

    pub(crate) fn on_mouse(&mut self, mouse: MouseEvent) {
        if !self.mouse_over_context_summary(mouse.column, mouse.row) {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll_context_summary_down(3),
            MouseEventKind::ScrollUp => self.scroll_context_summary_up(3),
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

    fn scroll_context_summary_down(&mut self, amount: usize) {
        self.context_summary_offset = self
            .context_summary_offset
            .saturating_add(amount)
            .min(self.context_summary_max_offset);
    }

    fn scroll_context_summary_up(&mut self, amount: usize) {
        self.context_summary_offset = self.context_summary_offset.saturating_sub(amount);
    }

    fn clamp_context_summary_scroll(&mut self) {
        self.context_summary_offset = self
            .context_summary_offset
            .min(self.context_summary_max_offset);
    }

    fn mouse_over_context_summary(&self, column: u16, row: u16) -> bool {
        self.context_summary_area
            .is_some_and(|area| area.contains(Position { x: column, y: row }))
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
        let loaded = load_ranked_listings();
        self.listings = loaded.listings;
        self.bundle_context = loaded.bundle_context;
        if self.selected >= self.listings.len() {
            self.selected = self.listings.len().saturating_sub(1);
        }
        self.rebuild_context_cache(true);
        self.status = loaded.status;
        self.refresh_sources();
    }

    fn refresh_sources(&mut self) {
        self.source_status = collect_source_status();
    }

    fn rebuild_context_cache(&mut self, reset_selection: bool) {
        let (selected_media, context_items) = if let Some(listing) = self.selected_listing() {
            let selected_media = listing.media.clone();
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
            self.context_summary_offset = 0;
            self.context_summary_max_offset = 0;
            self.context_summary_page_size = 1;
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

fn build_context_media_items(
    listing: &TuiListingRow,
    media: &ListingMedia,
) -> Vec<ContextMediaItem> {
    let listing_key = listing.rightmove_id.clone();
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

fn load_ranked_listings() -> LoadedListings {
    let paths = let_sdk::paths::paths();
    let db_path = paths.derived.database;

    match load_tui_data(&db_path) {
        Ok((bundles, score_summaries)) => {
            let cache_root = paths.resolved.cache.as_path();
            let scores_by_rightmove = score_summary_index(&score_summaries);
            let mut listings = bundles
                .iter()
                .map(|bundle| {
                    TuiListingRow::from_bundle(bundle, cache_root)
                        .with_score_summary(scores_by_rightmove.get(&bundle.rightmove_id).copied())
                })
                .collect::<Vec<_>>();
            let bundle_context = bundle_context_index(&bundles);
            let listing_count = listings.len();
            listings.sort_by(|a, b| {
                score_sort_key(b)
                    .total_cmp(&score_sort_key(a))
                    .then_with(|| b.generated_at.cmp(&a.generated_at))
                    .then_with(|| a.rightmove_id.cmp(&b.rightmove_id))
            });
            LoadedListings {
                listings,
                bundle_context,
                status: format!(
                    "loaded {} evidence bundles and {} scores from {}",
                    listing_count,
                    score_summaries.len(),
                    db_path.display()
                ),
            }
        }
        Err(error) => LoadedListings {
            listings: Vec::new(),
            bundle_context: HashMap::new(),
            status: format!("load failed: {} ({})", error.message, db_path.display()),
        },
    }
}

fn load_tui_data(db_path: &Path) -> let_sdk::Result<(Vec<EvidenceBundle>, Vec<ScoreSummary>)> {
    let db = IntelligenceDb::open_readonly(db_path)?;
    let bundles = db.load_bundles()?;
    let scores = db.list_score_summaries(Some(let_sdk::score::DEFAULT_SCORECARD_ID))?;
    Ok((bundles, scores))
}

fn score_summary_index(summaries: &[ScoreSummary]) -> HashMap<String, &ScoreSummary> {
    let mut index = HashMap::new();
    for summary in summaries {
        index.insert(summary.rightmove_id.clone(), summary);
        index.insert(summary.entity_id.clone(), summary);
    }
    index
}

fn score_sort_key(listing: &TuiListingRow) -> f64 {
    listing.score_overall.unwrap_or(-1.0)
}

fn bundle_context_index(bundles: &[EvidenceBundle]) -> HashMap<String, EvidenceBundle> {
    let mut index = HashMap::new();
    for bundle in bundles {
        index.insert(bundle.entity_id.clone(), bundle.clone());
        index.insert(bundle.rightmove_id.clone(), bundle.clone());
        index.insert(format!("rightmove:{}", bundle.rightmove_id), bundle.clone());
    }
    index
}

fn initial_selection(listings: &[TuiListingRow], requested_id: Option<&str>) -> usize {
    let Some(requested_id) = requested_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return 0;
    };
    listings
        .iter()
        .position(|listing| listing.matches_requested_id(requested_id))
        .unwrap_or(0)
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
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
    use let_sdk::intelligence::{
        AddressEvidence, AssessmentRecord, BroadbandEvidence, ConfidenceLevel,
        ContactSheetEvidence, DescriptionEvidence, EvidenceBundle, FactEvidence, FactProvider,
        InspectDepth, IntelligenceDb, MediaEvidence, MediaItemEvidence, NearestStationEvidence,
        RefreshPolicy, RightmoveEvidence, SectionState, SectionStatus,
    };
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
    use serde_json::json;

    use super::{
        App, ContextMediaItem, FocusPane, ListingMedia, build_context_media_items,
        bundle_context_index,
    };
    use crate::listing::TuiListingRow;
    use crate::preview::{PreviewAssetKind, PreviewController};
    use crate::theme::Theme;

    fn down_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
    }

    fn up_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)
    }

    fn sample_listing(index: usize) -> TuiListingRow {
        let rightmove_id = format!("900{index:03}");
        TuiListingRow {
            id: rightmove_id.clone(),
            entity_id: format!("rightmove:{rightmove_id}"),
            rightmove_id: rightmove_id.clone(),
            url: format!("https://www.rightmove.co.uk/properties/{rightmove_id}"),
            google_maps_url: String::new(),
            google_maps_street_view_url: String::new(),
            address: format!("{index} Test Street"),
            postcode: "SW1A 1AA".to_owned(),
            region: Some("test".to_owned()),
            price_pcm: 1200 + index as i64,
            price_display: format!("£{}", 1200 + index as i64),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "Flat".to_owned(),
            notes: Vec::new(),
            epc_rating: None,
            epc_remote: None,
            floor_area_sqm: None,
            epc_address_match: None,
            nearest_station_miles: None,
            gigabit_availability: None,
            crime_rate_per_1k: None,
            imd_decile: None,
            available_date: None,
            deposit: None,
            agent_name: None,
            agent_phone: None,
            score_overall: None,
            score_band: None,
            score_confidence: None,
            score_computed_at: None,
            generated_at: "2026-06-19T00:00:00Z".to_owned(),
            page_status: "active".to_owned(),
            media: ListingMedia::default(),
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

    fn app_with_bundle(bundle: EvidenceBundle) -> App {
        let mut app = App::with_preview(PreviewController::disabled("test preview"));
        app.listings = vec![TuiListingRow::from_bundle(
            &bundle,
            std::env::temp_dir().as_path(),
        )];
        app.bundle_context = bundle_context_index(&[bundle]);
        app.selected = 0;
        app.rebuild_context_cache(true);
        app
    }

    fn sample_evidence_bundle() -> EvidenceBundle {
        let mut sections = BTreeMap::new();
        sections.insert(
            "rightmove".to_owned(),
            SectionState::ok("Rightmove evidence captured", ConfidenceLevel::Exact),
        );
        sections.insert(
            "media".to_owned(),
            SectionState::ok(
                "20 media assets extracted; 18 cached locally",
                ConfidenceLevel::Probable,
            ),
        );
        sections.insert(
            "assessment".to_owned(),
            SectionState::ok("agent assessment is saved", ConfidenceLevel::Exact),
        );

        EvidenceBundle {
            entity_id: "rightmove:900001".to_owned(),
            rightmove_id: "900001".to_owned(),
            url: "https://www.rightmove.co.uk/properties/900001".to_owned(),
            generated_at: "2026-06-21T10:00:00Z".to_owned(),
            depth: InspectDepth::Standard,
            refresh: RefreshPolicy::None,
            sections,
            source_snapshots: Vec::new(),
            rightmove: RightmoveEvidence {
                rightmove_id: "900001".to_owned(),
                url: "https://www.rightmove.co.uk/properties/900001".to_owned(),
                page_status: "active".to_owned(),
                fetched_at: "2026-06-21T10:00:00Z".to_owned(),
                content_hash: "hash".to_owned(),
                title: Some("2 bedroom flat".to_owned()),
                address: Some("1 Test Street".to_owned()),
                postcode: Some("TN1 1AA".to_owned()),
                display_price: Some("£1,750 pcm".to_owned()),
                price_pcm: Some(1750),
                bedrooms: Some(2),
                bathrooms: Some(1),
                property_type: Some("Flat".to_owned()),
                agent_name: Some("Example Agent".to_owned()),
                agent_phone: Some("020 0000 0000".to_owned()),
                latitude: Some(51.2),
                longitude: Some(0.2),
                pin_type: None,
                listed_date: Some("2026-06-01".to_owned()),
                available_date: Some("2026-07-01".to_owned()),
                deposit: Some(2019),
                description: DescriptionEvidence {
                    raw_html: String::new(),
                    text: "Good light and practical layout.".to_owned(),
                    key_features: vec!["balcony".to_owned()],
                    normalized_text: "good light and practical layout".to_owned(),
                },
                nearest_stations: vec![NearestStationEvidence {
                    name: "Example Station".to_owned(),
                    distance: 0.7,
                    unit: "miles".to_owned(),
                }],
                media: Vec::new(),
            },
            address: AddressEvidence {
                candidates: Vec::new(),
                selected: None,
                status: SectionStatus::Ok,
                confidence: ConfidenceLevel::Probable,
                warnings: Vec::new(),
            },
            facts: Vec::new(),
            broadband: Some(BroadbandEvidence {
                postcode: "TN1 1AA".to_owned(),
                postcode_display: Some("TN1 1AA".to_owned()),
                outward: Some("TN1".to_owned()),
                area: Some("Tonbridge".to_owned()),
                gigabit_availability: Some(100.0),
                pct_over_300mbps: Some(97.0),
                ufbb_availability: None,
                sfbb_availability: None,
            }),
            epc: None,
            claims: Vec::new(),
            verifications: Vec::new(),
            media: MediaEvidence::default(),
            assessment: Some(AssessmentRecord::new(
                "rightmove:900001".to_owned(),
                json!({
                    "recommendation": "consider",
                    "confidence": "medium_high",
                    "score": 82,
                    "summary": "Worth viewing if the photos hold up.",
                    "positives": ["good station access", "usable layout"],
                    "risks": ["needs photo review"],
                    "nextActions": ["call agent"],
                    "familyFit": "workable for the household",
                    "evidenceGaps": ["floor area"]
                }),
                "2026-06-21T11:00:00Z".to_owned(),
            )),
            corrections: Vec::new(),
            next_actions: Vec::new(),
            flags: Vec::new(),
        }
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

        let mut bundle = sample_evidence_bundle();
        bundle.media.contact_sheet = Some(ContactSheetEvidence {
            status: "generated".to_owned(),
            photo_count: 1,
            local_path: Some(sheet_path.display().to_string()),
            generated_at: Some("2026-06-21T11:00:00Z".to_owned()),
            width: None,
            height: None,
            content_hash: None,
        });
        bundle.media.photos.push(MediaItemEvidence {
            kind: "photo".to_owned(),
            remote_url: "https://media.rightmove.co.uk/photo.jpg".to_owned(),
            local_path: Some(photo_path.display().to_string()),
            width: None,
            height: None,
            content_hash: None,
            status: "cached".to_owned(),
        });
        let listing = TuiListingRow::from_bundle(&bundle, &test_dir);
        let media = listing.media;

        assert_eq!(media.contact_sheet.as_deref(), Some(sheet_path.as_path()));
        assert_eq!(media.images, vec![photo_path]);
        fs::remove_dir_all(test_dir).expect("remove test media dir");
    }

    #[test]
    fn listing_row_preserves_active_area_facts() {
        let mut bundle = sample_evidence_bundle();
        bundle.facts = vec![
            FactEvidence {
                provider: FactProvider::DeprivationDb,
                category: "deprivation".to_owned(),
                name: "imdDecile".to_owned(),
                value: json!(8),
                confidence: ConfidenceLevel::Exact,
                sources: Vec::new(),
            },
            FactEvidence {
                provider: FactProvider::CrimeDb,
                category: "crime".to_owned(),
                name: "ratePer1k".to_owned(),
                value: json!(42.5),
                confidence: ConfidenceLevel::Exact,
                sources: Vec::new(),
            },
        ];

        let listing = TuiListingRow::from_bundle(&bundle, std::env::temp_dir().as_path());

        assert_eq!(listing.imd_decile, Some(8));
        assert_eq!(listing.crime_rate_per_1k, Some(42.5));
        assert_eq!(listing.nearest_station_miles, Some(0.7));
    }

    #[test]
    fn context_panel_renders_saved_assessment_and_evidence_details() {
        let mut app = app_with_bundle(sample_evidence_bundle());
        let theme = Theme::default();
        let backend = TestBackend::new(240, 60);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| crate::ui::render(frame, &mut app, &theme))
            .expect("render context");

        let text = buffer_text(&terminal);
        assert!(text.contains("assessment"));
        assert!(text.contains("consider"));
        assert!(text.contains("medium_high"));
        assert!(text.contains("Worth viewing"));
        assert!(text.contains("rightmove"));
        assert!(text.contains("media"));
        assert!(text.contains("20 media assets"));
    }

    #[test]
    fn context_panel_renders_assessment_loaded_from_repository() {
        let test_dir = std::env::temp_dir().join(format!(
            "let-tui-assessment-hydration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&test_dir).expect("create test db dir");
        let db_path = test_dir.join("let.db");
        let mut bundle = sample_evidence_bundle();
        bundle.assessment = None;
        bundle.sections.insert(
            "assessment".to_owned(),
            SectionState::skipped("no agent assessment saved"),
        );
        bundle.next_actions.push(
            "save the agent assessment with `let assess save <id> <assessment-json>`".to_owned(),
        );

        let mut db = IntelligenceDb::open(&db_path).expect("open db");
        db.save_bundle(&bundle).expect("save bundle");
        db.save_assessment(
            &bundle.rightmove_id,
            json!({
                "recommendation": "consider",
                "confidence": "medium_high",
                "summary": "Worth viewing if the photos hold up."
            }),
        )
        .expect("save assessment");
        let loaded = db
            .load_bundle(&bundle.rightmove_id)
            .expect("load bundle")
            .expect("bundle exists");
        drop(db);

        let mut app = app_with_bundle(loaded);
        let theme = Theme::default();
        let backend = TestBackend::new(240, 60);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| crate::ui::render(frame, &mut app, &theme))
            .expect("render context");

        let text = buffer_text(&terminal);
        assert!(text.contains("consider"));
        assert!(text.contains("medium_high"));
        assert!(text.contains("Worth viewing if the photos hold up."));
        assert!(!text.contains("no saved assessment"));
        assert!(!text.contains("let assess save"));
        fs::remove_dir_all(test_dir).expect("remove test db dir");
    }

    #[test]
    fn context_summary_page_keys_scroll_without_moving_media_selection() {
        let mut app = app_with_context_media(14);
        app.set_context_summary_viewport(Rect::new(120, 2, 58, 12), 40, 12);

        app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.context_selected_index(), 0);
        assert_eq!(app.context_summary_scroll_position().0, 8);

        app.on_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(app.context_selected_index(), 0);
        assert_eq!(app.context_summary_scroll_position().0, 0);
    }

    #[test]
    fn mouse_wheel_scrolls_context_summary_when_pointer_is_over_it() {
        let mut app = app_with_context_media(14);
        app.set_context_summary_viewport(Rect::new(120, 2, 58, 12), 40, 12);

        app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 130,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.context_summary_scroll_position().0, 3);

        app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 20,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.context_summary_scroll_position().0, 3);

        app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 130,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.context_summary_scroll_position().0, 0);
    }

    #[test]
    fn context_summary_scroll_reveals_long_assessment_content() {
        let mut bundle = sample_evidence_bundle();
        bundle.assessment = Some(AssessmentRecord::new(
            bundle.entity_id.clone(),
            json!({
                "recommendation": "consider",
                "confidence": "medium_high",
                "summary": "This is a deliberately long assessment summary with enough detail to wrap over multiple rendered lines in the context pane. It should stay readable through the summary scroll path instead of being truncated away.",
                "positives": [
                    "good station access",
                    "workable layout",
                    "cached media is available",
                    "pricing is within range"
                ],
                "risks": [
                    "risk one has enough words to wrap on a narrow context pane",
                    "risk two also has enough words to wrap on a narrow context pane"
                ],
                "tradeoffs": [
                    "tradeoff text should remain available after scrolling the assessment context"
                ],
                "evidenceGaps": [
                    "tail marker TAIL_TOKEN_VISIBLE_AFTER_SCROLL"
                ]
            }),
            "2026-06-21T11:00:00Z".to_owned(),
        ));
        let mut app = app_with_bundle(bundle);
        app.focus = FocusPane::Context;
        let theme = Theme::default();
        let backend = TestBackend::new(120, 26);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| crate::ui::render(frame, &mut app, &theme))
            .expect("render initial context");
        assert!(!buffer_text(&terminal).contains("TAIL_TOKEN_VISIBLE_AFTER_SCROLL"));

        let mut saw_tail = false;
        for _ in 0..10 {
            app.on_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
            terminal
                .draw(|frame| crate::ui::render(frame, &mut app, &theme))
                .expect("render scrolled context");
            saw_tail |= buffer_text(&terminal).contains("TAIL_TOKEN_VISIBLE_AFTER_SCROLL");
        }

        assert!(saw_tail);
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
    fn rapid_media_navigation_renders_one_selected_row() {
        let mut app = app_with_context_media(30);
        let theme = Theme::default();
        let backend = TestBackend::new(180, 48);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        for _ in 0..25 {
            app.on_key(down_key());
        }
        terminal
            .draw(|frame| crate::ui::render(frame, &mut app, &theme))
            .expect("render rapid media navigation");

        assert_eq!(app.context_selected_index(), 25);
        assert_eq!(selected_background_rows(&terminal), 1);
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

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let mut output = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                output.push_str(buffer[(x, y)].symbol());
            }
            output.push('\n');
        }
        output
    }
}
