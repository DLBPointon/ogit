//! Left-hand repository panel.
//!
//! Repos are grouped under organisation sub-headers.  A **Pinned** section
//! appears at the very top whenever at least one repo has been marked with `p`.
//! Org headers are selectable (Enter expands/collapses them).

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::model::{ALL_CONFIGURED, RepoDbEntry, RepoSpec};

// ── Panel mode ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    /// Repos grouped under organisation sub-headers.
    ByOrg,
}

// ── Panel cursor ──────────────────────────────────────────────────────────────

/// What is currently selected in the repository panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelCursor {
    /// A configured repo — index into `repo_specs` (0 = ALL sentinel).
    Repo(usize),
    /// A selectable org header row.
    OrgHeader { org: String, source: String },
    /// Fetch all issues for every repo (configured + discovered) in one org.
    OrgAll { org: String, source: String },
    /// A repo discovered via the org API that is not yet in `config.json`.
    DiscoveredRepo { address: String, source: String },
    /// A repo the user collaborates on (external org).
    LinkedRepo { address: String, source: String },
    /// A collapsible source-group header (e.g. "GitHub", "Gitea").
    SourceHeader { source: String },
    /// The collapsable "Linked" section header.
    LinkedHeader { source: String },
    /// The top-level Dashboard entry.
    Dashboard,
}

impl Default for PanelCursor {
    fn default() -> Self {
        PanelCursor::Repo(0)
    }
}

// ── Panel items ───────────────────────────────────────────────────────────────

/// A single logical row in the rendered repository panel.
#[derive(Debug, Clone)]
pub enum PanelItem {
    /// Non-selectable separator (e.g. "Pinned", "(local)").
    SectionHeader(String),
    /// Selectable org header with expand/collapse state and total repo count.
    OrgHeader {
        org: String,
        expanded: bool,
        /// Configured repos + discovered repos for this org.
        total_count: usize,
        /// Source name this org belongs to (e.g. `"github"`, `"mygitea"`),
        /// used to select the correct platform icon.
        source: String,
    },
    /// "All repos in this org" sentinel — first child row under an expanded OrgHeader.
    OrgAll { org: String, source: String },
    /// A configured repo row — index into `repo_specs`.
    Repo(usize),
    /// A repo from `repo_db` that is not in `config.json`.
    DiscoveredRepo { address: String, source: String },
    /// A repo the user collaborates on (external org).
    LinkedRepo { address: String, source: String },
    /// A collapsible source-group header (e.g. "GitHub", "Gitea").
    SourceHeader { source: String, expanded: bool },
    /// The collapsable "Linked" section header.
    LinkedHeader {
        expanded: bool,
        total_count: usize,
        source: String,
    },
    /// The top-level Dashboard entry.
    Dashboard,
}

impl PanelItem {
    /// Convert this item into its corresponding [`PanelCursor`] identity, if it
    /// is selectable.  Non-selectable items (`SectionHeader`) return `None`.
    ///
    /// This is the single place that documents the `PanelItem ↔ PanelCursor`
    /// correspondence; `nav_order` and `cursor_matches` both delegate here so
    /// any new variant only needs to be handled once.
    pub fn to_cursor(&self) -> Option<PanelCursor> {
        match self {
            PanelItem::Repo(i) => Some(PanelCursor::Repo(*i)),
            PanelItem::OrgHeader { org, source, .. } => Some(PanelCursor::OrgHeader {
                org: org.clone(),
                source: source.clone(),
            }),
            PanelItem::OrgAll { org, source } => Some(PanelCursor::OrgAll {
                org: org.clone(),
                source: source.clone(),
            }),
            PanelItem::DiscoveredRepo { address, source } => Some(PanelCursor::DiscoveredRepo {
                address: address.clone(),
                source: source.clone(),
            }),
            PanelItem::LinkedRepo { address, source } => Some(PanelCursor::LinkedRepo {
                address: address.clone(),
                source: source.clone(),
            }),
            PanelItem::SourceHeader { source, .. } => Some(PanelCursor::SourceHeader {
                source: source.clone(),
            }),
            PanelItem::LinkedHeader { source, .. } => Some(PanelCursor::LinkedHeader {
                source: source.clone(),
            }),
            PanelItem::Dashboard => Some(PanelCursor::Dashboard),
            PanelItem::SectionHeader(_) => None,
        }
    }
}

