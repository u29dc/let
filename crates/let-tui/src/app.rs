#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use let_sdk::load_listings_file;
use let_sdk::schema::listing::Listing;

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
    pub(crate) images: Vec<PathBuf>,
    pub(crate) floorplan: Option<PathBuf>,
    pub(crate) satellite: Option<PathBuf>,
    pub(crate) street: Option<PathBuf>,
}

impl ListingMedia {
    fn first_image(&self) -> Option<PathBuf> {
        self.images.first().cloned()
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
struct ContextMediaItem {
    kind: String,
    asset: String,
    path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct App {
    running: bool,
    listings: Vec<Listing>,
    selected: usize,
    focus: FocusPane,
    context_selected: usize,
    status: String,
    header: HeaderContract,
    palette_open: bool,
    palette_query: String,
    palette_selected: usize,
    palette_actions: Vec<PaletteAction>,
    palette_filtered: Vec<usize>,
    source_status: Vec<SourceStatus>,
}

impl App {
    pub(crate) fn is_running(&self) -> bool {
        self.running
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

    pub(crate) fn selected_media(&self) -> ListingMedia {
        let Some(listing) = self.selected_listing() else {
            return ListingMedia::default();
        };

        let paths = let_sdk::paths::paths();
        let cache_root = paths.resolved.cache.as_path();
        build_media_index(cache_root, listing)
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

    pub(crate) fn context_rows(&self) -> Vec<(String, String)> {
        self.build_context_media_items()
            .into_iter()
            .map(|item| (item.kind, item.asset))
            .collect()
    }

    pub(crate) fn context_selected_index(&self) -> usize {
        self.context_selected
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
            if let Some(path) = media.first_image() {
                actions.push(PaletteAction {
                    label: "quick look first image".to_owned(),
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.floorplan {
                actions.push(PaletteAction {
                    label: "quick look floorplan".to_owned(),
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.satellite {
                actions.push(PaletteAction {
                    label: "quick look satellite map".to_owned(),
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.street {
                actions.push(PaletteAction {
                    label: "quick look street map".to_owned(),
                    kind: PaletteActionKind::QuickLook(path),
                });
            }
            if let Some(path) = media.cache_dir {
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
            label: "build sources all".to_owned(),
            kind: PaletteActionKind::BuildSources("all"),
        });
        actions.push(PaletteAction {
            label: "build sources broadband".to_owned(),
            kind: PaletteActionKind::BuildSources("broadband"),
        });
        actions.push(PaletteAction {
            label: "build sources crime".to_owned(),
            kind: PaletteActionKind::BuildSources("crime"),
        });
        actions.push(PaletteAction {
            label: "build sources income".to_owned(),
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
        let len = self.build_context_media_items().len();
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
        let len = self.build_context_media_items().len();
        if len == 0 {
            self.context_selected = 0;
            return;
        }
        self.context_selected = len.saturating_sub(1);
    }

    fn clamp_context_selection(&mut self) {
        let len = self.build_context_media_items().len();
        if len == 0 {
            self.context_selected = 0;
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
        let items = self.build_context_media_items();
        let Some(item) = items.get(self.context_selected) else {
            self.status = "no media selected".to_owned();
            return;
        };
        let path = item.path.clone();
        self.quicklook_path(&path);
    }

    fn build_context_media_items(&self) -> Vec<ContextMediaItem> {
        let Some(listing) = self.selected_listing() else {
            return Vec::new();
        };

        let media = self.selected_media();
        let listing_key = listing
            .portal_ids
            .rightmove
            .as_deref()
            .unwrap_or(listing.id.as_str())
            .to_owned();
        let mut items = Vec::new();

        for (index, path) in media.images.iter().enumerate() {
            items.push(ContextMediaItem {
                kind: format!("img_{:02}", index + 1),
                asset: compact_media_asset(&listing_key, path),
                path: path.clone(),
            });
        }
        if let Some(path) = media.floorplan {
            items.push(ContextMediaItem {
                kind: "floorplan".to_owned(),
                asset: compact_media_asset(&listing_key, &path),
                path,
            });
        }
        if let Some(path) = media.satellite {
            items.push(ContextMediaItem {
                kind: "satellite-map".to_owned(),
                asset: compact_media_asset(&listing_key, &path),
                path,
            });
        }
        if let Some(path) = media.street {
            items.push(ContextMediaItem {
                kind: "street-map".to_owned(),
                asset: compact_media_asset(&listing_key, &path),
                path,
            });
        }

        items
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
        let mut command = Command::new("cargo");
        command
            .args([
                "run", "-q", "-p", "let-cli", "--", "build", "sources", target, "--jobs", "3",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let status = command.status();
        self.status = match status {
            Ok(exit) if exit.success() => format!("build sources {target} completed"),
            Ok(exit) => format!("build sources {target} failed (exit {:?})", exit.code()),
            Err(error) => format!("build sources {target} failed ({error})"),
        };
        self.refresh_sources();
    }

    fn select_next(&mut self) {
        if self.listings.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.listings.len().saturating_sub(1));
        self.clamp_context_selection();
    }

    fn select_prev(&mut self) {
        if self.listings.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
        self.clamp_context_selection();
    }

    fn select_first(&mut self) {
        if !self.listings.is_empty() {
            self.selected = 0;
            self.clamp_context_selection();
        }
    }

    fn select_last(&mut self) {
        if !self.listings.is_empty() {
            self.selected = self.listings.len().saturating_sub(1);
            self.clamp_context_selection();
        }
    }

    fn refresh_all(&mut self) {
        let (listings, status) = load_ranked_listings();
        self.listings = listings;
        if self.selected >= self.listings.len() {
            self.selected = self.listings.len().saturating_sub(1);
        }
        self.clamp_context_selection();
        self.status = status;
        self.refresh_sources();
    }

    fn refresh_sources(&mut self) {
        self.source_status = collect_source_status();
    }
}

impl Default for App {
    fn default() -> Self {
        let (listings, status) = load_ranked_listings();
        Self {
            running: true,
            listings,
            selected: 0,
            focus: FocusPane::Listings,
            context_selected: 0,
            status,
            header: HEADER_CONTRACT,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            palette_actions: Vec::new(),
            palette_filtered: Vec::new(),
            source_status: collect_source_status(),
        }
    }
}

fn load_ranked_listings() -> (Vec<Listing>, String) {
    let paths = let_sdk::paths::paths();
    let db_path = paths.derived.database;

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
        .filter_map(|image| {
            resolve_local_asset(cache_root, cache_dir_ref, image.local.as_deref(), listing)
        })
        .collect::<Vec<_>>();
    images.sort();
    images.dedup();

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
        images,
        floorplan,
        satellite,
        street,
    }
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{App, FocusPane};
    use crate::theme::HEADER_CONTRACT;

    #[test]
    fn quits_on_q_keypress() {
        let mut app = App::default();
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.is_running());
    }

    #[test]
    fn navigation_is_bounded_for_empty_lists() {
        let mut app = App {
            running: true,
            listings: vec![],
            selected: 0,
            focus: FocusPane::Listings,
            context_selected: 0,
            status: String::new(),
            header: HEADER_CONTRACT,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            palette_actions: Vec::new(),
            palette_filtered: Vec::new(),
            source_status: vec![],
        };

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_index(), 0);
        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.selected_index(), 0);
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
        assert_eq!(rows, vec!["build sources income".to_owned()]);
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
}
