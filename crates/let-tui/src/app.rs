use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug)]
pub(crate) struct App {
    running: bool,
}

impl App {
    pub(crate) fn is_running(&self) -> bool {
        self.running
    }

    pub(crate) fn on_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            self.running = false;
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self { running: true }
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
}
