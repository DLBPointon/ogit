//! Application state, event loop, and thin draw dispatcher.

use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Span, Text};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};

use crate::filter::{FilterStage, FilterState};
use crate::model::{
    ALL_CONFIGURED, Comment, IssueList, IssueSource, Platform, RepoDbEntry, RepoSpec,
};
use crate::tui::repo_panel::{PanelCursor, PanelItem, PanelMode, build_panel_items, nav_order};
use crate::worker::{BackendRequest, BackendResponse};

// ──────────────────────────────────────────────────────────────────────────────
// Enums
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Details,
    Dashboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Repos,
    Issues,
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Open a URL in the system's default browser.
fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
}

// ──────────────────────────────────────────────────────────────────────────────
// Sub-structs
// ──────────────────────────────────────────────────────────────────────────────

/// All state related to the issue-details / comments panel.
pub struct DetailsState {
    pub comments: Option<Vec<Comment>>,
    pub loading: bool,
    pub error: Option<String>,
    pub scroll: usize,
    pub has_more_pages: bool,
    /// Page most recently *requested* (pre-incremented before each send).
    pub comment_page: usize,
    pub comments_url: String,
    pub issue_number: u32,
    pub source_name: String,
    /// When true, keep fetching pages until `has_more_pages` is false (G key).
    pub load_all: bool,
}

impl Default for DetailsState {
    fn default() -> Self {
        Self {
            comments: None,
            loading: false,
            error: None,
            scroll: 0,
            has_more_pages: false,
            comment_page: 1,
            comments_url: String::new(),
            issue_number: 0,
            source_name: String::new(),
            load_all: false,
        }
    }
}

/// The currently-active issue filter values.
/// `pub` so `main` can set `app.filters.status = None` for the `--issue` flag.
pub struct ActiveFilters {
    pub author: Option<String>,
    pub tag: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
    /// `None` = show both issues and PRs; `Some("issue")` or `Some("pr")` to restrict.
    pub kind: Option<String>,
    /// Selected repo names; empty = show all repos.
    pub repo: Vec<String>,
}

