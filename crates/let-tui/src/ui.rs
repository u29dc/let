#![forbid(unsafe_code)]

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::{
    app::{App, FocusPane},
    preview::PreviewGraphicsClear,
    theme::Theme,
};

pub(crate) fn render(frame: &mut Frame<'_>, app: &mut App, theme: &Theme) {
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

fn render_body(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
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
        Cell::from("score"),
        Cell::from("price"),
        Cell::from("dep"),
        Cell::from("avail"),
        Cell::from("bed"),
        Cell::from("bath"),
        Cell::from("epc"),
        Cell::from("sqm"),
        Cell::from("stn"),
        Cell::from("gig"),
        Cell::from("crime"),
        Cell::from("imd"),
        Cell::from("type"),
        Cell::from("status"),
        Cell::from("address"),
    ])
    .style(theme.section_heading)
    .height(1);

    let visible_limit = 300usize;
    let selected_index = app.selected_index();
    let visible_start = selected_index
        .saturating_add(1)
        .saturating_sub(visible_limit);
    let rows = app
        .listings()
        .iter()
        .skip(visible_start)
        .take(visible_limit)
        .map(|listing| {
            let id = listing.rightmove_id.clone();
            let region = listing.region.clone().unwrap_or_else(|| "--".to_owned());
            let score = listing
                .score_overall
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| "--".to_owned());
            let price = format!("£{}", listing.price_pcm);
            let deposit = listing
                .deposit
                .map(|value| format!("£{value}"))
                .unwrap_or_else(|| "--".to_owned());
            let available = listing
                .available_date
                .as_deref()
                .map(short_date)
                .unwrap_or_else(|| "--".to_owned());
            let beds = listing.bedrooms.to_string();
            let baths = listing.bathrooms.to_string();
            let epc = listing
                .epc_rating
                .clone()
                .unwrap_or_else(|| "--".to_owned());
            let sqm = listing
                .floor_area_sqm
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| "--".to_owned());
            let station = listing
                .nearest_station_miles
                .map(|distance| format!("{distance:.1}"))
                .unwrap_or_else(|| "--".to_owned());
            let gigabit = listing
                .gigabit_availability
                .map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| "--".to_owned());
            let crime = listing
                .crime_rate_per_1k
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "--".to_owned());
            let imd = listing
                .imd_decile
                .map(|value| value.to_string())
                .unwrap_or_else(|| "--".to_owned());
            let property_type = truncate(&listing.property_type, 10);
            let status = listing.page_status.clone();
            let address = truncate(&listing.address, 44);

            Row::new([
                Cell::from(id),
                Cell::from(truncate(&region, 12)),
                Cell::from(score),
                Cell::from(price),
                Cell::from(deposit),
                Cell::from(available),
                Cell::from(beds),
                Cell::from(baths),
                Cell::from(epc),
                Cell::from(sqm),
                Cell::from(station),
                Cell::from(gigabit),
                Cell::from(crime),
                Cell::from(imd),
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
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(3),
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
        state.select(Some(selected_index.saturating_sub(visible_start)));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_context_panel(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let context_focused = app.focus() == FocusPane::Context;
    let layout = context_layout(area, app.preview_preferred_block_height(area.width));
    let summary_lines = build_context_summary_lines(app, theme, layout.compact_summary);

    let content_rows = wrapped_line_count(&summary_lines, layout.summary.width);
    app.set_context_summary_viewport(layout.summary, content_rows, layout.summary.height as usize);
    let (summary_offset, summary_max_offset) = app.context_summary_scroll_position();
    let summary_title = if summary_max_offset > 0 {
        format!(
            " context {}/{} pgup/pgdn ",
            summary_offset.saturating_add(1),
            summary_max_offset.saturating_add(1)
        )
    } else {
        " context ".to_owned()
    };
    let summary = Paragraph::new(summary_lines)
        .wrap(Wrap { trim: false })
        .scroll((app.context_summary_scroll_offset(), 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme.border)
                .title(Span::styled(summary_title, theme.header_meta)),
        );
    frame.render_widget(summary, layout.summary);

    if let Some(preview_area) = layout.preview {
        render_preview_panel(frame, preview_area, app, theme);
    }

    let media_items = app.context_items();
    frame.render_widget(Clear, layout.media);
    let media_rows = media_items
        .iter()
        .map(|item| {
            Row::new([
                Cell::from(truncate(&item.kind, 14)),
                Cell::from(truncate(&item.asset, 120)),
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
                .title(Span::styled(
                    format!(
                        " media {}/{} (enter quicklook) ",
                        app.context_selected_index()
                            .saturating_add(1)
                            .min(media_items.len()),
                        media_items.len()
                    ),
                    theme.header_meta,
                )),
        );

    let mut media_state =
        ratatui::widgets::TableState::default().with_offset(app.context_scroll_offset());
    if !media_items.is_empty() {
        media_state.select(Some(app.context_selected_index()));
    }
    frame.render_stateful_widget(media_table, layout.media, &mut media_state);
    app.set_context_scroll_offset(media_state.offset());
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
        Span::styled("m ", theme.section_heading),
        Span::styled(
            format!("preview:{} ", app.preview_mode_label()),
            theme.footer_meta,
        ),
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

#[derive(Debug, Clone, Copy)]
struct ContextLayout {
    summary: Rect,
    preview: Option<Rect>,
    media: Rect,
    compact_summary: bool,
}

fn context_layout(area: Rect, preview_block_height: u16) -> ContextLayout {
    const SUMMARY_MIN: u16 = 8;
    const MEDIA_MIN: u16 = 7;
    const PREVIEW_MIN: u16 = 8;
    const PREVIEW_MAX: u16 = 18;
    const PREVIEW_MIN_WIDTH: u16 = 26;

    let can_show_preview = area.width >= PREVIEW_MIN_WIDTH
        && area.height
            >= SUMMARY_MIN
                .saturating_add(MEDIA_MIN)
                .saturating_add(PREVIEW_MIN);

    if !can_show_preview {
        let summary_height = area.height.saturating_mul(3).saturating_div(5).clamp(
            SUMMARY_MIN.min(area.height),
            area.height.saturating_sub(3).max(1),
        );
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(summary_height), Constraint::Min(3)])
            .split(area);
        return ContextLayout {
            summary: sections[0],
            preview: None,
            media: sections[1],
            compact_summary: area.height < 22,
        };
    }

    let min_preview = PREVIEW_MIN.min(area.height);
    let mut summary_height = area.height.saturating_mul(45).saturating_div(100).clamp(
        SUMMARY_MIN.min(area.height),
        area.height
            .saturating_sub(MEDIA_MIN)
            .max(SUMMARY_MIN.min(area.height)),
    );
    let mut preview_height = preview_block_height.clamp(min_preview, PREVIEW_MAX);
    let mut media_height = area.height.saturating_sub(summary_height + preview_height);

    if media_height < MEDIA_MIN {
        let deficit = MEDIA_MIN - media_height;
        let shrink_summary = deficit.min(summary_height.saturating_sub(SUMMARY_MIN));
        summary_height = summary_height.saturating_sub(shrink_summary);
        let remaining = deficit.saturating_sub(shrink_summary);
        preview_height = preview_height
            .saturating_sub(remaining.min(preview_height.saturating_sub(min_preview)));
        media_height = area.height.saturating_sub(summary_height + preview_height);
    }

    if media_height < MEDIA_MIN {
        let fallback = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(summary_height), Constraint::Min(3)])
            .split(area);
        return ContextLayout {
            summary: fallback[0],
            preview: None,
            media: fallback[1],
            compact_summary: true,
        };
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(summary_height),
            Constraint::Length(preview_height),
            Constraint::Min(media_height),
        ])
        .split(area);

    ContextLayout {
        summary: sections[0],
        preview: Some(sections[1]),
        media: sections[2],
        compact_summary: true,
    }
}

fn wrapped_line_count(lines: &[Line<'_>], block_width: u16) -> usize {
    let inner_width = block_width.saturating_sub(2).max(1) as usize;
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(inner_width))
        .sum::<usize>()
        .saturating_add(2)
}

fn render_preview_panel(frame: &mut Frame<'_>, area: Rect, app: &mut App, theme: &Theme) {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    app.sync_preview(inner);
    let preview = app.preview_view();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Span::styled(preview.title, theme.header_meta));
    frame.render_widget(block, area);
    frame.render_widget(Clear, inner);

    if let Some(protocol) = preview.protocol {
        frame.render_widget(protocol, inner);
        return;
    }

    if preview.clear_graphics {
        frame.render_widget(PreviewGraphicsClear, inner);
    }

    if preview.lines.is_empty() {
        return;
    }

    let placeholder = Paragraph::new(preview.lines)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(placeholder, inner);
}

