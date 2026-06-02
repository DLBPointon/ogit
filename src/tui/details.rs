//! Issue-details page.
//!
//! Layout:
//!   ┌─ Issue Details ──────────────────────────────────┐
//!   │  [header table: #, title, author, repo, …]       │
//!   │  <issue body — Markdown>                         │
//!   │  ── Discussion (N) ──────────────────────────────│
//!   │  ┌─ @alice · 2024-01-10 ──────────────────────┐  │
//!   │  │  comment body (wrapped by ratatui)         │  │
//!   │  └────────────────────────────────────────────┘  │
//!   └──────────────────────────────────────────────────┘
//!
//! Each comment is a real `Paragraph` widget inside a coloured `Block`, so
//! ratatui's own word-wrap handles the content correctly — no manual │ injection.
//!
//! Scrolling is managed with a virtual-layout loop: we compute each section's
//! logical height, then position every visible section into the correct `Rect`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell as TCell, Paragraph, Row, Table, Wrap};

use crate::model::{Comment, Issue};

// ── Colour palette ────────────────────────────────────────────────────────────

const BUBBLE_PALETTE: &[Color] = &[
    Color::Cyan,
    Color::Green,
    Color::Yellow,
    Color::Magenta,
    Color::Blue,
    Color::Red,
    Color::LightCyan,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightMagenta,
    Color::LightBlue,
];

fn bubble_color(username: &str) -> Color {
    let mut hash_val: u64 = 5381;
    for byte in username.bytes() {
        hash_val = hash_val.wrapping_mul(33).wrapping_add(byte as u64);
    }
    BUBBLE_PALETTE[(hash_val as usize) % BUBBLE_PALETTE.len()]
}

fn date_part(timestamp_str: &str) -> &str {
    if timestamp_str.len() >= 10 {
        &timestamp_str[..10]
    } else {
        timestamp_str
    }
}

// ── Context ───────────────────────────────────────────────────────────────────

pub struct IssueDetailsCtx<'a> {
    pub filtered: &'a [&'a Issue],
    pub selected: usize,
    pub details_comments: Option<&'a Vec<Comment>>,
    pub details_loading_comments: bool,
    pub details_error: Option<&'a str>,
    /// Current vertical scroll offset (virtual lines from the top).
    pub details_scroll: usize,
    /// Whether the API has further comment pages to load.
    pub details_has_more_pages: bool,
}

// ── Virtual layout ────────────────────────────────────────────────────────────

