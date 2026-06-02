use std::collections::HashMap;

use csscolorparser::parse;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::model::{ALL_CONFIGURED, Issue, IssueList, RepoSpec};

/// ITU-R BT.601 luma coefficients × 1000, used to compute perceived brightness.
const LUMA_R: u32 = 299;
const LUMA_G: u32 = 587;
const LUMA_B: u32 = 114;
const LUMA_SCALE: u32 = 1000;
/// W3C WCAG mid-tone contrast threshold — above this brightness, use black text.
const WCAG_THRESHOLD: u32 = 186;

pub struct IssueListCtx<'a> {
    pub loading: bool,
    pub error: Option<&'a str>,
    pub issues: Option<&'a IssueList>,
    pub filtered: &'a [&'a Issue],
    pub is_all_view: bool,
    pub repo_specs: &'a [RepoSpec],
    pub selected: usize,
    pub filters_summary: Vec<String>,
}

pub fn render_issue_list(frame: &mut Frame, area: Rect, ctx: IssueListCtx<'_>) {
    let right_block = Block::default().borders(Borders::ALL).title("Issues");

    if ctx.loading {
        frame.render_widget(Paragraph::new("Loading...").block(right_block), area);
        return;
    }
    if let Some(err) = ctx.error {
        frame.render_widget(
            Paragraph::new(err.to_owned())
                .block(right_block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    if ctx.issues.is_none() {
        frame.render_widget(
            Paragraph::new(
                "No issues loaded. Select a repo and press Enter or press 'r' to refresh.",
            )
            .block(right_block),
            area,
        );
        return;
    }

    frame.render_widget(right_block, area);
    let inner = crate::tui::inner_rect(area);

    let table_area = if !ctx.filters_summary.is_empty() && inner.height > 1 {
        let vchunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)])
            .split(inner);
        frame.render_widget(
            Paragraph::new(format!(
                "Active Filter:  {}",
                ctx.filters_summary.join("  |  ")
            ))
            .style(Style::default().fg(Color::Magenta)),
            vchunks[0],
        );
        vchunks[1]
    } else {
        inner
    };

    let repo_colour_map: HashMap<&str, Color> = if ctx.is_all_view {
        ctx.repo_specs
            .iter()
            .filter(|repo_spec| {
                repo_spec.name.as_str() != ALL_CONFIGURED && !repo_spec.colour.trim().is_empty()
            })
            .filter_map(|repo_spec| {
                crate::tui::parse_css_color_to_rgb(&repo_spec.colour)
                    .map(|parsed_color| (repo_spec.name.as_str(), parsed_color))
            })
            .collect()
    } else {
        HashMap::new()
    };

    let mut rows: Vec<TableRow> = Vec::new();
    for issue in ctx.filtered.iter() {
        let mut tag_spans: Vec<Span> = Vec::new();
        for label in &issue.labels.0 {
            let name = label.name.trim().to_string();
            if name.is_empty() {
                continue;
            }
            let mut style = Style::default().add_modifier(Modifier::BOLD);
            if !label.colour.trim().is_empty() {
                let cstr = label.colour.trim();
                let parsed = parse(cstr).or_else(|_| parse(&format!("#{}", cstr)));
                if let Ok(col) = parsed {
                    let [red, green, blue, _alpha] = col.to_rgba8();
                    let brightness =
                        (red as u32 * LUMA_R + green as u32 * LUMA_G + blue as u32 * LUMA_B)
                            / LUMA_SCALE;
                    let fg_color = if brightness > WCAG_THRESHOLD {
                        Color::Black
                    } else {
                        Color::White
                    };
                    style = Style::default()
                        .fg(fg_color)
                        .bg(Color::Rgb(red, green, blue))
                        .add_modifier(Modifier::BOLD);
                }
            }
            tag_spans.push(Span::styled(format!(" {} ", name), style));
            tag_spans.push(Span::raw(" "));
        }
        let tags_cell = if tag_spans.is_empty() {
            Cell::from(Span::raw(""))
        } else {
            Cell::from(Text::from(Line::from(tag_spans)))
        };

        let mut row_cells = vec![Cell::from(issue.number.to_string())];
        if ctx.is_all_view {
            let colour_cell = match repo_colour_map.get(issue.repo_name.as_str()).copied() {
                Some(repo_color) => Cell::from(Span::styled("  ", Style::default().bg(repo_color))),
                None => Cell::from(Span::raw("  ")),
            };
            row_cells.push(colour_cell);
        }
        let is_pr = issue.pull_request.is_some();
        let title = if is_pr {
            format!("⎇ {}", issue.title)
        } else {
            issue.title.clone()
        };
        row_cells.push(Cell::from(title));
        row_cells.push(Cell::from(issue.state.clone()));
        row_cells.push(Cell::from(issue.user.login.clone()));
        row_cells.push(tags_cell);
        rows.push(TableRow::new(row_cells));
    }

    let mut header_cells = vec![Cell::from(Span::styled(
        "NO.",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if ctx.is_all_view {
        header_cells.push(Cell::from(Span::raw("  ")));
    }
    header_cells.extend([
        Cell::from(Span::styled(
            "TITLE",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "STATE",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "AUTHOR",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "TAGS",
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ]);
    let header = TableRow::new(header_cells);

    let constraints: Vec<Constraint> = if ctx.is_all_view {
        vec![
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Percentage(50),
            Constraint::Length(10),
            Constraint::Length(15),
            Constraint::Percentage(25),
        ]
    } else {
        vec![
            Constraint::Length(6),
            Constraint::Percentage(55),
            Constraint::Length(10),
            Constraint::Length(15),
            Constraint::Percentage(30),
        ]
    };

    let table = Table::new(rows, constraints)
        .header(header)
        .block(Block::default())
        .column_spacing(1)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("» ");

    let effective_selected = if ctx.filtered.is_empty() {
        0
    } else {
        ctx.selected.min(ctx.filtered.len() - 1)
    };
    let mut state = TableState::default();
    state.select(Some(effective_selected));
    frame.render_stateful_widget(table, table_area, &mut state);
}
