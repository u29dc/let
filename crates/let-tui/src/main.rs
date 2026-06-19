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
        let mut setup = TerminalSetupGuard::default();
        setup.enable_raw_mode()?;
        let mut stdout = io::stdout();
        setup.enter_alternate_screen(&mut stdout)?;
        setup.hide_cursor(&mut stdout)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        setup.disarm();

        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
    }
}

#[derive(Default)]
struct TerminalSetupGuard {
    raw_mode: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
}

impl TerminalSetupGuard {
    fn enable_raw_mode(&mut self) -> AppResult<()> {
        terminal::enable_raw_mode()?;
        self.raw_mode = true;
        Ok(())
    }

    fn enter_alternate_screen(&mut self, stdout: &mut Stdout) -> AppResult<()> {
        execute!(stdout, EnterAlternateScreen)?;
        self.alternate_screen = true;
        Ok(())
    }

    fn hide_cursor(&mut self, stdout: &mut Stdout) -> AppResult<()> {
        execute!(stdout, Hide)?;
        self.cursor_hidden = true;
        Ok(())
    }

    fn disarm(&mut self) {
        self.raw_mode = false;
        self.alternate_screen = false;
        self.cursor_hidden = false;
    }
}

impl Drop for TerminalSetupGuard {
    fn drop(&mut self) {
        if self.cursor_hidden || self.alternate_screen {
            let _ = execute!(io::stdout(), Show);
        }
        if self.alternate_screen {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
        if self.raw_mode {
            let _ = terminal::disable_raw_mode();
        }
    }
}