/// One logical section of the scrollable body.
enum Section {
    /// Plain text lines — rendered as a bare `Paragraph` (no border).
    Text(Vec<Line<'static>>),
    /// A comment — rendered as a `Paragraph` inside a coloured `Block`.
    Comment {
        title: String,
        color: Color,
        body: Vec<Line<'static>>,
    },
    /// Blank vertical gap of `n` rows.
    Spacer(usize),
}

impl Section {
    /// Logical row count this section occupies in the virtual scroll space.
    ///
    /// For `Comment` the two border rows (+2) are included.
    fn height(&self) -> usize {
        match self {
            Section::Text(lines) => lines.len(),
            Section::Comment { body, .. } => body.len().max(1) + 2,
            Section::Spacer(n) => *n,
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Render the issue details panel.
/// Returns `(content_lines, visible_lines)` — the total virtual scroll height and
/// the visible body height — so the caller can update its scroll-clamping state.
pub fn render_issue_details(
    frame: &mut Frame,
    area: Rect,
    ctx: IssueDetailsCtx<'_>,
) -> (usize, usize) {
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(" Issue Details ");

    let Some(issue) = ctx.filtered.get(ctx.selected) else {
        frame.render_widget(
            Paragraph::new("No issue selected.").block(outer_block),
            area,
        );
        return (0, 0);
    };

    frame.render_widget(outer_block, area);

    let inner = crate::tui::inner_rect(area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(inner);

    render_header(frame, chunks[0], issue);
    render_body(frame, chunks[1], issue, &ctx)
}

// ── Header table ──────────────────────────────────────────────────────────────

fn render_header(frame: &mut Frame, area: Rect, issue: &Issue) {
    let repo = if issue.repo_name.is_empty() {
        "—".to_string()
    } else {
        issue.repo_name.clone()
    };

    let row_data = Row::new(vec![
        TCell::from(format!("#{}", issue.number)),
        TCell::from(issue.title.clone()),
        TCell::from(issue.user.login.clone()),
        TCell::from(repo),
        TCell::from(format!("💬 {}", issue.comments)),
        TCell::from(date_part(&issue.created_at).to_string()),
        TCell::from(format!("↺ {}", date_part(&issue.updated_at))),
    ])
    .style(
        Style::default()
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::REVERSED),
    );

    let widths = [
        Constraint::Length(7),
        Constraint::Percentage(38),
        Constraint::Length(14),
        Constraint::Length(18),
        Constraint::Length(7),
        Constraint::Length(12),
        Constraint::Length(14),
    ];

    frame.render_widget(Table::new(vec![row_data], widths).column_spacing(1), area);
}

// ── Scrollable body ───────────────────────────────────────────────────────────

fn render_body(
    frame: &mut Frame,
    area: Rect,
    issue: &Issue,
    ctx: &IssueDetailsCtx<'_>,
) -> (usize, usize) {
    let area_width = area.width as usize;
    let visible_height = area.height as usize;

    // ── Assemble virtual sections ─────────────────────────────────────────
    let mut sections: Vec<Section> = Vec::new();

    // Issue body (Markdown)
    sections.push(Section::Text(crate::tui::markdown::markdown_to_lines(
        &issue.body,
        area_width,
    )));
    sections.push(Section::Spacer(1));

    // Discussion separator
    let disc_label = match ctx.details_comments {
        Some(c) => format!("── Discussion ({}) ", c.len()),
        None => "── Discussion ".to_string(),
    };
    let fill = area_width.saturating_sub(disc_label.chars().count());
    sections.push(Section::Text(vec![Line::from(Span::styled(
        format!("{}{}", disc_label, "─".repeat(fill)),
        Style::default().add_modifier(Modifier::BOLD),
    ))]));
    sections.push(Section::Spacer(1));

    // Comments / states
    if let Some(err) = ctx.details_error {
        sections.push(Section::Text(vec![Line::from(Span::styled(
            format!("⚠  {}", err),
            Style::default().fg(Color::Red),
        ))]));
    } else if ctx.details_loading_comments && ctx.details_comments.is_none() {
        sections.push(Section::Text(vec![Line::from(Span::styled(
            "  Loading discussion…".to_string(),
            Style::default().fg(Color::DarkGray),
        ))]));
    } else if let Some(comments) = ctx.details_comments {
        if comments.is_empty() && issue.comments == 0 {
            sections.push(Section::Text(vec![Line::from(Span::styled(
                "  No comments.".to_string(),
                Style::default().fg(Color::DarkGray),
            ))]));
        } else {
            for comment in comments.iter() {
                sections.push(Section::Comment {
                    title: format!(
                        " @{} · {} ",
                        comment.user.login,
                        date_part(&comment.created_at)
                    ),
                    color: bubble_color(&comment.user.login),
                    body: crate::tui::markdown::markdown_to_lines(
                        &comment.body,
                        area_width.saturating_sub(2),
                    ),
                });
                sections.push(Section::Spacer(1));
            }
            if ctx.details_loading_comments {
                sections.push(Section::Text(vec![Line::from(Span::styled(
                    "  Loading more comments…".to_string(),
                    Style::default().fg(Color::DarkGray),
                ))]));
            } else if ctx.details_has_more_pages {
                sections.push(Section::Text(vec![Line::from(Span::styled(
                    "  ↓  more comments — scroll to reveal".to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ))]));
            }
        }
    } else if issue.comments == 0 {
        sections.push(Section::Text(vec![Line::from(Span::styled(
            "  No comments.".to_string(),
            Style::default().fg(Color::DarkGray),
        ))]));
    }

    // ── Virtual total height ──────────────────────────────────────────────
    let total_height: usize = sections.iter().map(|section| section.height()).sum();

    // Clamp scroll so we never push the last content above the viewport top.
    let scroll = ctx
        .details_scroll
        .min(total_height.saturating_sub(visible_height.max(1)));

    // ── Render each section at its viewport position ──────────────────────
    let mut virtual_y: i32 = 0;

    for section in sections {
        let sec_h = section.height() as i32;
        // y position of this section inside the viewport (may be negative if
        // the section is partially scrolled above the top).
        let vp_y = virtual_y - scroll as i32;

        // Entirely below the viewport — stop.
        if vp_y >= visible_height as i32 {
            break;
        }

        // Entirely above the viewport — skip.
        if vp_y + sec_h <= 0 {
            virtual_y += sec_h;
            continue;
        }

        // How many rows of this section are above the viewport top.
        let clip_top = (-vp_y).max(0) as usize;
        // First visible row inside the viewport.
        let y_start = vp_y.max(0) as u16;
        // Visible height (clamped to viewport bottom and section bottom).
        let avail =
            ((sec_h - clip_top as i32).min(visible_height as i32 - y_start as i32)).max(0) as u16;

        if avail > 0 {
            let rect = Rect {
                x: area.x,
                y: area.y + y_start,
                width: area.width,
                height: avail,
            };

            match section {
                // ── Plain text ─────────────────────────────────────────
                Section::Text(lines) => {
                    let visible: Vec<Line<'static>> = lines.into_iter().skip(clip_top).collect();
                    frame.render_widget(Paragraph::new(Text::from(visible)), rect);
                }

                // ── Spacer — nothing to draw ───────────────────────────
                Section::Spacer(_) => {}

                // ── Comment block ──────────────────────────────────────
                Section::Comment { title, color, body } => {
                    let border_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
                    let title_span = Span::styled(title, border_style);

                    if clip_top == 0 {
                        // Full comment visible from the top border down.
                        let block = Block::default()
                            .borders(Borders::ALL)
                            .title(title_span)
                            .border_style(border_style);
                        frame.render_widget(
                            Paragraph::new(Text::from(body))
                                .block(block)
                                .wrap(Wrap { trim: false }),
                            rect,
                        );
                    } else {
                        // Top border (and possibly some body lines) scrolled off.
                        // clip_top == 1  → only the top border is gone
                        // clip_top == 2  → top border + 1 body line gone, etc.
                        let body_skip = clip_top.saturating_sub(1);
                        let visible_body: Vec<Line<'static>> =
                            body.into_iter().skip(body_skip).collect();
                        // Render without a top border so the content flows
                        // naturally from the viewport edge.
                        let block = Block::default()
                            .borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
                            .border_style(border_style);
                        frame.render_widget(
                            Paragraph::new(Text::from(visible_body))
                                .block(block)
                                .wrap(Wrap { trim: false }),
                            rect,
                        );
                    }
                }
            }
        }

        virtual_y += sec_h;
    }

    (total_height, visible_height)
}