// ── Item-list builder ─────────────────────────────────────────────────────────

/// Build the ordered list of [`PanelItem`]s for the given specs and db.
///
/// Layout (top → bottom):
/// 1. `Repo(0)` — the synthetic ALL sentinel
/// 2. `SectionHeader("Pinned")` + pinned repos  *(omitted when nothing is pinned)*
/// 3. For each unique non-empty org: `OrgHeader` + (if expanded) configured
///    repos then discovered repos from `repo_db`
/// 4. If any repos have an empty `organisation`: `SectionHeader("(local)")`
///    + those repos
pub fn build_panel_items(
    repo_specs: &[RepoSpec],
    mode: PanelMode,
    repo_db: &[RepoDbEntry],
    org_expanded: &HashMap<String, bool>,
    user_orgs: &[(String, String)],
    current_user: Option<&str>,
    linked_repos: &[RepoDbEntry],
    show_dashboard: bool,
) -> Vec<PanelItem> {
    let mut items: Vec<PanelItem> = Vec::new();

    if repo_specs.is_empty() {
        return items;
    }

    if show_dashboard {
        items.push(PanelItem::Dashboard);
    }
    // ALL sentinel is always index 0
    items.push(PanelItem::Repo(0));

    // Linked sections are now per-source, emitted inside each source group.

    let pinned: Vec<usize> = (1..repo_specs.len())
        .filter(|&i| repo_specs[i].pinned)
        .collect();
    let unpinned: Vec<usize> = (1..repo_specs.len())
        .filter(|&i| !repo_specs[i].pinned)
        .collect();

    // ── Pinned section ───────────────────────────────────────────────────────────────────
    if !pinned.is_empty() {
        items.push(PanelItem::SectionHeader("Pinned".to_string()));
        for i in pinned {
            items.push(PanelItem::Repo(i));
        }
    }

    // ── Org sections ──────────────────────────────────────────────────────────────────────
    let PanelMode::ByOrg = mode;

    // Start with orgs from configured repos, keyed by (source, org) so that the
    // same org name on different platforms gets its own separate header.
    let mut seen_orgs: Vec<(String, String)> = Vec::new();
    for &i in &unpinned {
        let org = &repo_specs[i].organisation;
        let src = &repo_specs[i].source;
        if !org.is_empty()
            && !seen_orgs
                .iter()
                .any(|(s, o)| s.eq_ignore_ascii_case(src) && o.eq_ignore_ascii_case(org))
        {
            seen_orgs.push((src.clone(), org.clone()));
        }
    }
    // Add every (source, org) pair the user is a member of (from the API).
    for (src, org) in user_orgs {
        if !org.is_empty()
            && !seen_orgs
                .iter()
                .any(|(s, o)| s.eq_ignore_ascii_case(src) && o.eq_ignore_ascii_case(org))
        {
            seen_orgs.push((src.clone(), org.clone()));
        }
    }
    // Fallback: pick up (source, org) pairs from repo_db not yet covered.
    for entry in repo_db {
        let org = &entry.organisation;
        let src = &entry.source;
        if !org.is_empty()
            && !seen_orgs
                .iter()
                .any(|(s, o)| s.eq_ignore_ascii_case(src) && o.eq_ignore_ascii_case(org))
        {
            let is_personal = current_user.map_or(false, |u| u.eq_ignore_ascii_case(org));
            let has_configured = repo_specs.iter().any(|r| {
                r.organisation.eq_ignore_ascii_case(org)
                    && (r.source.is_empty() || r.source.eq_ignore_ascii_case(src))
            });
            if is_personal || has_configured {
                seen_orgs.push((src.clone(), org.clone()));
            }
        }
    }

    // Collect unique sources in first-appearance order so we can emit one
    // section divider per source when multiple platforms are configured.
    let mut unique_sources: Vec<&str> = Vec::new();
    for (src, _) in &seen_orgs {
        if !unique_sources.iter().any(|s| s.eq_ignore_ascii_case(src)) {
            unique_sources.push(src.as_str());
        }
    }
    let multi_source = unique_sources.len() > 1;

    for src in unique_sources {
        // In multi-source mode emit a collapsible source header; its children
        // are only rendered when expanded (default: true).
        let src_expanded = if multi_source {
            let src_key = format!("__source__:{}", src);
            let exp = *org_expanded.get(src_key.as_str()).unwrap_or(&true);
            items.push(PanelItem::SourceHeader {
                source: src.to_string(),
                expanded: exp,
            });
            exp
        } else {
            true
        };

        if src_expanded {
            for (source, org) in seen_orgs
                .iter()
                .filter(|(s, _)| s.eq_ignore_ascii_case(src))
            {
                // Configured repos for this (source, org) pair (non-pinned).
                let configured: Vec<usize> = unpinned
                    .iter()
                    .copied()
                    .filter(|&i| {
                        repo_specs[i].organisation.eq_ignore_ascii_case(org)
                            && repo_specs[i].source.eq_ignore_ascii_case(source)
                    })
                    .collect();

                // Discovered repos for this (source, org) pair (not already in config).
                let discovered: Vec<(String, String)> = repo_db
                    .iter()
                    .filter(|e| {
                        e.organisation.eq_ignore_ascii_case(org)
                            && e.source.eq_ignore_ascii_case(source)
                    })
                    .filter(|e| {
                        !repo_specs
                            .iter()
                            .any(|r| r.address.eq_ignore_ascii_case(&e.address))
                    })
                    .map(|e| (e.address.clone(), e.source.clone()))
                    .collect();

                let total_count = configured.len() + discovered.len();
                let key = format!("{}:{}", source, org);
                let expanded = *org_expanded.get(key.as_str()).unwrap_or(&false);

                items.push(PanelItem::OrgHeader {
                    org: org.clone(),
                    expanded,
                    total_count,
                    source: source.clone(),
                });

                if expanded {
                    // "All" sentinel — lets the user fetch every repo in this org at once.
                    if total_count > 0 {
                        items.push(PanelItem::OrgAll {
                            org: org.clone(),
                            source: source.clone(),
                        });
                    }
                    for i in configured {
                        items.push(PanelItem::Repo(i));
                    }
                    for (address, source) in discovered {
                        items.push(PanelItem::DiscoveredRepo { address, source });
                    }
                }
            }

            // Linked repos for this source: collaborator repos not already covered
            // by a configured spec or an org the user is a member of.
            let linked_for_source: Vec<(String, String)> = linked_repos
                .iter()
                .filter(|e| e.source.eq_ignore_ascii_case(src))
                .filter(|e| {
                    !repo_specs
                        .iter()
                        .any(|r| r.address.eq_ignore_ascii_case(&e.address))
                        && !user_orgs.iter().any(|(s, o)| {
                            s.eq_ignore_ascii_case(&e.source)
                                && o.eq_ignore_ascii_case(&e.organisation)
                        })
                        && current_user.map_or(true, |u| !e.organisation.eq_ignore_ascii_case(u))
                })
                .map(|e| (e.address.clone(), e.source.clone()))
                .collect();
            if !linked_for_source.is_empty() {
                let linked_key = format!("__linked__:{}", src);
                let expanded = *org_expanded.get(linked_key.as_str()).unwrap_or(&false);
                items.push(PanelItem::LinkedHeader {
                    expanded,
                    total_count: linked_for_source.len(),
                    source: src.to_string(),
                });
                if expanded {
                    for (address, source) in linked_for_source {
                        items.push(PanelItem::LinkedRepo { address, source });
                    }
                }
            }
        } // end if src_expanded
    }

    // Repos with no organisation are omitted — they appear under their org
    // once discovered, and the (local) section has been removed.

    items
}

