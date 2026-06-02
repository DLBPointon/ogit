//! Dashboard view — shown when ogit is opened outside a git repository.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

/// Data needed to render the dashboard.
pub struct DashboardCtx<'a> {
    /// Number of organisations the user is a member of.
    pub org_count: usize,
    /// True while background org-discovery requests are still in-flight.
    pub orgs_loading: bool,
    /// Number of repos the user collaborates on (external to their orgs).
    pub collab_repo_count: usize,
    /// Total issues the user is involved in.
    /// `None` = still loading; `Some(None)` = not supported on this platform; `Some(Some(n))` = loaded.
    pub issue_count: Option<Option<u64>>,
    /// The authenticated user's login, if known.
    pub username: Option<&'a str>,
}

use crate::cli::OGIT_LOGO;

pub fn render_dashboard(frame: &mut Frame, area: Rect, ctx: DashboardCtx<'_>) {
    let outer_block = Block::default().borders(Borders::ALL).title(" Dashboard ");
    frame.render_widget(outer_block, area);

    let inner = crate::tui::inner_rect(area);

    // Split: logo on top, stats table below, with some padding
    let logo_lines: Vec<Line> = OGIT_LOGO
        .lines()
        .map(|logo_line| {
            Line::from(Span::styled(
                logo_line.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    let logo_h = logo_lines.len() as u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(logo_h),
            Constraint::Length(1), // gap
            Constraint::Min(5),    // stats table
        ])
        .split(inner);

    // ── Logo ──────────────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new(logo_lines).alignment(Alignment::Center),
        chunks[0],
    );

    // ── Stats table ───────────────────────────────────────────────────────────
    let issue_str = match ctx.issue_count {
        None => "Loading…".to_string(),
        Some(None) => "N/A".to_string(),
        Some(Some(n)) => n.to_string(),
    };

    // Show the live count as orgs trickle in; append "…" while still loading.
    let orgs_str = if ctx.orgs_loading {
        if ctx.org_count > 0 {
            format!("{}…", ctx.org_count)
        } else {
            "Loading…".to_string()
        }
    } else {
        ctx.org_count.to_string()
    };

    let user_str = ctx.username.unwrap_or("—");

    let rows = vec![
        Row::new(vec![
            Cell::from("User").style(Style::default().fg(Color::DarkGray)),
            Cell::from(user_str.to_string()).style(Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Row::new(vec![
            Cell::from("Organisations").style(Style::default().fg(Color::DarkGray)),
            Cell::from(orgs_str).style(Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Row::new(vec![
            Cell::from("Collaborator Repos").style(Style::default().fg(Color::DarkGray)),
            Cell::from(ctx.collab_repo_count.to_string())
                .style(Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Row::new(vec![
            Cell::from("Issues Involved In").style(Style::default().fg(Color::DarkGray)),
            Cell::from(issue_str).style(Style::default().add_modifier(Modifier::BOLD)),
        ]),
    ];

    let table = Table::new(
        rows,
        // Min(22) ensures the widest label ("Issues Involved In" = 18 chars)
        // is never truncated, regardless of terminal width.
        [Constraint::Min(22), Constraint::Fill(1)],
    )
    .block(Block::default().borders(Borders::NONE))
    .column_spacing(2);

    // Centre the table horizontally by wrapping it in a padded horizontal layout
    let table_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(50),
            Constraint::Percentage(25),
        ])
        .split(chunks[2]);

    frame.render_widget(table, table_chunks[1]);
}