impl Default for ActiveFilters {
    fn default() -> Self {
        Self {
            author: None,
            tag: None,
            status: Some("open".to_string()),
            search: None,
            kind: None,
            repo: Vec::new(),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Utilities
// ──────────────────────────────────────────────────────────────────────────────

/// Copy `text` to the system clipboard. Silently no-ops if the clipboard is
/// unavailable (e.g. no display server on a headless system).
fn copy_to_clipboard(text: &str) {
    if let Ok(mut ctx) = arboard::Clipboard::new() {
        let _ = ctx.set_text(text.to_string());
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// App
// ──────────────────────────────────────────────────────────────────────────────

pub struct App {
    // channels to the background worker
    backend_tx: mpsc::Sender<BackendRequest>,
    backend_rx: mpsc::Receiver<BackendResponse>,

    // repository list — pub so main can push the local repo and the ALL sentinel
    pub repo_specs: Vec<RepoSpec>,
    pub(crate) panel_cursor: PanelCursor,
    pub repo_db: Vec<RepoDbEntry>,
    org_expanded: HashMap<String, bool>,
    /// All (source_name, org_name) pairs the authenticated user is a member of.
    /// Keyed by source so that same-named orgs on different platforms stay separate.
    user_orgs: Vec<(String, String)>,
    /// The authenticated user's login (from config `user` field).
    pub current_user: Option<String>,
    /// Repos the user collaborates on but doesn't own — source for the LINKED panel section.
    linked_repos: Vec<RepoDbEntry>,

    // dashboard
    /// Whether to show the Dashboard entry in the panel (set when not in a git repo).
    show_dashboard: bool,
    /// Names of sources whose platform is Gitea — computed once at startup,
    /// passed to the repo panel to render the 🫖 icon on Gitea org headers.
    gitea_source_names: Vec<String>,
    /// Total issues the user is involved in — populated by UserStats response.
    /// `None` = not yet loaded; `Some(None)` = platform does not support this metric (e.g. GitLab);
    /// `Some(Some(n))` = loaded count.
    dashboard_issue_count: Option<Option<u64>>,
    /// Number of `FetchUserOrgs` responses still awaited.  Drives the
    /// "Loading…" indicator on the Organisations dashboard row.
    orgs_pending: usize,

    // active issues
    issues: Option<IssueList>,
    loading: bool,
    /// Progress for multi-repo fetches: (repos_done, repos_total, last_completed_repo).
    /// None for single-repo loads.
    loading_progress: Option<(usize, usize, String)>,
    /// Issues result deferred from the previous tick when IssueProgress was also
    /// processed — ensures at least one frame renders the final progress state.
    pending_issues: Option<Result<IssueList, String>>,
    error: Option<String>,
    selected: usize,
    active_cursor: Option<PanelCursor>,
    view: View,
    focus: Focus,

    // filtering
    pub filters: ActiveFilters,
    filter_state: Option<FilterState>,
    /// The `status` value that was sent to the API in the most recent fetch.
    /// Used to detect when a status filter change requires a new network request.
    fetched_state: Option<String>,

    // tag picker UI state
    tag_picker_open: bool,
    tag_picker_items: Vec<String>,
    tag_picker_selected: usize,

    // details/comments state
    details: DetailsState,
    /// Feedback from the last render: total assembled line count in the body.
    content_lines: usize,
    /// Feedback from the last render: visible line height of the body area.
    visible_lines: usize,

    // config paths — repo_path is pub so main can auto-detect the local git repo
    pub repo_path: String,

    // root directory for clone operations — resolved once at startup
    clone_root: String,

    // small status message shown at the bottom (warnings / feedback)
    status_message: Option<String>,

    // help overlay
    help_open: bool,

    // clone result modals
    clone_error: Option<String>,
    clone_success: Option<String>,
    /// Error from a background operation (org refresh, org discovery, etc.).
    /// Shown as a dismissable modal so the full message is always readable.
    background_error: Option<String>,

    /// Number of comments to request per API page (from config, default 10).
    comments_per_page: usize,

    /// If set at startup (via --issue), jump straight to this issue number once
    /// the first issue list arrives.
    pub startup_issue_number: Option<u32>,
}

impl App {
    pub fn new() -> Self {
        // Load config first so derived settings (thread count, etc.) are available
        // before the worker is spawned.
        let config = crate::backend::load_config().ok();

        let max_threads = config
            .as_ref()
            .map(|cfg_ref| cfg_ref.max_worker_threads)
            .unwrap_or(4);

        let (tx_req, rx_req) = mpsc::channel::<BackendRequest>();
        let (tx_resp, rx_resp) = mpsc::channel::<BackendResponse>();

        thread::spawn(move || crate::worker::worker(rx_req, tx_resp, max_threads));

        let repo_path = ".git/config".to_string();

        let repo_specs = match crate::backend::load_repo_specs() {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };

        // Load persisted org-discovery database (non-fatal if absent).
        let repo_db = crate::backend::load_repo_db().unwrap_or_default();

        let clone_root = config
            .as_ref()
            .and_then(|cfg_ref| cfg_ref.clone_root.clone())
            .unwrap_or_else(|| {
                std::env::var("HOME")
                    .map(|home_dir| format!("{}/code", home_dir))
                    .unwrap_or_else(|_| "./code".to_string())
            });
        let comments_per_page = config
            .as_ref()
            .map(|cfg_ref| cfg_ref.comments_per_page)
            .unwrap_or(10);
        let current_user = config.as_ref().and_then(|cfg_ref| cfg_ref.user.clone());

        let gitea_source_names: Vec<String> = config
            .as_ref()
            .map(|cfg_ref| {
                cfg_ref
                    .sources
                    .iter()
                    .filter(|source_cfg| {
                        source_cfg.enabled && matches!(source_cfg.platform, Platform::Gitea)
                    })
                    .map(|source_cfg| source_cfg.name.clone())
                    .collect()
            })
            .unwrap_or_default();

        // Kick off a full user-org discovery for every enabled source.
        let mut orgs_pending = 0usize;
        if let Some(ref cfg) = config {
            for source_cfg in cfg.sources.iter().filter(|source_cfg| source_cfg.enabled) {
                let _ = tx_req.send(BackendRequest::FetchUserOrgs {
                    source_name: source_cfg.name.clone(),
                });
                orgs_pending += 1;
                let _ = tx_req.send(BackendRequest::FetchCollaboratorRepos {
                    source_name: source_cfg.name.clone(),
                });
            }
        }

        // Kick off a refresh for any orgs already listed in the config.
        {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for spec in &repo_specs {
                if !spec.organisation.is_empty()
                    && !spec.source.is_empty()
                    && seen.insert(spec.organisation.clone())
                {
                    let _ = tx_req.send(BackendRequest::FetchOrgRepos {
                        org: spec.organisation.clone(),
                        source_name: spec.source.clone(),
                    });
                }
            }
        }

        App {
            backend_tx: tx_req,
            backend_rx: rx_resp,
            repo_specs,
            panel_cursor: PanelCursor::default(),
            repo_db: repo_db.entries,
            org_expanded: HashMap::new(),
            issues: None,
            loading: false,
            loading_progress: None,
            pending_issues: None,
            error: None,
            selected: 0,
            active_cursor: None,
            view: View::List,
            focus: Focus::Repos,
            filters: ActiveFilters::default(),
            filter_state: None,
            fetched_state: Some("open".to_string()),
            tag_picker_open: false,
            tag_picker_items: Vec::new(),
            tag_picker_selected: 0,
            details: DetailsState::default(),
            content_lines: 0,
            visible_lines: 0,
            repo_path,
            clone_root,
            status_message: None,
            help_open: false,
            clone_error: None,
            clone_success: None,
            background_error: None,
            comments_per_page,
            startup_issue_number: None,
            user_orgs: Vec::new(),
            current_user,
            linked_repos: Vec::new(),
            show_dashboard: false,
            dashboard_issue_count: None,
            orgs_pending,
            gitea_source_names,
        }
    }

    // ── Public API ──────────────────────────────────────────────────────────

    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> io::Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame)).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Terminal draw failed: {}", err),
                )
            })?;

            self.drain_backend_responses();

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    // Filter modal captures all input while open.
                    if self.filter_state.is_some() {
                        self.handle_filter_modal_keys(key);
                        continue;
                    }

                    // Tag picker captures all input while open.
                    if self.tag_picker_open {
                        self.handle_tag_picker_keys(key);
                        continue;
                    }

                    // One-shot modal dismissals — any key closes them.
                    // Help is checked first so it always has priority over
                    // background notifications that may have arrived concurrently.
                    if self.help_open {
                        self.help_open = false;
                        continue;
                    }
                    if self.background_error.is_some() {
                        if key.code == KeyCode::Char('c') {
                            if let Some(ref err) = self.background_error {
                                copy_to_clipboard(err);
                                self.status_message = Some("Error copied to clipboard".to_string());
                            }
                        }
                        self.background_error = None;
                        continue;
                    }
                    if self.clone_error.is_some() {
                        if key.code == KeyCode::Char('c') {
                            if let Some(ref err) = self.clone_error {
                                copy_to_clipboard(err);
                                self.status_message = Some("Error copied to clipboard".to_string());
                            }
                        }
                        self.clone_error = None;
                        continue;
                    }
                    if self.clone_success.is_some() {
                        self.clone_success = None;
                        continue;
                    }

                    // Block all input while a fetch is in flight (except quit).
                    if self.loading && key.code != KeyCode::Char('q') {
                        continue;
                    }

