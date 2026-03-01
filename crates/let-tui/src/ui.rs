#![forbid(unsafe_code)]

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::{app::App, theme::Theme};

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
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(67), Constraint::Percentage(33)])
        .split(area);

    render_listings_table(frame, rows[0], app, theme);
    render_context_panel(frame, rows[1], app, theme);
}

fn render_listings_table(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
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
        Cell::from("flood"),
        Cell::from("garden"),
        Cell::from("pets"),
        Cell::from("type"),
        Cell::from("status"),
        Cell::from("listed"),
        Cell::from("lat"),
        Cell::from("lng"),
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
            let flood = listing
                .area
                .flood_risk
                .level
                .clone()
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
            let listed = listing
                .listed_date
                .as_deref()
                .map(short_date)
                .unwrap_or_else(|| "--".to_owned());
            let lat = format!("{:.4}", listing.location.lat);
            let lng = format!("{:.4}", listing.location.lng);
            let address = truncate(&listing.address, 34);

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
                Cell::from(truncate(&flood, 8)),
                Cell::from(garden),
                Cell::from(pets),
                Cell::from(property_type),
                Cell::from(status),
                Cell::from(listed),
                Cell::from(lat),
                Cell::from(lng),
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
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Min(24),
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

fn render_context_panel(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let lines = if let Some(listing) = app.selected_listing() {
        let media = app.selected_media();
        let mut lines = Vec::new();
        let id = listing
            .portal_ids
            .rightmove
            .as_deref()
            .unwrap_or(listing.id.as_str());

        lines.push(kv_line("id", id, theme));
        lines.push(kv_line("rightmove", &listing.url, theme));
        lines.push(kv_line("maps", &listing.google_maps_url, theme));
        lines.push(kv_line(
            "street view",
            &listing.google_maps_street_view_url,
            theme,
        ));

        if let Some(dir) = media.cache_dir {
            lines.push(kv_line("cache", dir.display().to_string(), theme));
        } else {
            lines.push(kv_line("cache", "--", theme));
        }

        if let Some(path) = media.images.first() {
            lines.push(kv_line("first image", path.display().to_string(), theme));
        } else {
            lines.push(kv_line("first image", "--", theme));
        }
        if let Some(path) = media.floorplan {
            lines.push(kv_line("floorplan", path.display().to_string(), theme));
        }
        if let Some(path) = media.satellite {
            lines.push(kv_line("satellite", path.display().to_string(), theme));
        }
        if let Some(path) = media.street {
            lines.push(kv_line("street map", path.display().to_string(), theme));
        }

        if !listing.notes.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("notes", theme.section_heading)));
            for note in listing.notes.iter().take(6) {
                lines.push(Line::from(Span::styled(
                    format!("- {}", truncate(note, 160)),
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
                "reasoning",
                truncate(&assessment.reasoning, 200),
                theme,
            ));
            if let Some(tradeoffs) = &assessment.tradeoffs {
                lines.push(kv_line("tradeoffs", truncate(tradeoffs, 200), theme));
            }
            if let Some(neighborhood) = &assessment.neighborhood_analysis {
                lines.push(kv_line("neighborhood", truncate(neighborhood, 200), theme));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("enter ", theme.section_heading),
            Span::styled("quick look image ", theme.footer_meta),
            Span::styled("cmd/ctrl+p ", theme.section_heading),
            Span::styled("listing actions", theme.footer_meta),
        ]));
        lines
    } else {
        vec![Line::from(Span::styled(
            "No listing selected",
            theme.footer_meta,
        ))]
    };

    let panel = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(Span::styled(" context ", theme.header_meta)),
    );
    frame.render_widget(panel, area);
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
        Span::styled("j/k ", theme.section_heading),
        Span::styled("rows ", theme.footer_meta),
        Span::styled("g/G ", theme.section_heading),
        Span::styled("jump ", theme.footer_meta),
        Span::styled("enter ", theme.section_heading),
        Span::styled("quicklook ", theme.footer_meta),
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