/// Return the ordered list of navigable [`PanelCursor`] values (section headers
/// are excluded).  Used by `App` to drive Up / Down in the Repos panel.
pub fn nav_order(
    repo_specs: &[RepoSpec],
    mode: PanelMode,
    repo_db: &[RepoDbEntry],
    org_expanded: &HashMap<String, bool>,
    user_orgs: &[(String, String)],
    current_user: Option<&str>,
    linked_repos: &[RepoDbEntry],
    show_dashboard: bool,
) -> Vec<PanelCursor> {
    build_panel_items(
        repo_specs,
        mode,
        repo_db,
        org_expanded,
        user_orgs,
        current_user,
        linked_repos,
        show_dashboard,
    )
    .into_iter()
    .filter_map(|item| item.to_cursor())
    .collect()
}

// ── Context ───────────────────────────────────────────────────────────────────

pub struct RepoPanelCtx<'a> {
    /// Pre-computed panel items — build with [`build_panel_items`].
    pub items: &'a [PanelItem],
    /// The full repo-spec slice (needed to read names, colours, pinned flags).
    pub repo_specs: &'a [RepoSpec],
    /// Currently highlighted item.
    pub panel_cursor: &'a PanelCursor,
    /// The repo that was last loaded (Enter pressed), independent of cursor position.
    pub active_cursor: Option<&'a PanelCursor>,
    /// Whether the Repos pane has keyboard focus (controls highlight style).
    pub focus_repos: bool,
    /// Source names whose platform is Gitea — used to render the 🫖 icon.
    pub gitea_source_names: &'a [String],
    /// Orgs the authenticated user belongs to (used for Linked section logic).
    pub user_orgs: &'a [(String, String)],
    /// The authenticated user's login handle.
    pub current_user: Option<&'a str>,
    /// Repos fetched via the collaborator API — source for the Linked panel section.
    pub linked_repos: &'a [RepoDbEntry],
}