fn build_context_summary_lines(app: &App, theme: &Theme, compact: bool) -> Vec<Line<'static>> {
    let Some(listing) = app.selected_listing() else {
        return vec![Line::from(Span::styled(
            "No listing selected",
            theme.footer_meta,
        ))];
    };

    let mut lines = Vec::new();
    let id = listing.rightmove_id.as_str();
    let cache = app
        .selected_media()
        .cache_dir
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "--".to_owned());
    let media_count = app.context_items().len();

    lines.push(kv_line("id", id, theme));
    append_listing_detail_lines(&mut lines, listing, cache, media_count, theme, compact);

    if let Some(bundle) = app.selected_bundle() {
        append_bundle_assessment_lines(&mut lines, bundle, theme, compact);
        append_bundle_evidence_lines(&mut lines, bundle, theme, compact);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("tab ", theme.section_heading),
        Span::styled("switch pane ", theme.footer_meta),
        Span::styled("enter ", theme.section_heading),
        Span::styled("quicklook media ", theme.footer_meta),
        Span::styled("pgup/pgdn ", theme.section_heading),
        Span::styled("scroll context ", theme.footer_meta),
        Span::styled("m ", theme.section_heading),
        Span::styled("cycle preview mode", theme.footer_meta),
    ]));
    lines
}

