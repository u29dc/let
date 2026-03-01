#![forbid(unsafe_code)]

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug)]
pub(crate) struct Theme {
    pub(crate) accent: Color,
    pub(crate) title: Style,
    pub(crate) key: Style,
    pub(crate) value: Style,
    pub(crate) table_header: Style,
    pub(crate) selected_row: Style,
    pub(crate) accent_key: Style,
    pub(crate) muted: Style,
    pub(crate) footer: Style,
}

impl Default for Theme {
    fn default() -> Self {
        let accent = Color::Cyan;
        Self {
            accent,
            title: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            key: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            value: Style::default().fg(Color::White),
            table_header: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            selected_row: Style::default().bg(Color::Rgb(0, 35, 35)),
            accent_key: Style::default().fg(accent).add_modifier(Modifier::BOLD),
            muted: Style::default().fg(Color::Gray),
            footer: Style::default().fg(Color::DarkGray),
        }
    }
}
