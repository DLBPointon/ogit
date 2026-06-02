use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap};

use crate::filter::{FilterStage, FilterState};
use crate::model::ALL_CONFIGURED;

pub struct FilterModalCtx<'a> {
    pub filter_state: &'a FilterState,
    pub tag_picker_open: bool,
    pub tag_picker_items: &'a [String],
    pub tag_picker_selected: usize,
}

/// Fill `area` with `░` shade characters over a black background.
/// Used by every modal/overlay to darken the content behind it.
fn render_shade_overlay(frame: &mut Frame, area: Rect) {
    let overlay_width = area.width as usize;
    let overlay_height = area.height as usize;
    if overlay_width > 0 && overlay_height > 0 {
        let shade_line = std::iter::repeat('\u{2591}')
            .take(overlay_width)
            .collect::<String>();
        let overlay_lines: Vec<Line> = (0..overlay_height)
            .map(|_| {
                Line::from(Span::styled(
                    shade_line.clone(),
                    Style::default().fg(Color::Rgb(80, 80, 80)),
                ))
            })
            .collect();
        frame.render_widget(
            Paragraph::new(Text::from(overlay_lines))
                .style(Style::default().bg(Color::Black).fg(Color::Rgb(80, 80, 80))),
            area,
        );
    }
}

/// Render a vertical `│` divider at column `x`, from row `y` down for `height` rows.
fn render_vdivider(frame: &mut Frame, x: u16, y: u16, height: u16) {
    use ratatui::layout::Rect as R;
    let lines: Vec<Line> = (0..height).map(|_| Line::from("\u{2502}")).collect();
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(Style::default().fg(Color::DarkGray)),
        R {
            x,
            y,
            width: 1,
            height,
        },
    );
}

