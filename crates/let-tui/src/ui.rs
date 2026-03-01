#![forbid(unsafe_code)]

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::{
    app::{App, FocusPane},
    theme::Theme,
};

pub(crate) fn render(frame: &mut Frame<'_>, app: &App, theme: &Theme) {
    let root = Block::default().style(theme.root);
    frame.render_widget(root, frame.area());

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(1),
        ])
        .split(frame.area());

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
        .constraints([Constraint::Percentage(72), Constraint::Percentage(28)])
        .split(area);

    render_listings_table(frame, columns[0], app, theme);
    render_context_panel(frame, columns[1], app, theme);
}

fn render_listings_table(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let list_focused = app.focus() == FocusPane::Listings;
    let header = Row::new([
        Cell::from("id"),
        Cell::from("region"),
        Cell::from("price"),
        Cell::from("dep"),
        Cell::from("avail"),
        Cell::from("bed"),
        Cell::from("bath"),
        Cell::from("algo"),
        Cell::from("assess"),
        Cell::from("aff"),
        Cell::from("loc"),
        Cell::from("live"),
        Cell::from("conf"),
        Cell::from("epc"),
        Cell::from("sqm"),
        Cell::from("stn"),
        Cell::from("gig"),
        Cell::from("crime"),
        Cell::from("imd"),
        Cell::from("garden"),
        Cell::from("pets"),
        Cell::from("type"),
        Cell::from("status"),
        Cell::from("address"),
    ])
    .style(theme.section_heading)
    .height(1);

    let rows = app
        .listings()
        .iter()
        .take(300)
        .map(|listing| {
            let id = listing
                .portal_ids
                .rightmove
                .as_deref()
                .unwrap_or(listing.id.as_str())
                .to_owned();
            let region = listing.region.clone().unwrap_or_else(|| "--".to_owned());
            let price = format!("£{}", listing.price);
            let deposit = listing
                .lettings
                .deposit
                .map(|value| format!("£{value}"))
                .unwrap_or_else(|| "--".to_owned());
            let available = listing
                .lettings
                .available_date
                .as_deref()
                .map(short_date)
                .unwrap_or_else(|| "--".to_owned());
            let beds = listing.bedrooms.to_string();
            let baths = listing.bathrooms.to_string();
            let algo = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:.0}", scores.overall))
                .unwrap_or_else(|| "--".to_owned());
            let assess = listing
                .assessed_score
                .map(|value| format!("{:.0}", value))
                .unwrap_or_else(|| "--".to_owned());
            let affordability = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:.0}", scores.affordability))
                .unwrap_or_else(|| "--".to_owned());
            let location = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:.0}", scores.location))
                .unwrap_or_else(|| "--".to_owned());
            let liveability = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:.0}", scores.liveability))
                .unwrap_or_else(|| "--".to_owned());
            let confidence = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:.0}%", scores.confidence * 100.0))
                .unwrap_or_else(|| "--".to_owned());
            let epc = listing
                .epc_rating
                .as_ref()
                .map(epc_band_label)
                .unwrap_or_else(|| "--".to_owned());
            let sqm = listing
                .floor_area_sqm
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| "--".to_owned());
            let station = listing
                .nearest_stations
                .first()
                .map(|item| format!("{:.1}", item.distance))
                .unwrap_or_else(|| "--".to_owned());
            let gigabit = listing
                .gigabit_availability
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| "--".to_owned());
            let crime = listing
                .area
                .crime
                .rate_per_1k
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "--".to_owned());
            let imd = listing
                .area
                .imd
                .decile
                .map(|value| value.to_string())
                .unwrap_or_else(|| "--".to_owned());
            let garden = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:?}", scores.factors.garden_type))
                .unwrap_or_else(|| "--".to_owned())
                .to_lowercase();
            let pets = listing
                .scores
                .as_ref()
                .map(|scores| format!("{:?}", scores.factors.pet_policy))
                .unwrap_or_else(|| "--".to_owned())
                .to_lowercase();
            let property_type = truncate(&listing.property_type, 10);
            let status = format!("{:?}", listing.status).to_lowercase();
            let address = truncate(&listing.address, 44);

            Row::new([
                Cell::from(id),
                Cell::from(truncate(&region, 12)),
                Cell::from(price),
                Cell::from(deposit),
                Cell::from(available),
                Cell::from(beds),
                Cell::from(baths),
                Cell::from(algo),
                Cell::from(assess),
                Cell::from(affordability),
                Cell::from(location),
                Cell::from(liveability),
                Cell::from(confidence),
                Cell::from(epc),
                Cell::from(sqm),
                Cell::from(station),
                Cell::from(gigabit),
                Cell::from(crime),
                Cell::from(imd),
                Cell::from(garden),
                Cell::from(pets),
                Cell::from(property_type),
                Cell::from(status),
                Cell::from(address),
            ])
            .style(theme.body)
        })
        .collect::<Vec<_>>();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(36),
        ],
    )
    .header(header)
    .row_highlight_style(if list_focused {
        theme.selected
    } else {
        theme.key
    })
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

