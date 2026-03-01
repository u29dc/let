#![forbid(unsafe_code)]

use crossterm::event::{KeyCode, KeyEvent};
use let_sdk::load_listings_file;
use let_sdk::schema::listing::Listing;

#[derive(Debug)]
pub(crate) struct App {
    running: bool,
    listings: Vec<Listing>,
    selected: usize,
    status: String,
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

    pub(crate) fn on_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                self.running = false;
            }
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
            KeyCode::Char('g') | KeyCode::Home => self.select_first(),
            KeyCode::Char('G') | KeyCode::End => self.select_last(),
            KeyCode::Char('r') | KeyCode::Char('R') => self.refresh(),
            _ => {}
        }
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

    fn refresh(&mut self) {
        let (listings, status) = load_ranked_listings();
        self.listings = listings;
        if self.selected >= self.listings.len() {
            self.selected = self.listings.len().saturating_sub(1);
        }
        self.status = status;
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
                let left = a.scores.as_ref().map_or(0.0, |scores| scores.overall);
                let right = b.scores.as_ref().map_or(0.0, |scores| scores.overall);
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
        };

        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_index(), 0);

        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.selected_index(), 0);
    }
}