pub fn render_filter_modal(frame: &mut Frame, area: Rect, ctx: FilterModalCtx<'_>) {
    let filter_state = ctx.filter_state;

    let dropdown: Option<(&Vec<String>, usize)> =
        if filter_state.stage == FilterStage::Author && !filter_state.author_items.is_empty() {
            Some((&filter_state.author_items, filter_state.author_selected))
        } else if filter_state.stage == FilterStage::Tag && !filter_state.tag_items.is_empty() {
            Some((&filter_state.tag_items, filter_state.tag_selected))
        } else if filter_state.stage == FilterStage::Status {
            Some((&filter_state.status_items, filter_state.status_selected))
        } else if filter_state.stage == FilterStage::Kind {
            Some((&filter_state.kind_items, filter_state.kind_selected))
        } else {
            None
        };

    let repo_panel = filter_state.stage == FilterStage::Repo && !filter_state.repo_items.is_empty();

    let has_dropdown = dropdown.is_some() || repo_panel;
    let fields_col_w: u16 = 36;
    let side_col_w: u16 = if has_dropdown { 28 } else { 0 };
    let sep_col_w: u16 = if has_dropdown { 1 } else { 0 };
    let desired_modal_w = fields_col_w + sep_col_w + side_col_w + 2;
    let modal_w = desired_modal_w.min(area.width.saturating_sub(4));
    let modal_h = if filter_state.repo_items.is_empty() {
        11u16
    } else {
        12u16
    };
    let modal_x = area.x + (area.width.saturating_sub(modal_w)) / 2;
    let modal_y = area.y + (area.height.saturating_sub(modal_h)) / 2;
    let modal_rect = Rect {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
    };

    // Shade the background
    render_shade_overlay(frame, area);

    frame.render_widget(Clear, modal_rect);
    let modal_block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " Filter ",
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(Color::Black));
    frame.render_widget(modal_block, modal_rect);

    let inner = crate::tui::inner_rect(modal_rect);

    let actual_fields_w = if has_dropdown && inner.width > side_col_w + sep_col_w + 4 {
        inner.width.saturating_sub(side_col_w + sep_col_w)
    } else {
        inner.width
    };
    let fields_area = Rect {
        x: inner.x,
        y: inner.y,
        width: actual_fields_w,
        height: inner.height,
    };

    let mut lines: Vec<Line> = Vec::new();
    let hint = if !filter_state.repo_items.is_empty() {
        "Tab: next  Enter: apply  Esc: cancel  Space: toggle repo"
    } else {
        "Tab: next field  Enter: apply  Esc: cancel"
    };
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::raw("")));

    // Author field
    {
        let prefix = if filter_state.stage == FilterStage::Author {
            "▶ "
        } else {
            "  "
        };
        let text = format!("{}Author:  {}", prefix, filter_state.author);
        if filter_state.stage == FilterStage::Author {
            lines.push(Line::from(Span::styled(
                text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::raw(text)));
        }
    }
    // Tag field
    {
        let prefix = if filter_state.stage == FilterStage::Tag {
            "▶ "
        } else {
            "  "
        };
        let text = format!("{}Tag:     {}", prefix, filter_state.tag);
        if filter_state.stage == FilterStage::Tag {
            lines.push(Line::from(Span::styled(
                text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::raw(text)));
        }
    }
    // Status field
    {
        let prefix = if filter_state.stage == FilterStage::Status {
            "▶ "
        } else {
            "  "
        };
        let status_display = if filter_state.status.trim().is_empty() {
            "(all)"
        } else {
            filter_state.status.as_str()
        };
        let text = format!("{}Status:  {}", prefix, status_display);
        if filter_state.stage == FilterStage::Status {
            lines.push(Line::from(Span::styled(
                text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::raw(text)));
        }
    }
    // Kind field
    {
        let prefix = if filter_state.stage == FilterStage::Kind {
            "▶ "
        } else {
            "  "
        };
        let kind_display = if filter_state.kind.trim().is_empty() {
            "(all)"
        } else {
            filter_state.kind.as_str()
        };
        let text = format!("{}Kind:    {}", prefix, kind_display);
        if filter_state.stage == FilterStage::Kind {
            lines.push(Line::from(Span::styled(
                text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::raw(text)));
        }
    }
    // Search field
    {
        let prefix = if filter_state.stage == FilterStage::Search {
            "▶ "
        } else {
            "  "
        };
        let text = format!("{}Search:  {}", prefix, filter_state.query);
        if filter_state.stage == FilterStage::Search {
            lines.push(Line::from(Span::styled(
                text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::raw(text)));
        }
    }
    // Repo field (multi-select; only shown in ALL view)
    if !filter_state.repo_items.is_empty() {
        let prefix = if filter_state.stage == FilterStage::Repo {
            "▶ "
        } else {
            "  "
        };
        let selected_count = filter_state.repo_toggles.iter().filter(|&&t| t).count();
        let summary = if selected_count == 0 {
            "(all)".to_string()
        } else {
            format!("{} selected", selected_count)
        };
        let text = format!("{}Repo:    {}", prefix, summary);
        if filter_state.stage == FilterStage::Repo {
            lines.push(Line::from(Span::styled(
                text,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::raw(text)));
        }
    }

    let fields_p = Paragraph::new(Text::from(lines));
    frame.render_widget(fields_p, fields_area);

    // Right column: author/tag dropdown
    if let Some((items, selected)) = dropdown {
        if inner.width > actual_fields_w + sep_col_w {
            let sep_x = inner.x + actual_fields_w;
            render_vdivider(frame, sep_x, inner.y, inner.height);

            let dd_area = Rect {
                x: sep_x + 1,
                y: inner.y,
                width: inner.width.saturating_sub(actual_fields_w + sep_col_w),
                height: inner.height,
            };

            let total = items.len();
            let max_visible = dd_area.height as usize;
            let mut start = if selected >= max_visible / 2 {
                selected - max_visible / 2
            } else {
                0
            };
            if start + max_visible > total {
                start = total.saturating_sub(max_visible);
            }

            let mut dlines: Vec<Line> = Vec::new();
            for item_idx in 0..max_visible {
                let idx = start + item_idx;
                if idx >= total {
                    break;
                }
                let item = &items[idx];
                let label = if item.trim().is_empty() {
                    "(all)".to_string()
                } else {
                    item.clone()
                };
                if idx == selected {
                    dlines.push(Line::from(Span::styled(
                        label,
                        Style::default().fg(Color::Black).bg(Color::Yellow),
                    )));
                } else {
                    dlines.push(Line::from(Span::raw(label)));
                }
            }

            let dropdown_para = Paragraph::new(Text::from(dlines));
            frame.render_widget(dropdown_para, dd_area);
        }
    }

    // Right column: repo multi-select panel (stage 4)
    if repo_panel && inner.width > actual_fields_w + sep_col_w {
        let sep_x = inner.x + actual_fields_w;
        render_vdivider(frame, sep_x, inner.y, inner.height);

        let dd_area = Rect {
            x: sep_x + 1,
            y: inner.y,
            width: inner.width.saturating_sub(actual_fields_w + sep_col_w),
            height: inner.height,
        };

        let total = filter_state.repo_items.len();
        let max_visible = dd_area.height as usize;
        let cursor = filter_state.repo_cursor;
        let mut start = if cursor >= max_visible / 2 {
            cursor - max_visible / 2
        } else {
            0
        };
        if start + max_visible > total {
            start = total.saturating_sub(max_visible);
        }

        let mut dlines: Vec<Line> = Vec::new();
        for item_idx in 0..max_visible {
            let idx = start + item_idx;
            if idx >= total {
                break;
            }
            let name = &filter_state.repo_items[idx];
            let toggled = filter_state.repo_toggles.get(idx).copied().unwrap_or(false);
            let check = if toggled { "[✓] " } else { "[ ] " };
            let label = format!("{}{}", check, name);
            if idx == cursor {
                dlines.push(Line::from(Span::styled(
                    label,
                    Style::default().fg(Color::Black).bg(Color::Yellow),
                )));
            } else if toggled {
                dlines.push(Line::from(Span::styled(
                    label,
                    Style::default().fg(Color::Green),
                )));
            } else {
                dlines.push(Line::from(Span::raw(label)));
            }
        }

        let dropdown_para = Paragraph::new(Text::from(dlines));
        frame.render_widget(dropdown_para, dd_area);
    }

    // Tag picker overlay (rendered on top of the filter modal)
    if ctx.tag_picker_open {
        let tag_picker_w = std::cmp::min(40u16, area.width.saturating_sub(8));
        let tag_picker_h = std::cmp::min(10u16, (ctx.tag_picker_items.len() as u16) + 4);
        let tag_picker_x = area.x + (area.width.saturating_sub(tag_picker_w)) / 2;
        let tag_picker_y = area.y + (area.height.saturating_sub(tag_picker_h)) / 2;
        let tag_picker_rect = Rect {
            x: tag_picker_x,
            y: tag_picker_y,
            width: tag_picker_w,
            height: tag_picker_h,
        };

        render_shade_overlay(frame, area);

        frame.render_widget(Clear, tag_picker_rect);
        let tag_picker_block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                " Tags ",
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Black));

        let mut tag_picker_lines: Vec<Line> = Vec::new();
        tag_picker_lines.push(Line::from(Span::raw(
            "Use ↑/↓ to pick, Enter to apply, Esc to cancel",
        )));
        tag_picker_lines.push(Line::from(Span::raw(" ")));
        for (item_idx, item) in ctx.tag_picker_items.iter().enumerate() {
            if item_idx == ctx.tag_picker_selected {
                tag_picker_lines.push(Line::from(Span::styled(
                    item.clone(),
                    Style::default().fg(Color::Black).bg(Color::Yellow),
                )));
            } else {
                tag_picker_lines.push(Line::from(Span::raw(item.clone())));
            }
        }

        let text_para = Paragraph::new(Text::from(tag_picker_lines))
            .block(tag_picker_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(text_para, tag_picker_rect);
    }
}

/// Internal core used by [`render_error_modal`] and [`render_success_modal`].
fn render_notification_modal(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    title_colour: Color,
    message: &str,
    copy_hint: bool,
) {
    let modal_w: u16 = 60;
    let inner_w = (modal_w - 2) as usize;
    let mut body_lines: Vec<String> = Vec::new();
    for raw_line in message.lines() {
        if raw_line.is_empty() {
            body_lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            // Flush the current segment if adding this word would overflow.
            if !current.is_empty() && current.len() + 1 + word.len() > inner_w {
                body_lines.push(std::mem::take(&mut current));
            }
            // Hard-break any word that is itself wider than the inner width
            // (e.g. long URLs that contain no spaces).
            let mut remaining = word;
            while remaining.chars().count() > inner_w {
                let split = remaining
                    .char_indices()
                    .nth(inner_w)
                    .map(|(i, _)| i)
                    .unwrap_or(remaining.len());
                if !current.is_empty() {
                    body_lines.push(std::mem::take(&mut current));
                }
                body_lines.push(remaining[..split].to_string());
                remaining = &remaining[split..];
            }
            // Append the (possibly shortened) remainder to the current segment.
            if current.is_empty() {
                current.push_str(remaining);
            } else {
                current.push(' ');
                current.push_str(remaining);
            }
        }
        if !current.is_empty() {
            body_lines.push(current);
        }
    }

    let modal_h = (2 + 1 + body_lines.len() as u16 + 1 + 1).max(7);
    let modal_x = area.x + (area.width.saturating_sub(modal_w)) / 2;
    let modal_y = area.y + (area.height.saturating_sub(modal_h)) / 2;
    let modal_rect = Rect {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
    };

    render_shade_overlay(frame, area);

    frame.render_widget(Clear, modal_rect);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                format!(" {} ", title),
                Style::default()
                    .fg(title_colour)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Black)),
        modal_rect,
    );

    let inner = crate::tui::inner_rect(modal_rect);

    let mut lines: Vec<Line> = vec![Line::from(Span::raw(""))];
    for body_line in &body_lines {
        lines.push(Line::from(Span::styled(
            body_line.clone(),
            Style::default().fg(Color::White),
        )));
    }
    lines.push(Line::from(Span::raw("")));

    if copy_hint {
        lines.push(Line::from(vec![
            Span::styled(
                "c",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  copy  ·  any other key to dismiss",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "Press any key to dismiss",
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

/// Render an error notification modal (red title).
/// `c` copies the message to the clipboard; any other key dismisses.
pub fn render_error_modal(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    render_notification_modal(frame, area, title, Color::Rgb(220, 80, 80), message, true);
}

/// Render a success notification modal (green title). Any key dismisses it.
pub fn render_success_modal(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    render_notification_modal(frame, area, title, Color::Rgb(80, 200, 80), message, false);
}

pub fn render_help_modal(frame: &mut Frame, area: Rect) {
    // Keybind data for both columns
    let left_rows: &[(&str, &str)] = &[
        ("↑ / ↓", "Navigate list"),
        ("Enter", "Fetch issues / expand org"),
        ("Tab", "Switch panel focus"),
        ("p", "Pin / unpin repo"),
        ("c", "Clone repo to local disk"),
    ];
    let right_rows: &[(&str, &str)] = &[
        ("r", "Refresh issues"),
        ("f", "Open filter"),
        ("F", "Clear all filters"),
        ("Enter", "Open issue details"),
        ("o", "Open issue in browser"),
        ("Esc", "Back to list"),
        ("q", "Quit"),
    ];

    // Each section: header + separator + N rows
    let left_h = 2 + left_rows.len() as u16;
    let right_h = 2 + right_rows.len() as u16;
    let col_h = left_h.max(right_h);

    // Minimum inner width for two columns: col(26) + gap(2) + col(26) = 54
    // Preferred width for two full columns: col(41) + gap(3) + col(41) = 85
    let preferred_w: u16 = 87; // 85 inner + 2 border
    let two_col_min_w: u16 = 56; // 54 inner + 2 border

    // Clamp to terminal, always leaving at least a 1-col margin each side.
    let modal_w = preferred_w.min(area.width.saturating_sub(2)).max(30);
    let two_col = modal_w >= two_col_min_w;

    // inner height: subtitle + blank + columns + blank + footer
    let content_h = if two_col { col_h } else { left_h + 1 + right_h };
    let inner_h: u16 = 1 + 1 + content_h + 1 + 1;
    let modal_h = (inner_h + 2).min(area.height.saturating_sub(2)).max(7);

    let modal_x = area.x + (area.width.saturating_sub(modal_w)) / 2;
    let modal_y = area.y + (area.height.saturating_sub(modal_h)) / 2;
    let modal_rect = Rect {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
    };

    render_shade_overlay(frame, area);
    frame.render_widget(Clear, modal_rect);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                " ogit-rs ",
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Black)),
        modal_rect,
    );

    let inner = crate::tui::inner_rect(modal_rect);

    // Subtitle
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "TUI for git issue management",
            Style::default().fg(Color::DarkGray),
        ))),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    // Helper: build lines for one section (header + separator + keybinds)
    let build_section = |title: &str, rows: &[(&str, &str)], key_w: usize| -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(Span::styled(
            title.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "─".repeat(key_w + 2 + 20),
            Style::default().fg(Color::DarkGray),
        )));
        for (key, desc) in rows {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<width$}", key, width = key_w),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(desc.to_string()),
            ]));
        }
        lines
    };

    let col_y = inner.y + 2; // below subtitle + blank line

    if two_col {
        // Distribute inner width: left col | gap(3) | right col
        let gap: u16 = 3;
        let col_w = (inner.width.saturating_sub(gap)) / 2;
        let key_w = 13usize;

        let left_lines = build_section("Navigation & Repos", left_rows, key_w);
        let right_lines = build_section("Issues & General", right_rows, key_w);

        let left_rect = Rect {
            x: inner.x,
            y: col_y,
            width: col_w,
            height: col_h,
        };
        let right_rect = Rect {
            x: inner.x + col_w + gap,
            y: col_y,
            width: col_w,
            height: col_h,
        };

        // Guard: only render if rects are within the modal inner area
        if left_rect.right() <= modal_rect.right() {
            frame.render_widget(Paragraph::new(Text::from(left_lines)), left_rect);
        }
        if right_rect.right() <= modal_rect.right() {
            frame.render_widget(Paragraph::new(Text::from(right_lines)), right_rect);
        }
    } else {
        // Single-column: list all rows with a blank line between sections
        let key_w = 10usize;
        let mut all_lines = build_section("Navigation & Repos", left_rows, key_w);
        all_lines.push(Line::from(Span::raw("")));
        all_lines.extend(build_section("Issues & General", right_rows, key_w));
        frame.render_widget(
            Paragraph::new(Text::from(all_lines)),
            Rect {
                x: inner.x,
                y: col_y,
                width: inner.width,
                height: content_h,
            },
        );
    }

    // Footer
    let footer_y = col_y + content_h + 1;
    let footer_max_y = modal_rect.y + modal_rect.height.saturating_sub(1);
    if footer_y < footer_max_y {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Press any key to close",
                Style::default().fg(Color::DarkGray),
            ))),
            Rect {
                x: inner.x,
                y: footer_y,
                width: inner.width,
                height: 1,
            },
        );
    }
}

