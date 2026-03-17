#![forbid(unsafe_code)]

mod app;
mod preview;
mod theme;
mod ui;

use std::io::{self, Stdout};
use std::time::Duration;

use app::App;
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use preview::PreviewController;
use ratatui::{Terminal, backend::CrosstermBackend};
use theme::Theme;

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> AppResult<()> {
    let mut terminal = TerminalGuard::new()?;
    let mut app = App::with_preview(PreviewController::detect());
    let theme = Theme::default();

    while app.is_running() {
        app.tick();
        terminal
            .terminal
            .draw(|frame| ui::render(frame, &mut app, &theme))?;

        if event::poll(Duration::from_millis(app.poll_timeout_ms()))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.on_key(key);
        }
    }

    Ok(())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> AppResult<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
    }
}
