#![forbid(unsafe_code)]

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::{app::App, theme::Theme};

pub(crate) fn render(frame: &mut Frame<'_>, app: &App, theme: &Theme) {
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, layout[0], theme);
    render_body(frame, layout[1], app, theme);
    render_footer(frame, layout[2], app, theme);
}

fn render_header(frame: &mut Frame<'_>, area: ratatui::layout::Rect, theme: &Theme) {
    let line = Line::from(vec![
        Span::styled("■", Style::default().fg(theme.accent)),
        Span::raw(" "),
        Span::styled("let", theme.title),
        Span::raw(" "),
        Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(theme.accent),
        ),
        Span::raw("  "),
        Span::styled("rental search cockpit", theme.muted),
    ]);

    let header = Paragraph::new(line).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, area);
}

fn render_body(frame: &mut Frame<'_>, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    render_listings_table(frame, columns[0], app, theme);
    render_detail(frame, columns[1], app, theme);
}

fn render_listings_table(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    app: &App,
    theme: &Theme,
) {
    let header = Row::new([
        Cell::from("ID"),
        Cell::from("ADDRESS"),
        Cell::from("PRICE"),
        Cell::from("BED"),
        Cell::from("ALGO"),
        Cell::from("ASSESS"),
    ])
    .style(theme.table_header)
    .height(1);

    let rows = app
        .listings()
        .iter()
        .take(150)
        .map(|listing| {
            let id = listing
                .portal_ids
                .rightmove
                .as_deref()
                .unwrap_or(listing.id.as_str());
            let address = truncate(&listing.address, 34);
            let price = format!("£{}", listing.price);
            let score = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:.0}", scores.overall))
                .unwrap_or_else(|| "--".to_owned());
            let assessed = listing
                .assessed_score
                .map(|score| format!("{:.0}", score))
                .unwrap_or_else(|| "--".to_owned());

            Row::new([
                Cell::from(id.to_owned()),
                Cell::from(address),
                Cell::from(price),
                Cell::from(listing.bedrooms.to_string()),
                Cell::from(score),
                Cell::from(assessed),
            ])
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Min(26),
            Constraint::Length(9),
            Constraint::Length(4),
            Constraint::Length(6),
            Constraint::Length(7),
        ],
    )
    .header(header)
    .row_highlight_style(theme.selected_row)
    .highlight_symbol("▐ ")
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Listings ({})", app.listings().len())),
    );

    let mut state = ratatui::widgets::TableState::default();
    if !app.listings().is_empty() {
        state.select(Some(app.selected_index()));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame<'_>, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let content = if let Some(listing) = app.selected_listing() {
        let id = listing
            .portal_ids
            .rightmove
            .as_deref()
            .unwrap_or(listing.id.as_str());
        let station = listing
            .nearest_stations
            .first()
            .map(|station| {
                format!(
                    "{} ({:.1} {})",
                    station.name, station.distance, station.unit
                )
            })
            .unwrap_or_else(|| "--".to_owned());
        let score = listing
            .scores
            .as_ref()
            .map(|scores| format!("{:.1}", scores.overall))
            .unwrap_or_else(|| "--".to_owned());
        let confidence = listing
            .scores
            .as_ref()
            .map(|scores| format!("{:.0}%", scores.confidence * 100.0))
            .unwrap_or_else(|| "--".to_owned());
        let assessed = listing
            .assessed_score
            .map(|value| format!("{:.1}", value))
            .unwrap_or_else(|| "--".to_owned());
        let status = match listing.status {
            let_sdk::schema::listing::ListingStatus::Active => "active",
            let_sdk::schema::listing::ListingStatus::Inactive => "inactive",
        };

        vec![
            Line::from(vec![
                Span::styled("ID: ", theme.key),
                Span::styled(id, theme.value),
            ]),
            Line::from(vec![
                Span::styled("Address: ", theme.key),
                Span::styled(&listing.address, theme.value),
            ]),
            Line::from(vec![
                Span::styled("Region: ", theme.key),
                Span::styled(listing.region.as_deref().unwrap_or("--"), theme.value),
            ]),
            Line::from(vec![
                Span::styled("Price: ", theme.key),
                Span::styled(&listing.price_display, theme.value),
            ]),
            Line::from(vec![
                Span::styled("Beds/Baths: ", theme.key),
                Span::styled(
                    format!("{}/{}", listing.bedrooms, listing.bathrooms),
                    theme.value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Algo/Assess: ", theme.key),
                Span::styled(format!("{score} / {assessed}"), theme.value),
            ]),
            Line::from(vec![
                Span::styled("Confidence: ", theme.key),
                Span::styled(confidence, theme.value),
            ]),
            Line::from(vec![
                Span::styled("Station: ", theme.key),
                Span::styled(station, theme.value),
            ]),
            Line::from(vec![
                Span::styled("Postcode: ", theme.key),
                Span::styled(&listing.postcode, theme.value),
            ]),
            Line::from(vec![
                Span::styled("Status: ", theme.key),
                Span::styled(status, theme.value),
            ]),
            Line::from(vec![
                Span::styled("Fetched: ", theme.key),
                Span::styled(&listing.fetched_at, theme.value),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("URL: ", theme.key),
                Span::styled(&listing.url, theme.value),
            ]),
        ]
    } else {
        vec![Line::styled("No listings loaded", theme.muted)]
    };

    let panel =
        Paragraph::new(content).block(Block::default().borders(Borders::ALL).title("Detail"));
    frame.render_widget(panel, area);
}

fn render_footer(frame: &mut Frame<'_>, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let footer = Line::from(vec![
        Span::styled("j/k", theme.accent_key),
        Span::styled(" move ", theme.footer),
        Span::styled("g/G", theme.accent_key),
        Span::styled(" top/bottom ", theme.footer),
        Span::styled("r", theme.accent_key),
        Span::styled(" refresh ", theme.footer),
        Span::styled("q", theme.accent_key),
        Span::styled(" quit  ", theme.footer),
        Span::styled(app.status(), theme.muted),
    ]);

    let footer = Paragraph::new(footer).block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, area);
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_owned();
    }

    let mut out = String::with_capacity(max + 1);
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max.saturating_sub(1) {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}