// ── Rendering ─────────────────────────────────────────────────────────────────

pub fn render_repo_panel(f: &mut Frame, area: Rect, ctx: RepoPanelCtx<'_>) {
    let block = Block::default().borders(Borders::ALL).title("Repos");

    if ctx.repo_specs.is_empty() {
        f.render_widget(Paragraph::new("(no repos configured)").block(block), area);
        return;
    }

    f.render_widget(block, area);

    let inner = crate::tui::inner_rect(area);
    let inner_w = inner.width as usize;
    let inner_h = inner.height as usize;

    // Returns true when `cursor` refers to the same logical row as `item`.
    // Delegates to `PanelItem::to_cursor` so the mapping is defined once.
    let cursor_matches = |item: &PanelItem, cursor: &PanelCursor| -> bool {
        item.to_cursor().as_ref() == Some(cursor)
    };

    // Scroll offset is computed here rather than in draw() so the cursor-matching
    // logic lives in one place — cursor_matches is reused for both scroll and highlight.
    let scroll_offset = {
        let sel_row = ctx
            .items
            .iter()
            .position(|item| cursor_matches(item, ctx.panel_cursor))
            .unwrap_or(0);
        if sel_row >= inner_h {
            sel_row.saturating_sub(inner_h.saturating_sub(1))
        } else {
            0
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    for (row_idx, item) in ctx.items.iter().enumerate() {
        if row_idx < scroll_offset {
            continue;
        }
        if lines.len() >= inner_h {
            break;
        }

        match item {
            // ── Dashboard entry ───────────────────────────────────────────
            PanelItem::Dashboard => {
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let is_active = ctx
                    .active_cursor
                    .map_or(false, |ac| cursor_matches(item, ac));
                let label = "  \u{229e} Dashboard";
                let pad = inner_w.saturating_sub(label.chars().count());
                let display = format!("{}{}", label, " ".repeat(pad));
                let span = if is_selected && ctx.focus_repos {
                    let mut style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_active {
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                };
                lines.push(Line::from(span));
            }

            // ── Non-selectable section header ─────────────────────────────
            PanelItem::SectionHeader(name) => {
                let label = format!("─ {} ", name);
                let fill = inner_w.saturating_sub(label.chars().count());
                let full = format!("{}{}", label, "─".repeat(fill));
                lines.push(Line::from(Span::styled(
                    full,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )));
            }

            // ── Collapsible source-group header ───────────────────────────
            PanelItem::SourceHeader { source, expanded } => {
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let arrow = if *expanded { "▾" } else { "▸" };
                let label = format!("─ {} {} ", arrow, source);
                let fill = inner_w.saturating_sub(label.chars().count());
                let full = format!("{}{}", label, "─".repeat(fill));
                let style = if is_selected && ctx.focus_repos {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                };
                lines.push(Line::from(Span::styled(full, style)));
            }

            // ── Selectable org header ─────────────────────────────────────
            PanelItem::OrgHeader {
                org,
                expanded,
                total_count,
                source,
            } => {
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let is_active = ctx
                    .active_cursor
                    .map_or(false, |ac| cursor_matches(item, ac));

                // Gitea orgs use the teacup emoji (🫖) instead of the
                // expand/collapse arrow to signal a different source platform.
                // The emoji occupies 2 terminal columns but counts as 1 char,
                // so we add 1 to the visual width when computing padding.
                let is_gitea = ctx.gitea_source_names.iter().any(|s| s == source);
                let (prefix, extra_cols): (&str, usize) = if is_gitea {
                    ("🫖", 1)
                } else if *expanded {
                    ("▾", 0)
                } else {
                    ("▸", 0)
                };
                let label = format!("{} {} ({})", prefix, org, total_count);

                // Pad to inner width, accounting for wide emoji.
                let pad = inner_w.saturating_sub(label.chars().count() + extra_cols);
                let display = format!("{}{}", label, " ".repeat(pad));

                let span: Span = if is_selected && ctx.focus_repos {
                    // Navigation cursor, focused: yellow highlight; underline if also active
                    let mut style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD);
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_active {
                    // Active repo (issues loaded), cursor elsewhere: cyan accent
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(display, Style::default().add_modifier(Modifier::BOLD))
                };
                lines.push(Line::from(span));
            }

            // ── All-repos-in-org sentinel ─────────────────────────────────────────
            PanelItem::OrgAll { org: _, source: _ } => {
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let is_active = ctx
                    .active_cursor
                    .map_or(false, |ac| cursor_matches(item, ac));

                let label = format!("  ≡ All");
                let pad = inner_w.saturating_sub(label.chars().count());
                let display = format!("{}{}", label, " ".repeat(pad));

                let span: Span = if is_selected && ctx.focus_repos {
                    let mut style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD);
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_active {
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(display, Style::default().add_modifier(Modifier::ITALIC))
                };
                lines.push(Line::from(span));
            }

            // ── Configured repo ────────────────────────────────────────────
            PanelItem::Repo(spec_idx) => {
                let spec = &ctx.repo_specs[*spec_idx];
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let is_active = ctx
                    .active_cursor
                    .map_or(false, |ac| cursor_matches(item, ac));
                let is_all = spec.name == ALL_CONFIGURED;

                // Display name: ALL and pinned repos show their full name;
                // repos under an org header show only the repo part.
                let in_pinned = is_in_pinned_section(ctx.items, row_idx);
                let display_name: String = if is_all || in_pinned {
                    spec.name.clone()
                } else {
                    repo_part(&spec.name).to_owned()
                };

                let indent = if is_all { 0usize } else { 2usize };
                let sel = if is_selected { "▸ " } else { "  " };
                let full_label = format!("{}{}{}", " ".repeat(indent), sel, display_name);

                let colour_present = !spec.colour.trim().is_empty();
                let colour_w = if colour_present { 2usize } else { 0usize };
                let available = inner_w.saturating_sub(colour_w);

                let mut display = full_label;
                if display.chars().count() > available {
                    if available > 0 {
                        let take = available.saturating_sub(1);
                        display = display.chars().take(take).collect::<String>();
                        display.push('…');
                    } else {
                        display = String::new();
                    }
                }
                let pad = available.saturating_sub(display.chars().count());

                let is_local = !spec.path.trim().is_empty();
                let name_span: Span = if is_selected && ctx.focus_repos && is_local {
                    // Navigation cursor on a local repo, focused: yellow highlight;
                    // underline if also active.
                    let mut style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD);
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_selected && ctx.focus_repos {
                    // Navigation cursor on a non-local (remote-only) repo:
                    // show a muted highlight so the cursor position is still
                    // visible but the bright yellow is not used.
                    let mut style = Style::default().fg(Color::Gray).bg(Color::Rgb(50, 50, 50));
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_active {
                    // Active repo (issues loaded), cursor elsewhere: cyan accent
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw(display)
                };

                let mut spans: Vec<Span> = vec![name_span];
                spans.push(Span::raw(" ".repeat(pad + 1)));
                if colour_present {
                    if let Some(rgb) = crate::tui::parse_css_color_to_rgb(&spec.colour) {
                        spans.push(Span::styled("  ".to_string(), Style::default().bg(rgb)));
                    } else {
                        spans.push(Span::raw("  ".to_string()));
                    }
                }
                lines.push(Line::from(spans));
            }

            // ── Discovered (not-yet-configured) repo ──────────────────────────
            PanelItem::DiscoveredRepo { address, source } => {
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let is_active = ctx
                    .active_cursor
                    .map_or(false, |ac| cursor_matches(item, ac));

                // Show only the repo part (we're already under an org header)
                let repo_name = repo_part(address);
                let label = format!("  + {}", repo_name);

                let pad = inner_w.saturating_sub(label.chars().count());
                let display = format!("{}{}", label, " ".repeat(pad));

                let span: Span = if is_selected && ctx.focus_repos {
                    // Navigation cursor, focused: dark-grey highlight; underline if also active
                    let mut style = Style::default().fg(Color::Black).bg(Color::Rgb(80, 80, 80));
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_active {
                    // Active discovered repo, cursor elsewhere: cyan accent
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(display, Style::default().fg(Color::DarkGray))
                };
                // Suppress unused variable warning on `source` — it is stored
                // in the item for use by App but not needed for rendering.
                let _ = source;
                lines.push(Line::from(span));
            }

            // ── Linked section header (collapsable) ─────────────────────────────
            PanelItem::LinkedHeader {
                expanded,
                total_count,
                source: _,
            } => {
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let is_active = ctx
                    .active_cursor
                    .map_or(false, |ac| cursor_matches(item, ac));
                let arrow = if *expanded { "▾" } else { "▸" };
                let label = format!("{} Linked ({})", arrow, total_count);
                let pad = inner_w.saturating_sub(label.chars().count());
                let display = format!("{}{}", label, " ".repeat(pad));

                let span: Span = if is_selected && ctx.focus_repos {
                    let mut style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::Magenta)
                        .add_modifier(Modifier::BOLD);
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_active {
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    )
                };
                lines.push(Line::from(span));
            }

            // ── Linked (external-org) repo ────────────────────────────────────
            PanelItem::LinkedRepo { address, source } => {
                let is_selected = cursor_matches(item, ctx.panel_cursor);
                let is_active = ctx
                    .active_cursor
                    .map_or(false, |ac| cursor_matches(item, ac));

                // Show the short repo name; the section header provides context.
                let display_name = address.split('/').last().unwrap_or(address.as_str());
                let label = format!("  ⇗ {}", display_name);

                let pad = inner_w.saturating_sub(label.chars().count());
                let display = format!("{}{}", label, " ".repeat(pad));

                let span: Span = if is_selected && ctx.focus_repos {
                    // Navigation cursor, focused: dark-grey highlight; underline if also active
                    let mut style = Style::default().fg(Color::Black).bg(Color::DarkGray);
                    if is_active {
                        style = style.add_modifier(Modifier::UNDERLINED);
                    }
                    Span::styled(display, style)
                } else if is_active {
                    // Active linked repo, cursor elsewhere: magenta bold accent
                    Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled(display, Style::default().fg(Color::Magenta))
                };
                // `source` is stored for use by App but not needed for rendering.
                let _ = source;
                lines.push(Line::from(span));
            }
        }
    }

    f.render_widget(Paragraph::new(Text::from(lines)), inner);
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn is_in_pinned_section(items: &[PanelItem], row_idx: usize) -> bool {
    for item in items[..row_idx].iter().rev() {
        if let PanelItem::SectionHeader(name) = item {
            return name == "Pinned";
        }
        // An OrgHeader breaks the Pinned section
        if matches!(item, PanelItem::OrgHeader { .. }) {
            return false;
        }
    }
    false
}

/// Extract the organisation part of `"org/repo"`.
pub fn org_of(name: &str) -> String {
    name.split('/').next().unwrap_or(name).to_string()
}

/// Extract the repository part of `"org/repo"`.
fn repo_part(name: &str) -> &str {
    name.rfind('/').map_or(name, |i| &name[i + 1..])
}
