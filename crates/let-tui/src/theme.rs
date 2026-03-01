use ratatui::style::{Color, Modifier, Style};

#[derive(Debug)]
pub(crate) struct Theme {
    pub(crate) accent: Color,
    pub(crate) title: Style,
    pub(crate) body: Style,
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
            body: Style::default().fg(Color::Gray),
            footer: Style::default().fg(Color::DarkGray),
        }
    }
}