/// Render a loading overlay over `area` while issues are being fetched.
///
/// Shades the area with `░` characters and shows a centred box with a
/// braille spinner (frame derived from system time — no extra app state
/// needed) and the name of the repo being loaded.
pub fn render_loading_overlay(
    frame: &mut Frame,
    area: Rect,
    repo_label: &str,
    progress: Option<(usize, usize, &str)>,
) {
    use std::time::{SystemTime, UNIX_EPOCH};

    // ── Background shade ──────────────────────────────────────────────────
    render_shade_overlay(frame, area);

    // ── Spinner frame from wall-clock time (cycles every 100 ms) ─────────
    const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_millis() / 100) as usize)
        .unwrap_or(0);
    let spinner = SPINNER[tick % SPINNER.len()];

    // ── Modal box ─────────────────────────────────────────────────────────
    let is_all_configured = repo_label.is_empty() || repo_label == ALL_CONFIGURED;
    let msg = if is_all_configured {
        format!("{}  Fetching all issues…", spinner)
    } else {
        format!("{}  Fetching issues for {}…", spinner, repo_label)
    };

    // "All (configured)" gets an extra subtitle row explaining scope.
    let subtitle: Option<&str> = if is_all_configured {
        Some("Configured repos only")
    } else {
        None
    };

    let min_w: u16 = if progress.is_some() { 42 } else { 36 };
    // Minimum width must also fit the subtitle when present.
    let min_w = subtitle
        .map(|subtitle_text| min_w.max(subtitle_text.len() as u16 + 4))
        .unwrap_or(min_w);
    let modal_w = (msg.chars().count() as u16 + 6)
        .max(min_w)
        .min(area.width.saturating_sub(4));
    // +1 height when subtitle is shown.
    let extra_h: u16 = if subtitle.is_some() { 1 } else { 0 };
    let modal_h: u16 = if progress.is_some() { 7 } else { 5 } + extra_h;
    let modal_x = area.x + (area.width.saturating_sub(modal_w)) / 2;
    let modal_y = area.y + (area.height.saturating_sub(modal_h)) / 2;
    let modal_rect = Rect {
        x: modal_x,
        y: modal_y,
        width: modal_w,
        height: modal_h,
    };

    frame.render_widget(Clear, modal_rect);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                " Loading ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Black)),
        modal_rect,
    );

    let inner = crate::tui::inner_rect(modal_rect);

    use ratatui::layout::Alignment;
    let mut lines = vec![
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            msg,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    if let Some(sub) = subtitle {
        lines.push(Line::from(Span::styled(
            sub,
            Style::default().fg(Color::DarkGray),
        )));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).alignment(Alignment::Center),
        inner,
    );

    // ── Progress indicator (shown for both single-repo and multi-repo fetches) ───
    if let Some((fetched, total, label)) = progress {
        // Sits below blank + message + optional subtitle + blank.
        let progress_rect = Rect {
            x: inner.x,
            y: inner.y + 3 + extra_h,
            width: inner.width,
            height: 1,
        };
        use ratatui::layout::Alignment as Align;
        if total > 0 {
            // Determinate: percentage gauge (multi-repo).
            let ratio = (fetched as f64 / total as f64).clamp(0.0, 1.0);
            let gauge_label = format!("{} / {}  ↳ {}", fetched, total, label);
            frame.render_widget(
                Gauge::default()
                    .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                    .label(Span::styled(
                        gauge_label,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .ratio(ratio),
                progress_rect,
            );
        } else {
            // Indeterminate: issue + page counter (single-repo, total pages unknown).
            let count_label = if fetched > 0 {
                format!("↳  {}  issues  ·  {}", fetched, label)
            } else {
                format!("↳  {}", label)
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    count_label,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(Align::Center),
                progress_rect,
            );
        }
    }
}
