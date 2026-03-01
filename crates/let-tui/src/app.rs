#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::Command;

use crossterm::event::{KeyCode, KeyEvent};
use let_sdk::load_listings_file;
use let_sdk::schema::listing::Listing;

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

const PALETTE_COMMANDS: [&str; 6] = [
    "refresh",
    "build sources all",
    "build sources broadband",
    "build sources crime",
    "build sources income",
    "quit",
];

#[derive(Debug, Clone)]
pub(crate) struct SourceStatus {
    pub(crate) name: String,
    pub(crate) exists: bool,
    pub(crate) size_mb: f64,
}

#[derive(Debug)]
pub(crate) struct App {
    running: bool,
    listings: Vec<Listing>,
    selected: usize,
    status: String,
    palette_open: bool,
    palette_query: String,
    palette_selected: usize,
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

    pub(crate) fn status(&self) -> &str {
        &self.status
    }

    pub(crate) fn selected_listing(&self) -> Option<&Listing> {
        self.listings.get(self.selected)
    }

    pub(crate) fn palette_open(&self) -> bool {
        self.palette_open
    }

    pub(crate) fn palette_query(&self) -> &str {
        &self.palette_query
    }

    pub(crate) fn palette_items(&self) -> Vec<&'static str> {
        let query = self.palette_query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return PALETTE_COMMANDS.into();
        }

        PALETTE_COMMANDS
            .iter()
            .copied()
            .filter(|command| command.contains(&query))
            .collect()
    }

    pub(crate) fn palette_selected_index(&self) -> usize {
        self.palette_selected
    }

    pub(crate) fn source_status(&self) -> &[SourceStatus] {
        &self.source_status
    }

    pub(crate) fn on_key(&mut self, key: KeyEvent) {
        if self.palette_open {
            self.on_palette_key(key);
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.running = false;
            }
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
            KeyCode::Char('g') | KeyCode::Home => self.select_first(),
            KeyCode::Char('G') | KeyCode::End => self.select_last(),
            KeyCode::Char('r') | KeyCode::Char('R') => self.refresh_all(),
            KeyCode::Char(':') => self.open_palette(),
            _ => {}
        }
    }

    fn on_palette_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_palette(),
            KeyCode::Enter => self.execute_palette_selection(),
            KeyCode::Up => self.palette_prev(),
            KeyCode::Down => self.palette_next(),
            KeyCode::Backspace => {
                self.palette_query.pop();
                self.clamp_palette_selection();
            }
            KeyCode::Char(ch) if !ch.is_control() => {
                self.palette_query.push(ch);
                self.clamp_palette_selection();
            }
            _ => {}
        }
    }

    fn open_palette(&mut self) {
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.status = "palette opened".to_owned();
    }

    fn close_palette(&mut self) {
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.status = "palette closed".to_owned();
    }

    fn clamp_palette_selection(&mut self) {
        let len = self.palette_items().len();
        if len == 0 {
            self.palette_selected = 0;
        } else if self.palette_selected >= len {
            self.palette_selected = len.saturating_sub(1);
        }
    }

    fn palette_next(&mut self) {
        let len = self.palette_items().len();
        if len == 0 {
            self.palette_selected = 0;
            return;
        }
        self.palette_selected = (self.palette_selected + 1).min(len.saturating_sub(1));
    }

    fn palette_prev(&mut self) {
        self.palette_selected = self.palette_selected.saturating_sub(1);
    }

    fn execute_palette_selection(&mut self) {
        let items = self.palette_items();
        let Some(command) = items.get(self.palette_selected).copied() else {
            self.status = "no palette command selected".to_owned();
            return;
        };

        match command {
            "refresh" => self.refresh_all(),
            "quit" => {
                self.running = false;
                self.status = "quitting".to_owned();
            }
            "build sources all" => self.build_sources("all"),
            "build sources broadband" => self.build_sources("broadband"),
            "build sources crime" => self.build_sources("crime"),
            "build sources income" => self.build_sources("income"),
            _ => {
                self.status = format!("unknown command: {command}");
            }
        }
        self.close_palette();
    }

    fn build_sources(&mut self, target: &str) {
        let mut command = Command::new("cargo");
        command
            .args([
                "run", "-q", "-p", "let-cli", "--", "build", "sources", target, "--jobs", "3",
                "--json",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

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
    }

    fn select_prev(&mut self) {
        if self.listings.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_first(&mut self) {
        if !self.listings.is_empty() {
            self.selected = 0;
        }
    }

    fn select_last(&mut self) {
        if !self.listings.is_empty() {
            self.selected = self.listings.len().saturating_sub(1);
        }
    }

    fn refresh_all(&mut self) {
        let (listings, status) = load_ranked_listings();
        self.listings = listings;
        if self.selected >= self.listings.len() {
            self.selected = self.listings.len().saturating_sub(1);
        }
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
            status,
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
            source_status: collect_source_status(),
        }
    }
}

fn load_ranked_listings() -> (Vec<Listing>, String) {
    let bundle = let_sdk::paths::paths();
    let db_path = bundle.derived.database;

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

fn collect_source_status() -> Vec<SourceStatus> {
    let bundle = let_sdk::paths::paths();
    SOURCE_NAMES
        .iter()
        .map(|name| {
            let path: PathBuf = bundle.derived.source_db(&bundle.resolved.sources, name);
            let exists = path.exists();
            let size_mb = if exists {
                fs_size_mb(&path).unwrap_or(0.0)
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

fn fs_size_mb(path: &PathBuf) -> Option<f64> {
    std::fs::metadata(path)
        .ok()
        .map(|meta| (meta.len() as f64) / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::App;

    #[test]
    fn quits_on_q_keypress() {
        let mut app = App::default();
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);

        app.on_key(key);

        assert!(!app.is_running());
    }

    #[test]
    fn navigation_is_bounded_for_empty_lists() {
        let mut app = App {
            running: true,
            listings: vec![],
            selected: 0,
            status: String::new(),
            palette_open: false,
            palette_query: String::new(),
            palette_selected: 0,
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
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));

        let items = app.palette_items();
        assert_eq!(items, vec!["quit"]);
    }

    #[test]
    fn palette_enter_executes_quit() {
        let mut app = App::default();
        app.on_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(!app.is_running());
    }
}
