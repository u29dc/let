#![forbid(unsafe_code)]

use ratatui::style::{Color, Modifier, Style};

pub(crate) const ACCENT: Color = Color::Cyan;
pub(crate) const FG: Color = Color::Gray;
pub(crate) const MUTED: Color = Color::DarkGray;
pub(crate) const BORDER: Color = Color::DarkGray;

#[derive(Debug, Clone, Copy)]
pub(crate) struct HeaderContract {
    pub(crate) project_name: &'static str,
    pub(crate) version: &'static str,
    pub(crate) glyph: &'static str,
}

impl HeaderContract {
    pub(crate) fn render(self) -> String {
        format!("{} {} v{}", self.glyph, self.project_name, self.version)
    }
}

pub(crate) const HEADER_CONTRACT: HeaderContract = HeaderContract {
    project_name: "let",
    version: env!("CARGO_PKG_VERSION"),
    glyph: "■",
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) root: Style,
    pub(crate) border: Style,
    pub(crate) brand: Style,
    pub(crate) header_meta: Style,
    pub(crate) section_heading: Style,
    pub(crate) body: Style,
    pub(crate) key: Style,
    pub(crate) selected: Style,
    pub(crate) footer_meta: Style,
    pub(crate) footer_status: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            root: Style::default().fg(FG),
            border: Style::default().fg(BORDER),
            brand: Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            header_meta: Style::default().fg(MUTED),
            section_heading: Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            body: Style::default().fg(FG),
            key: Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            selected: Style::default()
                .fg(Color::Rgb(255, 255, 255))
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
            footer_meta: Style::default().fg(MUTED),
            footer_status: Style::default().fg(MUTED),
        }
    }
}