fn render_context_panel(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let context_focused = app.focus() == FocusPane::Context;
    let media_items = app.context_rows();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let summary_lines = if let Some(listing) = app.selected_listing() {
        let mut lines = Vec::new();
        let id = listing
            .portal_ids
            .rightmove
            .as_deref()
            .unwrap_or(listing.id.as_str());
        let cache = app
            .selected_media()
            .cache_dir
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "--".to_owned());

        lines.push(kv_line("id", id, theme));
        lines.push(kv_line("rightmove", truncate(&listing.url, 128), theme));
        lines.push(kv_line(
            "maps",
            truncate(&listing.google_maps_url, 128),
            theme,
        ));
        lines.push(kv_line("cache", truncate(&cache, 128), theme));
        lines.push(kv_line("media items", media_items.len().to_string(), theme));

        if !listing.notes.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("notes", theme.section_heading)));
            for note in listing.notes.iter().take(3) {
                lines.push(Line::from(Span::styled(
                    format!("- {}", truncate(note, 140)),
                    theme.body,
                )));
            }
        }

        if let Some(assessment) = &listing.assessment {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "assessment",
                theme.section_heading,
            )));
            lines.push(kv_line(
                "recommendation",
                format!("{:?}", assessment.recommendation).to_lowercase(),
                theme,
            ));
            lines.push(kv_line(
                "maintenance",
                format!("{:?}", assessment.maintenance).to_lowercase(),
                theme,
            ));
            lines.push(kv_line(
                "family",
                format!("{:?}", assessment.family_suitability).to_lowercase(),
                theme,
            ));
            lines.push(kv_line(
                "score adjustment",
                format!("{:.1}", assessment.score_adjustment),
                theme,
            ));
            lines.push(kv_line(
                "light/space",
                truncate(&assessment.light_and_space, 220),
                theme,
            ));
            lines.push(kv_line(
                "photo analysis",
                truncate(&assessment.photo_analysis, 220),
                theme,
            ));
            lines.push(kv_line(
                "reasoning",
                truncate(&assessment.reasoning, 220),
                theme,
            ));
            if let Some(tradeoffs) = &assessment.tradeoffs {
                lines.push(kv_line("tradeoffs", truncate(tradeoffs, 220), theme));
            }
            if let Some(neighborhood) = &assessment.neighborhood_analysis {
                lines.push(kv_line("neighborhood", truncate(neighborhood, 220), theme));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("tab ", theme.section_heading),
            Span::styled("switch pane ", theme.footer_meta),
            Span::styled("enter ", theme.section_heading),
            Span::styled("quicklook selected media", theme.footer_meta),
        ]));
        lines
    } else {
        vec![Line::from(Span::styled(
            "No listing selected",
            theme.footer_meta,
        ))]
    };

    let summary = Paragraph::new(summary_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border)
                .title(Span::styled(" context ", theme.header_meta)),
        );
    frame.render_widget(summary, sections[0]);

    let media_rows = media_items
        .iter()
        .map(|(kind, asset)| {
            Row::new([
                Cell::from(truncate(kind, 14)),
                Cell::from(truncate(asset, 120)),
            ])
            .style(theme.body)
        })
        .collect::<Vec<_>>();

    let media_table = Table::new(media_rows, [Constraint::Length(14), Constraint::Min(12)])
        .header(Row::new([Cell::from("type"), Cell::from("asset")]).style(theme.section_heading))
        .row_highlight_style(if context_focused {
            theme.selected
        } else {
            theme.key
        })
        .highlight_symbol("▌ ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border)
                .title(Span::styled(" media (enter quicklook) ", theme.header_meta)),
        );

    let mut media_state = ratatui::widgets::TableState::default();
    if !media_items.is_empty() {
        media_state.select(Some(app.context_selected_index()));
    }
    frame.render_stateful_widget(media_table, sections[1], &mut media_state);
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
        .palette_rows()
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let style = if index == app.palette_selected_index() {
                theme.selected
            } else {
                theme.body
            };
            Row::new([Cell::from(row.clone())]).style(style)
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
    let (ready, total) = app.source_health_counts();
    let source_summary = truncate(&app.source_summary(), 132);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(144)])
        .split(area);

    let left = Paragraph::new(Line::from(vec![
        Span::styled("tab ", theme.section_heading),
        Span::styled("focus ", theme.footer_meta),
        Span::styled("j/k ", theme.section_heading),
        Span::styled("rows ", theme.footer_meta),
        Span::styled("g/G ", theme.section_heading),
        Span::styled("jump ", theme.footer_meta),
        Span::styled("enter ", theme.section_heading),
        Span::styled("open/quicklook ", theme.footer_meta),
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

    let right = Paragraph::new(Line::from(Span::styled(
        format!(
            "sources:{ready}/{total} | {source_summary} | {}",
            app.status()
        ),
        theme.footer_status,
    )))
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

fn epc_band_label(band: &let_sdk::schema::listing::EpcBand) -> String {
    match band {
        let_sdk::schema::listing::EpcBand::A => "A",
        let_sdk::schema::listing::EpcBand::B => "B",
        let_sdk::schema::listing::EpcBand::C => "C",
        let_sdk::schema::listing::EpcBand::D => "D",
        let_sdk::schema::listing::EpcBand::E => "E",
        let_sdk::schema::listing::EpcBand::F => "F",
        let_sdk::schema::listing::EpcBand::G => "G",
    }
    .to_owned()
}

fn short_date(value: &str) -> String {
    value.chars().take(10).collect::<String>()
}

fn centered_rect(percent_x: u16, percent_y: u16, rect: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(rect);

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
    for (index, ch) in input.chars().enumerate() {
        if index >= max.saturating_sub(1) {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}