                    if self.handle_main_keys(key)? {
                        return Ok(());
                    }
                }
            }
        }
    }

    pub fn fetch_selected_repo(&mut self) {
        if self.repo_specs.is_empty() {
            return;
        }

        self.active_cursor = Some(self.panel_cursor.clone());
        // Snapshot the status being fetched so we can detect later filter changes.
        self.fetched_state = self.filters.status.clone();
        // Reset progress state for all loading paths so stale data never bleeds
        // into the new fetch.
        self.loading_progress = None;
        self.pending_issues = None;

        match self.panel_cursor.clone() {
            // ── ALL sentinel ──────────────────────────────────────────────
            PanelCursor::Repo(0) => {
                let real_specs: Vec<RepoSpec> = self.repo_specs[1..].to_vec();
                if real_specs.is_empty() {
                    return;
                }
                let _ = self.backend_tx.send(BackendRequest::FetchIssuesMany {
                    repo_specs: real_specs,
                    state_filter: self.filters.status.clone(),
                });
                self.loading = true;
                self.loading_progress = None;
                self.pending_issues = None;
            }

            // ── Single configured repo ────────────────────────────────────
            PanelCursor::Repo(i) => {
                let Some(spec) = self.repo_specs.get(i).cloned() else {
                    return;
                };
                let _ = self.backend_tx.send(BackendRequest::FetchIssues {
                    repo_spec: spec,
                    creator: None,
                    state_filter: self.filters.status.clone(),
                });
                self.loading = true;
            }

            PanelCursor::OrgHeader { .. }
            | PanelCursor::SourceHeader { .. }
            | PanelCursor::LinkedHeader { .. }
            | PanelCursor::Dashboard => {}

            // ── Org-level All — fetch every repo (configured + discovered) in one org ───
            PanelCursor::OrgAll { org, source } => {
                // Configured specs for this (source, org).
                let mut specs: Vec<RepoSpec> = self.repo_specs[1..]
                    .iter()
                    .filter(|r| {
                        r.organisation.eq_ignore_ascii_case(&org)
                            && r.source.eq_ignore_ascii_case(&source)
                    })
                    .cloned()
                    .collect();

                // Synthetic specs for discovered repos not yet in config.
                for entry in &self.repo_db {
                    if entry.organisation.eq_ignore_ascii_case(&org)
                        && entry.source.eq_ignore_ascii_case(&source)
                        && !self
                            .repo_specs
                            .iter()
                            .any(|r| r.address.eq_ignore_ascii_case(&entry.address))
                    {
                        specs.push(RepoSpec::from_address(
                            entry.address.clone(),
                            source.clone(),
                        ));
                    }
                }

                if specs.is_empty() {
                    return;
                }
                let _ = self.backend_tx.send(BackendRequest::FetchIssuesMany {
                    repo_specs: specs,
                    state_filter: self.filters.status.clone(),
                });
                self.loading = true;
                self.loading_progress = None;
                self.pending_issues = None;
            }

            // ── Discovered repo — create a temporary RepoSpec ─────────────
            PanelCursor::DiscoveredRepo { address, source } => {
                let spec = RepoSpec::from_address(address, source);
                let _ = self.backend_tx.send(BackendRequest::FetchIssues {
                    repo_spec: spec,
                    creator: None,
                    state_filter: self.filters.status.clone(),
                });
                self.loading = true;
            }

            // ── Linked repo — fetch only issues created by the current user ─
            PanelCursor::LinkedRepo { address, source } => {
                let spec = RepoSpec::from_address(address, source);
                let _ = self.backend_tx.send(BackendRequest::FetchIssues {
                    repo_spec: spec,
                    creator: self.current_user.clone(),
                    state_filter: self.filters.status.clone(),
                });
                self.loading = true;
            }
        }
    }

    /// Activate the dashboard: switch view, set cursor, mark show_dashboard.
    /// Called from `main` when ogit is opened outside a git repository.
    pub fn activate_dashboard(&mut self) {
        self.show_dashboard = true;
        self.view = View::Dashboard;
        self.panel_cursor = PanelCursor::Dashboard;
    }

    /// Expose the backend sender so `main` can enqueue one-off startup requests.
    pub fn backend_tx_for_main(&self) -> &mpsc::Sender<BackendRequest> {
        &self.backend_tx
    }

    // ── Backend response handling ────────────────────────────────────────────

    /// Apply a completed `Issues` result (used by both the normal and the
    /// deferred path in `drain_backend_responses`).
    fn apply_issues_result(&mut self, res: Result<IssueList, String>) {
        match res {
            Ok(list) => {
                self.status_message = if list.warnings.is_empty() {
                    None
                } else {
                    Some(list.warnings.join(" | "))
                };
                self.issues = Some(list);
                self.loading = false;
                self.loading_progress = None;
                self.error = None;
                self.selected = 0;
                self.view = View::List;

                // --issue: jump straight to the requested issue number.
                if let Some(n) = self.startup_issue_number.take() {
                    let pos = self
                        .filtered_issues()
                        .iter()
                        .position(|issue| issue.number == n);
                    if let Some(idx) = pos {
                        self.selected = idx;
                        self.view = View::Details;
                        self.open_details_for_selected();
                    }
                }
            }
            Err(e) => {
                self.error = Some(e);
                self.loading = false;
                self.loading_progress = None;
            }
        }
    }

    fn drain_backend_responses(&mut self) {
        // If an Issues result was deferred last tick (to let the final progress
        // state render for one frame), apply it now before processing new messages.
        if let Some(res) = self.pending_issues.take() {
            self.apply_issues_result(res);
        }

        // Track whether any IssueProgress arrived this tick.  If so, the
        // terminal Issues response is deferred to the next tick so the progress
        // bar is visible for at least one rendered frame.
        let mut got_progress = false;

        while let Ok(msg) = self.backend_rx.try_recv() {
            match msg {
                BackendResponse::IssueProgress {
                    fetched,
                    total,
                    current_repo,
                } => {
                    self.loading_progress = Some((fetched, total, current_repo));
                    got_progress = true;
                }

                BackendResponse::Issues(res) => {
                    if got_progress {
                        // Defer so the progress bar renders for exactly one frame.
                        self.pending_issues = Some(res);
                    } else {
                        self.apply_issues_result(res);
                    }
                }

                BackendResponse::IssueDetails(res) => match res {
                    Ok((number, new_comments, _cached, has_more)) => {
                        // Discard stale responses from a previously-viewed issue.
                        // If the user navigated away quickly, in-flight comments
                        // for the old issue must not overwrite the current one.
                        if number != self.details.issue_number {
                            continue;
                        }
                        // Append so page > 1 accumulates.
                        if let Some(existing) = self.details.comments.as_mut() {
                            existing.extend(new_comments);
                        } else {
                            self.details.comments = Some(new_comments);
                        }
                        self.details.has_more_pages = has_more;
                        self.details.loading = false;
                        self.details.error = None;
                        // G key: chain-load all remaining pages.
                        if self.details.load_all {
                            if has_more {
                                self.load_next_comment_page();
                            } else {
                                self.details.load_all = false;
                            }
                            // Keep scroll pinned to bottom as content arrives.
                            self.details.scroll = self.details_max_scroll();
                        }
                    }
                    Err(e) => {
                        self.details.error = Some(e);
                        self.details.loading = false;
                        self.details.load_all = false;
                    }
                },

                BackendResponse::OrgRepos { org, result } => match result {
                    Ok(entries) => {
                        self.repo_db.retain(|entry| entry.organisation != org);
                        self.repo_db.extend(entries);
                        let repo_db_payload = crate::model::RepoDb {
                            entries: self.repo_db.clone(),
                        };
                        let _ = crate::backend::save_repo_db(&repo_db_payload);
                    }
                    Err(e) => {
                        self.background_error =
                            Some(format!("Org '{}' refresh failed: {}", org, e));
                    }
                },

                BackendResponse::UserOrgs {
                    source_name,
                    result,
                } => {
                    // Each UserOrgs response (success or error) settles one pending request.
                    self.orgs_pending = self.orgs_pending.saturating_sub(1);
                    match result {
                        Ok(orgs) => {
                            for org in &orgs {
                                if !self.user_orgs.iter().any(|(source_entry, org_entry)| {
                                    source_entry.eq_ignore_ascii_case(&source_name)
                                        && org_entry.eq_ignore_ascii_case(org)
                                }) {
                                    self.user_orgs.push((source_name.clone(), org.clone()));
                                }
                            }
                            for org in orgs {
                                let _ = self.backend_tx.send(BackendRequest::FetchOrgRepos {
                                    org,
                                    source_name: source_name.clone(),
                                });
                            }
                        }
                        Err(e) => {
                            self.background_error =
                                Some(format!("Org discovery failed ({}): {}", source_name, e));
                        }
                    }
                }

                BackendResponse::CollaboratorRepos {
                    source_name: _,
                    result,
                } => match result {
                    Ok(entries) => {
                        for entry in entries {
                            if let Some(existing) =
                                self.linked_repos.iter_mut().find(|existing_entry| {
                                    existing_entry.address.eq_ignore_ascii_case(&entry.address)
                                })
                            {
                                *existing = entry;
                            } else {
                                self.linked_repos.push(entry);
                            }
                        }
                    }
                    Err(e) => {
                        self.background_error = Some(format!("Linked repos fetch failed: {}", e));
                    }
                },

                BackendResponse::Cloned { address, result } => match result {
                    Ok((path, colour)) => {
                        let cloned_path = path.clone();
                        if let Some(repo_spec) = self
                            .repo_specs
                            .iter_mut()
                            .find(|repo_spec| repo_spec.address == address)
                        {
                            repo_spec.path = path.clone();
                            if repo_spec.colour.trim().is_empty() {
                                repo_spec.colour = colour;
                            }
                        } else {
                            let source = self
                                .repo_db
                                .iter()
                                .find(|entry| entry.address == address)
                                .map(|entry| entry.source.clone())
                                .unwrap_or_default();
                            let mut spec = RepoSpec::from_address(address.clone(), source);
                            spec.path = path;
                            spec.colour = colour;
                            self.repo_specs.push(spec);
                        }
                        let to_save: Vec<RepoSpec> =
                            self.repo_specs.iter().skip(1).cloned().collect();
                        let _ = crate::backend::save_repo_specs(&to_save);
                        self.clone_success =
                            Some(format!("{}\n\nCloned to: {}", address, cloned_path));
                    }
                    Err(e) => {
                        self.clone_error = Some(e);
                    }
                },

                BackendResponse::UserStats { result } => match result {
                    Ok(count_opt) => {
                        self.dashboard_issue_count = Some(count_opt);
                    }
                    Err(e) => {
                        self.background_error = Some(format!("Issue count fetch failed: {}", e));
                    }
                },
            }
        }
    }

    // ── Key handlers ─────────────────────────────────────────────────────────

    /// Handle keyboard input when the filter modal is open.
    fn handle_filter_modal_keys(&mut self, key: KeyEvent) {
        let filter_state = match self.filter_state.as_mut() {
            Some(filter_state) => filter_state,
            None => return,
        };
        match key.code {
            KeyCode::Char(typed_char) => {
                if typed_char == ' ' && filter_state.stage == FilterStage::Repo {
                    if filter_state.repo_cursor < filter_state.repo_toggles.len() {
                        let cursor_idx = filter_state.repo_cursor;
                        filter_state.repo_toggles[cursor_idx] =
                            !filter_state.repo_toggles[cursor_idx];
                    }
                } else {
                    match filter_state.stage {
                        FilterStage::Author => filter_state.author.push(typed_char),
                        FilterStage::Tag => filter_state.tag.push(typed_char),
                        // Status and Kind are pure dropdowns — no free-text input
                        FilterStage::Status | FilterStage::Kind => {}
                        FilterStage::Search => filter_state.query.push(typed_char),
                        FilterStage::Repo => {}
                    }
                }
            }
            KeyCode::Backspace => match filter_state.stage {
                FilterStage::Author => {
                    filter_state.author.pop();
                }
                FilterStage::Tag => {
                    filter_state.tag.pop();
                }
                // Status and Kind are pure dropdowns — no free-text to delete
                FilterStage::Status | FilterStage::Kind => {}
                FilterStage::Search => {
                    filter_state.query.pop();
                }
                FilterStage::Repo => {}
            },
            KeyCode::Up => {
                if filter_state.stage == FilterStage::Author
                    && !filter_state.author_items.is_empty()
                {
                    let item_count = filter_state.author_items.len();
                    let pos = filter_state.author_selected.min(item_count - 1);
                    let new_pos = if pos == 0 { item_count - 1 } else { pos - 1 };
                    filter_state.author_selected = new_pos;
                    filter_state.author = filter_state.author_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Tag
                    && !filter_state.tag_items.is_empty()
                {
                    let item_count = filter_state.tag_items.len();
                    let pos = filter_state.tag_selected.min(item_count - 1);
                    let new_pos = if pos == 0 { item_count - 1 } else { pos - 1 };
                    filter_state.tag_selected = new_pos;
                    filter_state.tag = filter_state.tag_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Status {
                    let item_count = filter_state.status_items.len();
                    let pos = filter_state.status_selected.min(item_count - 1);
                    let new_pos = if pos == 0 { item_count - 1 } else { pos - 1 };
                    filter_state.status_selected = new_pos;
                    filter_state.status = filter_state.status_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Kind {
                    let item_count = filter_state.kind_items.len();
                    let pos = filter_state.kind_selected.min(item_count - 1);
                    let new_pos = if pos == 0 { item_count - 1 } else { pos - 1 };
                    filter_state.kind_selected = new_pos;
                    filter_state.kind = filter_state.kind_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Repo
                    && !filter_state.repo_items.is_empty()
                {
                    if filter_state.repo_cursor > 0 {
                        filter_state.repo_cursor -= 1;
                    }
                }
            }
            KeyCode::Down => {
                if filter_state.stage == FilterStage::Author
                    && !filter_state.author_items.is_empty()
                {
                    let item_count = filter_state.author_items.len();
                    let pos = filter_state.author_selected.min(item_count - 1);
                    let new_pos = (pos + 1) % item_count;
                    filter_state.author_selected = new_pos;
                    filter_state.author = filter_state.author_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Tag
                    && !filter_state.tag_items.is_empty()
                {
                    let item_count = filter_state.tag_items.len();
                    let pos = filter_state.tag_selected.min(item_count - 1);
                    let new_pos = (pos + 1) % item_count;
                    filter_state.tag_selected = new_pos;
                    filter_state.tag = filter_state.tag_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Status {
                    let item_count = filter_state.status_items.len();
                    let pos = filter_state.status_selected.min(item_count - 1);
                    let new_pos = (pos + 1) % item_count;
                    filter_state.status_selected = new_pos;
                    filter_state.status = filter_state.status_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Kind {
                    let item_count = filter_state.kind_items.len();
                    let pos = filter_state.kind_selected.min(item_count - 1);
                    let new_pos = (pos + 1) % item_count;
                    filter_state.kind_selected = new_pos;
                    filter_state.kind = filter_state.kind_items[new_pos].clone();
                } else if filter_state.stage == FilterStage::Repo
                    && !filter_state.repo_items.is_empty()
                {
                    if filter_state.repo_cursor + 1 < filter_state.repo_items.len() {
                        filter_state.repo_cursor += 1;
                    }
                }
            }
            KeyCode::Tab => {
                filter_state.stage = filter_state.stage.next(!filter_state.repo_items.is_empty());
            }
            KeyCode::Enter => {
                // Collect all values from `filter_state` into locals before releasing the borrow.
                let new_author = if filter_state.author.trim().is_empty() {
                    None
                } else {
                    Some(filter_state.author.trim().to_string())
                };
                let new_tag = if filter_state.tag.trim().is_empty() {
                    None
                } else {
                    Some(filter_state.tag.trim().to_string())
                };
                let status_text = filter_state.status.trim().to_string();
                let new_status = if status_text.is_empty() {
                    None
                } else {
                    Some(status_text)
                };
                let new_search = if filter_state.query.trim().is_empty() {
                    None
                } else {
                    Some(filter_state.query.trim().to_string())
                };
                let kind_text = filter_state.kind.trim().to_string();
                let new_kind = if kind_text.is_empty() {
                    None
                } else {
                    Some(kind_text)
                };
                let new_repo: Vec<String> = filter_state
                    .repo_items
                    .iter()
                    .zip(filter_state.repo_toggles.iter())
                    .filter(|(_, tog)| **tog)
                    .map(|(name, _)| name.clone())
                    .collect();
                // `filter_state` borrow ends here (NLL); safe to write back to self.
                self.filters.author = new_author;
                self.filters.tag = new_tag;
                self.filters.status = new_status;
                self.filters.search = new_search;
                self.filters.kind = new_kind;
                self.filters.repo = new_repo;
                self.filter_state = None;
                self.selected = 0;
                // If the status filter changed, the cached issue set no longer matches
                // what the API returned — trigger a fresh fetch.
                if self.filters.status != self.fetched_state && self.issues.is_some() {
                    self.fetch_selected_repo();
                }
            }
            KeyCode::Esc => {
                self.filter_state = None;
            }
            _ => {}
        }
    }

    /// Handle keyboard input when the tag picker is open.
    fn handle_tag_picker_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.tag_picker_selected > 0 {
                    self.tag_picker_selected = self.tag_picker_selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if !self.tag_picker_items.is_empty() {
                    self.tag_picker_selected = (self.tag_picker_selected + 1)
                        .min(self.tag_picker_items.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                if let Some(tag) = self.tag_picker_items.get(self.tag_picker_selected) {
                    if tag.eq_ignore_ascii_case("Remove filter") {
                        self.filters.tag = None;
                    } else {
                        self.filters.tag = Some(tag.clone());
                    }
                    self.tag_picker_open = false;
                    self.selected = 0;
                }
            }
            KeyCode::Esc => {
                self.tag_picker_open = false;
            }
            _ => {}
        }
    }

    /// Handle all main (non-modal) keyboard input.
    /// Returns `Ok(true)` when the user requests quit.
    fn handle_main_keys(&mut self, key: KeyEvent) -> io::Result<bool> {
        match key.code {
            KeyCode::Char('q') => return Ok(true),

            KeyCode::Char('?') => self.help_open = true,

            KeyCode::Char('r') => {
                if !self.repo_specs.is_empty() {
                    self.fetch_selected_repo();
                }
            }

            KeyCode::Char('f') => {
                self.filter_state = Some(crate::filter::make_filter_state(
                    &self.filters.author,
                    &self.filters.tag,
                    &self.filters.status,
                    &self.filters.search,
                    &self.filters.kind,
                    self.issues.as_ref(),
                    self.details.comments.as_ref(),
                    &self.repo_specs,
                    &self.filters.repo,
                ));
            }

            KeyCode::Char('F') => {
                self.filters.author = None;
                self.filters.tag = None;
                self.filters.status = Some("open".to_string());
                self.filters.search = None;
                self.filters.kind = None;
                self.filters.repo = Vec::new();
                self.selected = 0;
            }

            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Repos => Focus::Issues,
                    Focus::Issues => Focus::Repos,
                };
            }

            KeyCode::Char('b') => {
                self.view = View::List;
            }

            KeyCode::Char('p') | KeyCode::Char('c') => {
                self.handle_repo_panel_keys(key);
            }

            KeyCode::Enter => match self.focus {
                Focus::Repos => {
                    if !self.repo_specs.is_empty() {
                        match &self.panel_cursor.clone() {
                            PanelCursor::SourceHeader { source } => {
                                let key = format!("__source__:{}", source);
                                let exp = self.org_expanded.entry(key).or_insert(true);
                                *exp = !*exp;
                            }
                            PanelCursor::OrgHeader { org, source } => {
                                let key = format!("{}:{}", source, org);
                                let exp = self.org_expanded.entry(key).or_insert(true);
                                *exp = !*exp;
                            }
                            PanelCursor::LinkedHeader { source } => {
                                let key = format!("__linked__:{}", source);
                                let exp = self.org_expanded.entry(key).or_insert(false);
                                *exp = !*exp;
                            }
                            PanelCursor::Dashboard => {
                                self.view = View::Dashboard;
                            }
                            _ => self.fetch_selected_repo(),
                        }
                    }
                }
                Focus::Issues => {
                    if self.issues.is_some() {
                        self.view = View::Details;
                        self.open_details_for_selected();
                    }
                }
            },

            // ── Navigation: routed by active view ──────────────────────────
            // Up/Down also work in Dashboard so the panel can be navigated
            // before selecting a repo (matches the original else-branch behaviour).
            KeyCode::Up | KeyCode::Down => match self.view {
                View::Details => self.handle_details_keys(key),
                View::List | View::Dashboard => self.handle_list_keys(key),
            },
            // PageUp/PageDown are a no-op in Dashboard (original behaviour).
            KeyCode::PageUp | KeyCode::PageDown => match self.view {
                View::Details => self.handle_details_keys(key),
                View::List => self.handle_list_keys(key),
                View::Dashboard => {}
            },

            // ── Details-only keys ───────────────────────────────────────────
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('G')
            | KeyCode::Char('g')
            | KeyCode::Home
            | KeyCode::Char('o') => {
                if self.view == View::Details {
                    self.handle_details_keys(key);
                }
            }

            _ => {}
        }
        Ok(false)
    }

    /// Handle navigation keys when the details view is active.
    fn handle_details_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                self.details.scroll = self.details.scroll.saturating_sub(1);
            }
            KeyCode::Down => {
                let max = self.details_max_scroll();
                self.details.scroll = (self.details.scroll + 1).min(max);
                if self.details.has_more_pages && !self.details.loading {
                    let content = self.content_lines;
                    let visible = self.visible_lines.max(1);
                    if self.details.scroll + visible + 3 >= content {
                        self.load_next_comment_page();
                    }
                }
            }
            KeyCode::PageUp => {
                self.details.scroll = self.details.scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                let max = self.details_max_scroll();
                self.details.scroll = (self.details.scroll + 10).min(max);
                if self.details.has_more_pages && !self.details.loading {
                    let content = self.content_lines;
                    let visible = self.visible_lines.max(1);
                    if self.details.scroll + visible + 3 >= content {
                        self.load_next_comment_page();
                    }
                }
            }
            KeyCode::Left => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.open_details_for_selected();
                }
            }
            KeyCode::Right => {
                let max = self.filtered_issues().len();
                if self.selected + 1 < max {
                    self.selected += 1;
                    self.open_details_for_selected();
                }
            }
            KeyCode::Char('G') => {
                self.details.scroll = self.details_max_scroll();
                if self.details.has_more_pages && !self.details.loading {
                    self.details.load_all = true;
                    self.load_next_comment_page();
                }
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.details.scroll = 0;
            }
            KeyCode::Char('o') => {
                let filtered = self.filtered_issues();
                if let Some(issue) = filtered.get(self.selected) {
                    let url = if !issue.html_url.is_empty() {
                        issue.html_url.clone()
                    } else {
                        issue.url.clone()
                    };
                    open_in_browser(&url);
                }
            }
            _ => {}
        }
    }

    /// Handle navigation keys when the issue list is active.
    fn handle_list_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => match self.focus {
                Focus::Repos => {
                    let order = self.panel_nav_order();
                    if let Some(pos) = order.iter().position(|cursor| cursor == &self.panel_cursor)
                    {
                        if pos > 0 {
                            self.panel_cursor = order[pos - 1].clone();
                        }
                    }
                }
                Focus::Issues => {
                    if self.selected > 0 {
                        self.selected -= 1;
                    }
                }
            },
            KeyCode::Down => match self.focus {
                Focus::Repos => {
                    let order = self.panel_nav_order();
                    if let Some(pos) = order.iter().position(|cursor| cursor == &self.panel_cursor)
                    {
                        if pos + 1 < order.len() {
                            self.panel_cursor = order[pos + 1].clone();
                        }
                    }
                }
                Focus::Issues => {
                    let max = self.filtered_issues().len();
                    if max > 0 {
                        self.selected = (self.selected + 1).min(max - 1);
                    }
                }
            },
            KeyCode::PageUp => match self.focus {
                Focus::Issues => {
                    self.selected = self.selected.saturating_sub(10);
                }
                Focus::Repos => {
                    let order = self.panel_nav_order();
                    if let Some(pos) = order.iter().position(|cursor| cursor == &self.panel_cursor)
                    {
                        self.panel_cursor = order[pos.saturating_sub(10)].clone();
                    }
                }
            },
            KeyCode::PageDown => match self.focus {
                Focus::Issues => {
                    let max = self.filtered_issues().len();
                    if max > 0 {
                        self.selected = (self.selected + 10).min(max - 1);
                    }
                }
                Focus::Repos => {
                    let order = self.panel_nav_order();
                    if let Some(pos) = order.iter().position(|cursor| cursor == &self.panel_cursor)
                    {
                        let new_pos = (pos + 10).min(order.len().saturating_sub(1));
                        self.panel_cursor = order[new_pos].clone();
                    }
                }
            },
            _ => {}
        }
    }

    /// Handle keys that apply when the repo panel has focus (`p` and `c`).
    fn handle_repo_panel_keys(&mut self, key: KeyEvent) {
        if self.focus != Focus::Repos {
            return;
        }
        match key.code {
            KeyCode::Char('p') => {
                if let PanelCursor::Repo(i) = self.panel_cursor {
                    if i > 0 {
                        if let Some(spec) = self.repo_specs.get_mut(i) {
                            spec.pinned = !spec.pinned;
                        } else {
                            return;
                        }
                        let to_save: Vec<RepoSpec> =
                            self.repo_specs.iter().skip(1).cloned().collect();
                        let _ = crate::backend::save_repo_specs(&to_save);
                    }
                }
            }
            KeyCode::Char('c') => {
                let target = match &self.panel_cursor {
                    PanelCursor::Repo(i) => self.repo_specs.get(*i).and_then(|spec| {
                        if spec.name != ALL_CONFIGURED && spec.path.is_empty() {
                            Some((spec.address.clone(), spec.source.clone()))
                        } else {
                            None
                        }
                    }),
                    PanelCursor::DiscoveredRepo { address, source } => {
                        Some((address.clone(), source.clone()))
                    }
                    PanelCursor::LinkedRepo { address, source } => {
                        Some((address.clone(), source.clone()))
                    }
                    PanelCursor::OrgHeader { .. }
                    | PanelCursor::OrgAll { .. }
                    | PanelCursor::SourceHeader { .. }
                    | PanelCursor::LinkedHeader { .. }
                    | PanelCursor::Dashboard => None,
                };
                if let Some((address, source_name)) = target {
                    self.status_message = Some(format!("Cloning {}\u{2026}", address));
                    let _ = self.backend_tx.send(BackendRequest::CloneRepo {
                        address,
                        source_name,
                        clone_root: self.clone_root.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Maximum valid scroll offset for the details view.
    fn details_max_scroll(&self) -> usize {
        self.content_lines.saturating_sub(self.visible_lines.max(1))
    }

    /// Compute the panel navigation order from current state.
    fn panel_nav_order(&self) -> Vec<PanelCursor> {
        nav_order(
            &self.repo_specs,
            PanelMode::ByOrg,
            &self.repo_db,
            &self.org_expanded,
            &self.user_orgs,
            self.current_user.as_deref(),
            &self.linked_repos,
            self.show_dashboard,
        )
    }

    /// Apply all active filters and return references to the matching issues.
    fn filtered_issues(&self) -> Vec<&crate::model::Issue> {
        crate::filter::filtered_issues(
            self.issues.as_ref(),
            &self.filters.status,
            &self.filters.author,
            &self.filters.tag,
            &self.filters.search,
            &self.filters.kind,
            &self.filters.repo,
        )
    }

    /// Reset all details state and fetch page 1 of comments for `self.selected`.
    /// Called on Enter (from list), Left, and Right (within details).
    fn open_details_for_selected(&mut self) {
        self.details = DetailsState::default();

        let issue_info = self.filtered_issues().get(self.selected).map(|issue| {
            (
                issue.comments,
                issue.comments_url.clone(),
                issue.number,
                issue.repo_name.clone(),
            )
        });

        if let Some((comments_count, comments_url, issue_number, repo_name)) = issue_info {
            if comments_count > 0 && !comments_url.is_empty() {
                let source_name = if !repo_name.is_empty() {
                    self.repo_specs
                        .iter()
                        .find(|repo_spec| {
                            repo_spec.name == repo_name || repo_spec.address == repo_name
                        })
                        .map(|repo_spec| repo_spec.source.clone())
                        .unwrap_or_default()
                } else {
                    self.repo_specs
                        .get(match &self.panel_cursor {
                            PanelCursor::Repo(idx) => *idx,
                            _ => 0,
                        })
                        .map(|repo_spec| repo_spec.source.clone())
                        .unwrap_or_default()
                };
                self.details.comments_url = comments_url.clone();
                self.details.issue_number = issue_number;
                self.details.source_name = source_name.clone();
                let _ = self.backend_tx.send(BackendRequest::FetchIssueDetails {
                    comments_url,
                    issue_number,
                    source_name,
                    page: 1,
                    per_page: self.comments_per_page,
                });
                self.details.loading = true;
            }
        }
    }

    /// Send a request for the next comment page.
    fn load_next_comment_page(&mut self) {
        if self.details.comments_url.is_empty() || self.details.loading {
            return;
        }
        self.details.comment_page += 1;
        let _ = self.backend_tx.send(BackendRequest::FetchIssueDetails {
            comments_url: self.details.comments_url.clone(),
            issue_number: self.details.issue_number,
            source_name: self.details.source_name.clone(),
            page: self.details.comment_page,
            per_page: self.comments_per_page,
        });
        self.details.loading = true;
    }

    // ── Drawing ──────────────────────────────────────────────────────────────

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Fill background
        let background =
            Paragraph::new(Text::from(Span::raw(" "))).style(Style::default().bg(Color::Black));
        frame.render_widget(background, area);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(15), Constraint::Percentage(85)])
            .split(area);

        // Left: repo panel — build items once, derive scroll offset, then render
        let panel_items = build_panel_items(
            &self.repo_specs,
            PanelMode::ByOrg,
            &self.repo_db,
            &self.org_expanded,
            &self.user_orgs,
            self.current_user.as_deref(),
            &self.linked_repos,
            self.show_dashboard,
        );

        crate::tui::repo_panel::render_repo_panel(
            frame,
            chunks[0],
            crate::tui::repo_panel::RepoPanelCtx {
                items: &panel_items,
                repo_specs: &self.repo_specs,
                panel_cursor: &self.panel_cursor,
                active_cursor: self.active_cursor.as_ref(),
                focus_repos: self.focus == Focus::Repos,
                gitea_source_names: &self.gitea_source_names,
                user_orgs: &self.user_orgs,
                current_user: self.current_user.as_deref(),
                linked_repos: &self.linked_repos,
            },
        );

        // Right: issue list or details
        let is_all_view = matches!(
            self.issues.as_ref().map(|issue_list| &issue_list.meta_data),
            Some(IssueSource::All)
        );
        let filtered = self.filtered_issues();

        let mut filters_summary: Vec<String> = Vec::new();
        if let Some(a) = &self.filters.author {
            filters_summary.push(format!("author: {}", a));
        }
        if let Some(t) = &self.filters.tag {
            filters_summary.push(format!("tag: {}", t));
        }
        if let Some(s) = &self.filters.status {
            filters_summary.push(format!("status: {}", s));
        }
        if let Some(k) = &self.filters.kind {
            filters_summary.push(format!("kind: {}", k));
        }
        if let Some(q) = &self.filters.search {
            filters_summary.push(format!("search: {}", q));
        }
        if !self.filters.repo.is_empty() {
            if self.filters.repo.len() == 1 {
                filters_summary.push(format!("repo: {}", self.filters.repo[0]));
            } else {
                filters_summary.push(format!("repo: {} selected", self.filters.repo.len()));
            }
        }

        match self.view {
            View::List => {
                crate::tui::issue_list::render_issue_list(
                    frame,
                    chunks[1],
                    crate::tui::issue_list::IssueListCtx {
                        loading: self.loading,
                        error: self.error.as_deref(),
                        issues: self.issues.as_ref(),
                        filtered: &filtered,
                        is_all_view,
                        repo_specs: &self.repo_specs,
                        selected: self.selected,
                        filters_summary,
                    },
                );
            }
            View::Details => {
                let (cl, vl) = crate::tui::details::render_issue_details(
                    frame,
                    chunks[1],
                    crate::tui::details::IssueDetailsCtx {
                        filtered: &filtered,
                        selected: self.selected,
                        details_comments: self.details.comments.as_ref(),
                        details_loading_comments: self.details.loading,
                        details_error: self.details.error.as_deref(),
                        details_scroll: self.details.scroll,
                        details_has_more_pages: self.details.has_more_pages,
                    },
                );
                self.content_lines = cl;
                self.visible_lines = vl;
            }
            View::Dashboard => {
                crate::tui::dashboard::render_dashboard(
                    frame,
                    chunks[1],
                    crate::tui::dashboard::DashboardCtx {
                        org_count: self.user_orgs.len(),
                        orgs_loading: self.orgs_pending > 0,
                        collab_repo_count: self.linked_repos.len(),
                        issue_count: self.dashboard_issue_count,
                        username: self.current_user.as_deref(),
                    },
                );
            }
        }

        // ── Loading overlay ───────────────────────────────────────────────────
        if self.loading {
            let label = match &self.panel_cursor {
                PanelCursor::Repo(0) => "All (configured)".to_string(),
                PanelCursor::Repo(i) => self
                    .repo_specs
                    .get(*i)
                    .map(|spec_ref| spec_ref.name.clone())
                    .unwrap_or_default(),
                PanelCursor::OrgAll { org, .. } => format!("All ({})", org),
                PanelCursor::DiscoveredRepo { address, .. } => address.clone(),
                PanelCursor::LinkedRepo { address, .. } => address.clone(),
                _ => String::new(),
            };
            let progress =
                self.loading_progress
                    .as_ref()
                    .map(|(fetched_count, total_count, repo_label)| {
                        (*fetched_count, *total_count, repo_label.as_str())
                    });
            crate::tui::modal::render_loading_overlay(frame, chunks[1], &label, progress);
        }

        // Filter modal overlay
        if let Some(filter_modal_state) = &self.filter_state {
            crate::tui::modal::render_filter_modal(
                frame,
                area,
                crate::tui::modal::FilterModalCtx {
                    filter_state: filter_modal_state,
                    tag_picker_open: self.tag_picker_open,
                    tag_picker_items: &self.tag_picker_items,
                    tag_picker_selected: self.tag_picker_selected,
                },
            );
        }

        // ── Notification modals (suppressed when help is open) ────────────────
        if !self.help_open {
            if let Some(err) = &self.background_error {
                crate::tui::modal::render_error_modal(frame, area, "Background Error", err);
            }
            if let Some(err) = &self.clone_error {
                crate::tui::modal::render_error_modal(frame, area, "Clone error", err);
            }
            if let Some(msg) = &self.clone_success {
                crate::tui::modal::render_success_modal(frame, area, "Cloned successfully", msg);
            }
        }

        // Help modal — rendered last so it is always the topmost overlay.
        if self.help_open {
            crate::tui::modal::render_help_modal(frame, area);
        }

        // Status bar
        {
            let status_rect = Rect {
                x: area.x,
                y: area.y + area.height.saturating_sub(1),
                width: area.width,
                height: 1,
            };
            let (text, style) = if let Some(msg) = &self.status_message {
                (
                    msg.clone(),
                    Style::default().bg(Color::Black).fg(Color::White),
                )
            } else {
                (
                    "?  help".to_string(),
                    Style::default().bg(Color::Black).fg(Color::DarkGray),
                )
            };
            frame.render_widget(Paragraph::new(Span::raw(text)).style(style), status_rect);
        }
    }
}
