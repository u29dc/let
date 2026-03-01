use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{app::App, theme::Theme};

pub(crate) fn render(frame: &mut Frame<'_>, _app: &App, theme: &Theme) {
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Line::from(vec![
        Span::styled("■", Style::default().fg(theme.accent)),
        Span::raw(" "),
        Span::styled("let ", theme.title),
        Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(theme.accent),
        ),
    ]);

    let header = Paragraph::new(header).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, layout[0]);

    let body_lines = vec![
        Line::styled(
            "Search, fetch, enrich, and assess rental listings from one terminal session.",
            theme.body,
        ),
        Line::raw(""),
        Line::raw("Pipeline placeholders:"),
        Line::raw("  search discover   -> identify candidate region IDs"),
        Line::raw("  search diff       -> classify new/known portal IDs"),
        Line::raw("  fetch             -> parse + enrich + score listings"),
        Line::raw("  assess context    -> inspect media, maps, and local metrics"),
        Line::raw("  assess submit     -> persist verdict + notes"),
        Line::raw("  export json       -> emit snapshot for downstream tooling"),
        Line::raw(""),
        Line::raw("Status: skeleton mode (read-only placeholder UI)"),
    ];

    let body = Paragraph::new(body_lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Workspace"));
    frame.render_widget(body, layout[1]);

    let footer = Line::from(vec![
        Span::styled("[q]", Style::default().fg(theme.accent)),
        Span::styled(" quit", theme.footer),
        Span::raw("  "),
        Span::styled("[r]", Style::default().fg(theme.accent)),
        Span::styled(" refresh (todo)", theme.footer),
        Span::raw("  "),
        Span::styled("[/]", Style::default().fg(theme.accent)),
        Span::styled(" search (todo)", theme.footer),
    ]);

    let footer = Paragraph::new(footer).block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, layout[2]);
}