fn append_listing_detail_lines(
    lines: &mut Vec<Line<'static>>,
    listing: &crate::listing::TuiListingRow,
    cache: String,
    media_count: usize,
    theme: &Theme,
    compact: bool,
) {
    let max = if compact { 112 } else { 180 };
    let price = if listing.price_display.trim().is_empty() {
        format!("£{} pcm", listing.price_pcm)
    } else {
        listing.price_display.clone()
    };
    let deposit = listing
        .deposit
        .map(|value| format!("£{value}"))
        .unwrap_or_else(|| "--".to_owned());
    let available = listing.available_date.as_deref().unwrap_or("--");
    let epc = listing
        .epc_rating
        .clone()
        .or_else(|| listing.epc_remote.clone())
        .unwrap_or_else(|| "--".to_owned());
    let epc_match = listing
        .epc_address_match
        .map(|matched| {
            if matched {
                "address match"
            } else {
                "address mismatch"
            }
        })
        .unwrap_or("--");
    let floor_area = listing
        .floor_area_sqm
        .map(|value| format!("{value:.0} sqm"))
        .unwrap_or_else(|| "--".to_owned());
    let broadband = listing
        .gigabit_availability
        .map(|value| format!("{value:.0}% gigabit"))
        .unwrap_or_else(|| "--".to_owned());

    lines.push(kv_line(
        "links",
        truncate(
            &format!(
                "rightmove: {} / maps: {}",
                listing.url, listing.google_maps_url
            ),
            max,
        ),
        theme,
    ));
    lines.push(kv_line(
        "price/deposit",
        format!("{price} / {deposit}"),
        theme,
    ));
    lines.push(kv_line(
        "home",
        format!(
            "{} bed / {} bath / {} / available {}",
            listing.bedrooms, listing.bathrooms, listing.property_type, available
        ),
        theme,
    ));
    let address = if listing.postcode.trim().is_empty() {
        listing.address.clone()
    } else {
        format!("{} / {}", listing.address, listing.postcode)
    };
    lines.push(kv_line("address", truncate(&address, max), theme));
    lines.push(kv_line(
        "evidence",
        format!("{epc} / {epc_match} / {floor_area} / {broadband}"),
        theme,
    ));
    if let Some(score) = listing.score_overall {
        let band = listing.score_band.as_deref().unwrap_or("--");
        let confidence = listing.score_confidence.as_deref().unwrap_or("--");
        lines.push(kv_line(
            "score",
            format!("{score:.0}/100 / {band} / {confidence}"),
            theme,
        ));
    }
    lines.push(kv_line(
        "cache/media",
        truncate(&format!("{cache} / {media_count} items"), max),
        theme,
    ));

    if let Some(name) = listing.agent_name.as_deref() {
        let phone = listing.agent_phone.as_deref().unwrap_or("--");
        lines.push(kv_line(
            "agent",
            truncate(&format!("{name} / {phone}"), max),
            theme,
        ));
    }

    if !compact && !listing.notes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("notes", theme.section_heading)));
        for note in listing.notes.iter().take(3) {
            lines.push(Line::from(Span::styled(
                format!("- {}", truncate(note, 140)),
                theme.body,
            )));
        }
    }
}

