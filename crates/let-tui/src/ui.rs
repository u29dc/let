#![forbid(unsafe_code)]

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
};

use crate::{app::App, theme::Theme};

pub(crate) fn render(frame: &mut Frame<'_>, app: &App, theme: &Theme) {
    let root = Block::default().style(theme.root);
    frame.render_widget(root, frame.area());

    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, layout[0], app, theme);
    render_body(frame, layout[1], app, theme);
    render_footer(frame, layout[2], app, theme);

    if app.palette_open() {
        render_palette(frame, app, theme);
    }
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(24),
            Constraint::Min(10),
            Constraint::Length(36),
        ])
        .split(area);

    let brand = Paragraph::new(Line::from(Span::styled(app.header_text(), theme.brand)));
    frame.render_widget(brand, chunks[0]);

    let center = Paragraph::new(Line::from(Span::styled(
        app.route_context(),
        theme.header_meta,
    )))
    .alignment(Alignment::Center);
    frame.render_widget(center, chunks[1]);

    let right = Paragraph::new(Line::from(Span::styled(
        "cmd+p | ctrl+p command palette",
        theme.header_meta,
    )))
    .alignment(Alignment::Right);
    frame.render_widget(right, chunks[2]);
}

fn render_body(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    render_listings_table(frame, columns[0], app, theme);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(columns[1]);
    render_detail(frame, right[0], app, theme);
    render_sources(frame, right[1], app, theme);
}

fn render_listings_table(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let header = Row::new([
        Cell::from("id"),
        Cell::from("address"),
        Cell::from("price"),
        Cell::from("bed"),
        Cell::from("algo"),
        Cell::from("assess"),
    ])
    .style(theme.section_heading)
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
                .map(|value| format!("{:.0}", value))
                .unwrap_or_else(|| "--".to_owned());

            Row::new([
                Cell::from(id.to_owned()),
                Cell::from(address),
                Cell::from(price),
                Cell::from(listing.bedrooms.to_string()),
                Cell::from(score),
                Cell::from(assessed),
            ])
            .style(theme.body)
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
    .row_highlight_style(theme.selected)
    .highlight_symbol("▌ ")
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(Span::styled(
                format!(" listings ({}) ", app.listings().len()),
                theme.header_meta,
            )),
    );

    let mut state = ratatui::widgets::TableState::default();
    if !app.listings().is_empty() {
        state.select(Some(app.selected_index()));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_detail(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let lines = if let Some(listing) = app.selected_listing() {
        let id = listing
            .portal_ids
            .rightmove
            .as_deref()
            .unwrap_or(listing.id.as_str());
        let station = listing
            .nearest_stations
            .first()
            .map(|item| format!("{} ({:.1} {})", item.name, item.distance, item.unit))
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
            kv_line("id", id, theme),
            kv_line("address", &listing.address, theme),
            kv_line("region", listing.region.as_deref().unwrap_or("--"), theme),
            kv_line("price", &listing.price_display, theme),
            kv_line(
                "beds/baths",
                format!("{}/{}", listing.bedrooms, listing.bathrooms),
                theme,
            ),
            kv_line("algo/assess", format!("{score} / {assessed}"), theme),
            kv_line("confidence", &confidence, theme),
            kv_line("station", &station, theme),
            kv_line("postcode", &listing.postcode, theme),
            kv_line("status", status, theme),
            kv_line("fetched", &listing.fetched_at, theme),
            Line::from(""),
            kv_line("url", &listing.url, theme),
        ]
    } else {
        vec![Line::from(Span::styled(
            "No listings loaded",
            theme.footer_meta,
        ))]
    };

    let panel = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(Span::styled(" selected ", theme.header_meta)),
    );
    frame.render_widget(panel, area);
}

fn render_sources(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let header = Row::new([
        Cell::from("source"),
        Cell::from("status"),
        Cell::from("size mb"),
    ])
    .style(theme.section_heading);

    let rows = app
        .source_status()
        .iter()
        .map(|source| {
            let status = if source.exists { "ready" } else { "missing" };
            let size = if source.exists {
                format!("{:.1}", source.size_mb)
            } else {
                "--".to_owned()
            };

            Row::new([
                Cell::from(source.name.clone()),
                Cell::from(status),
                Cell::from(size),
            ])
            .style(theme.body)
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Length(9),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(Span::styled(" sources ", theme.header_meta)),
    );
    frame.render_widget(table, area);
}

fn render_palette(frame: &mut Frame<'_>, app: &App, theme: &Theme) {
    let area = centered_rect(74, 62, frame.area());
    frame.render_widget(Clear, area);

    let container = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.brand)
        .title(Span::styled(" Command Palette ", theme.section_heading));
    let inner = container.inner(area);
    frame.render_widget(container, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .split(inner);

    let query = Paragraph::new(Line::from(vec![
        Span::styled("> ", theme.section_heading),
        Span::styled(app.palette_query(), theme.body),
    ]));
    frame.render_widget(query, sections[0]);

    let divider = Paragraph::new(Line::from(Span::styled("actions", theme.header_meta)));
    frame.render_widget(divider, sections[1]);

    let rows = app
        .palette_items()
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let style = if idx == app.palette_selected_index() {
                theme.selected
            } else {
                theme.body
            };
            Row::new([Cell::from((*item).to_owned())]).style(style)
        })
        .collect::<Vec<_>>();

    let table = Table::new(rows, [Constraint::Percentage(100)]).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(Span::styled(" commands ", theme.header_meta)),
    );
    frame.render_widget(table, sections[2]);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(56)])
        .split(area);

    let left = Paragraph::new(Line::from(vec![
        Span::styled("j/k ", theme.section_heading),
        Span::styled("rows ", theme.footer_meta),
        Span::styled("g/G ", theme.section_heading),
        Span::styled("jump ", theme.footer_meta),
        Span::styled(": ", theme.section_heading),
        Span::styled("palette ", theme.footer_meta),
        Span::styled("cmd/ctrl+p ", theme.section_heading),
        Span::styled("palette ", theme.footer_meta),
        Span::styled("r ", theme.section_heading),
        Span::styled("refresh ", theme.footer_meta),
        Span::styled("q ", theme.section_heading),
        Span::styled("quit", theme.footer_meta),
    ]));
    frame.render_widget(left, chunks[0]);

    let right = Paragraph::new(Line::from(Span::styled(app.status(), theme.footer_status)))
        .alignment(Alignment::Right);
    frame.render_widget(right, chunks[1]);
}

fn kv_line(key: &str, value: impl Into<String>, theme: &Theme) -> Line<'static> {
    let value = value.into();
    Line::from(vec![
        Span::styled(format!("{key}: "), theme.key),
        Span::styled(value, theme.body),
    ])
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
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