fn append_bundle_assessment_lines(
    lines: &mut Vec<Line<'static>>,
    bundle: &let_sdk::intelligence::EvidenceBundle,
    theme: &Theme,
    _compact: bool,
) {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "assessment",
        theme.section_heading,
    )));

    let Some(record) = bundle.assessment.as_ref() else {
        lines.push(kv_line("status", "no saved assessment", theme));
        return;
    };

    let normalized = &record.normalized_assessment;
    let recommendation = normalized
        .recommendation
        .as_deref()
        .unwrap_or("--")
        .to_owned();
    let confidence = normalized.confidence.as_deref().unwrap_or("--");

    lines.push(kv_line(
        "recommendation",
        format!("{recommendation} / {confidence}"),
        theme,
    ));
    lines.push(kv_line("saved", record.saved_at.clone(), theme));
    if let Some(summary) = normalized.summary.as_deref() {
        lines.push(kv_line("summary", summary, theme));
    }
    if let Some(family_fit) = normalized.family_fit.as_deref() {
        lines.push(kv_line("family", family_fit, theme));
    }
    if let Some(area_notes) = normalized.area_notes.as_deref() {
        lines.push(kv_line("area", area_notes, theme));
    }
    if let Some(commute_notes) = normalized.commute_notes.as_deref() {
        lines.push(kv_line("commute", commute_notes, theme));
    }
    append_wrapped_list_kv(lines, "positives", &normalized.positives, theme);
    append_wrapped_list_kv(lines, "risks", &normalized.risks, theme);
    append_wrapped_list_kv(lines, "tradeoffs", &normalized.tradeoffs, theme);
    if normalized.next_actions.is_empty() {
        append_wrapped_list_kv(lines, "next", &bundle.next_actions, theme);
    } else {
        append_wrapped_list_kv(lines, "next", &normalized.next_actions, theme);
    }
    append_wrapped_list_kv(lines, "gaps", &normalized.evidence_gaps, theme);
    append_wrapped_list_kv(lines, "warnings", &normalized.warnings, theme);
}

fn append_bundle_evidence_lines(
    lines: &mut Vec<Line<'static>>,
    bundle: &let_sdk::intelligence::EvidenceBundle,
    theme: &Theme,
    compact: bool,
) {
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("evidence", theme.section_heading)));
    let max = if compact { 120 } else { 180 };
    for section in [
        "rightmove",
        "description",
        "address",
        "facts",
        "broadband",
        "epc",
        "media",
        "verifications",
        "assessment",
    ] {
        if let Some(state) = bundle.sections.get(section) {
            lines.push(kv_line(
                section,
                truncate(
                    &format!("{} / {}", section_status_label(state.status), state.summary),
                    max,
                ),
                theme,
            ));
        }
    }

    lines.push(kv_line(
        "media",
        format!(
            "{} photos / {} local / contact sheet {}",
            bundle.media.photos.len(),
            local_media_count(bundle),
            bundle
                .media
                .contact_sheet
                .as_ref()
                .map(|sheet| sheet.status.as_str())
                .unwrap_or("--")
        ),
        theme,
    ));

    if let Some(broadband) = bundle.broadband.as_ref() {
        let gigabit = broadband
            .gigabit_availability
            .map(|value| format!("{value:.0}%"))
            .unwrap_or_else(|| "--".to_owned());
        let over_300 = broadband
            .pct_over_300mbps
            .map(|value| format!("{value:.0}% over 300mbps"))
            .unwrap_or_else(|| "--".to_owned());
        lines.push(kv_line(
            "broadband",
            format!("{gigabit} gigabit / {over_300}"),
            theme,
        ));
    }

    if let Some(epc) = bundle.epc.as_ref() {
        let rating = epc.rating.as_deref().unwrap_or("--");
        let floor_area = epc
            .floor_area_sqm
            .map(|value| format!("{value:.0} sqm"))
            .unwrap_or_else(|| "--".to_owned());
        let match_label = if epc.address_match {
            "address match"
        } else {
            "address mismatch"
        };
        lines.push(kv_line(
            "epc",
            truncate(&format!("{rating} / {floor_area} / {match_label}"), max),
            theme,
        ));
    }

    if !bundle.verifications.is_empty() {
        lines.push(kv_line("verified", verification_summary(bundle), theme));
    }
    if !bundle.flags.is_empty() {
        let flags = bundle
            .flags
            .iter()
            .take(if compact { 2 } else { 4 })
            .map(|flag| format!("{}: {}", flag.severity, flag.summary))
            .collect::<Vec<_>>();
        append_list_kv(lines, "flags", &flags, theme, compact);
    }
}

fn append_list_kv(
    lines: &mut Vec<Line<'static>>,
    key: &str,
    values: &[String],
    theme: &Theme,
    compact: bool,
) {
    if values.is_empty() {
        return;
    }
    let limit = if compact { 2 } else { 4 };
    let max = if compact { 150 } else { 220 };
    let mut text = values
        .iter()
        .take(limit)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("; ");
    if values.len() > limit {
        text.push_str("; ...");
    }
    lines.push(kv_line(key, truncate(&text, max), theme));
}

fn append_wrapped_list_kv(
    lines: &mut Vec<Line<'static>>,
    key: &str,
    values: &[String],
    theme: &Theme,
) {
    if values.is_empty() {
        return;
    }
    lines.push(kv_line(key, values.join("; "), theme));
}

fn local_media_count(bundle: &let_sdk::intelligence::EvidenceBundle) -> usize {
    bundle
        .media
        .photos
        .iter()
        .chain(bundle.media.floorplans.iter())
        .chain(bundle.media.epc_graphs.iter())
        .chain(bundle.media.maps.iter())
        .filter(|item| item.local_path.is_some())
        .count()
}

fn verification_summary(bundle: &let_sdk::intelligence::EvidenceBundle) -> String {
    let mut supported = 0usize;
    let mut contradicted = 0usize;
    let mut unknown = 0usize;
    let mut insufficient = 0usize;

    for verification in &bundle.verifications {
        match verification.status {
            let_sdk::intelligence::VerificationStatus::Supported => supported += 1,
            let_sdk::intelligence::VerificationStatus::Contradicted => contradicted += 1,
            let_sdk::intelligence::VerificationStatus::Unknown => unknown += 1,
            let_sdk::intelligence::VerificationStatus::InsufficientEvidence => insufficient += 1,
        }
    }

    format!(
        "{supported} supported / {contradicted} contradicted / {unknown} unknown / {insufficient} insufficient"
    )
}

fn section_status_label(status: let_sdk::intelligence::SectionStatus) -> &'static str {
    match status {
        let_sdk::intelligence::SectionStatus::Ok => "ok",
        let_sdk::intelligence::SectionStatus::Partial => "partial",
        let_sdk::intelligence::SectionStatus::Degraded => "degraded",
        let_sdk::intelligence::SectionStatus::Blocked => "blocked",
        let_sdk::intelligence::SectionStatus::Skipped => "skipped",
        let_sdk::intelligence::SectionStatus::Stale => "stale",
    }
}

fn kv_line(key: &str, value: impl Into<String>, theme: &Theme) -> Line<'static> {
    let value = value.into();
    Line::from(vec![
        Span::styled(format!("{key}: "), theme.key),
        Span::styled(value, theme.body),
    ])
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
