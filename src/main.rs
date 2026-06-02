//! New thin entry point — to be renamed to `main.rs` once the old files are removed.

mod app;
mod backend;
mod cli;
mod filter;
mod model;
mod tui;
mod worker;

use std::io;

use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use serde::Serialize;

use crate::cli::Cli;

/// Returns true if `address` (e.g. `"owner/repo"`) matches the user-supplied
/// `name`, which may be a full address or just the repo-name part, compared
/// case-insensitively.
fn db_entry_matches(address: &str, name: &str) -> bool {
    let lower_addr = address.to_lowercase();
    let lower_name = name.to_lowercase();
    let repo_part = lower_addr.split('/').last().unwrap_or("");
    lower_addr == lower_name || repo_part == lower_name
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    // Validate --repo before entering TUI mode so errors print cleanly to stderr.
    if let Some(ref name) = cli.repo {
        let specs = crate::backend::load_repo_specs().unwrap_or_default();
        let repo_db = crate::backend::load_repo_db().unwrap_or_default();

        let in_specs = specs
            .iter()
            .any(|repo_spec| &repo_spec.name == name || &repo_spec.address == name);
        let in_db = repo_db
            .entries
            .iter()
            .any(|entry| db_entry_matches(&entry.address, name));

        if !in_specs && !in_db {
            let lower = name.to_lowercase();
            let mut suggestions: Vec<String> = specs
                .iter()
                .filter(|repo_spec| {
                    repo_spec.name.to_lowercase().contains(&lower)
                        || repo_spec.address.to_lowercase().contains(&lower)
                })
                .map(|repo_spec| repo_spec.name.clone())
                .collect();

            // Also suggest from org-discovered repos.
            let db_suggestions: Vec<String> = repo_db
                .entries
                .iter()
                .filter(|entry| entry.address.to_lowercase().contains(&lower))
                .map(|entry| entry.address.clone())
                .collect();
            suggestions.extend(db_suggestions);

            eprintln!("error: no configured or discovered repo matches '{name}'.");
            if !suggestions.is_empty() {
                eprintln!("  Did you mean: {}", suggestions.join(", "));
            } else {
                let all: Vec<&str> = specs
                    .iter()
                    .map(|repo_spec| repo_spec.name.as_str())
                    .collect();
                if all.is_empty() {
                    eprintln!("  No repos are configured.");
                } else {
                    eprintln!("  Available repos: {}", all.join(", "));
                }
            }

            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("repo '{name}' not found"),
            ));
        }
    }

    // --json: print issue (+ optional comments) to stdout and exit, no TUI.
    if cli.json {
        if cli.repo.is_none() || cli.issue.is_empty() {
            eprintln!("error: --json requires both --repo and --issue");
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--json requires --repo and --issue",
            ));
        }
        return run_json_output(cli.repo.as_deref().unwrap(), &cli.issue, cli.with_comments);
    }

    // Subcommands (e.g. `ogit clone`) are handled inside the TUI for now;
    // the parsed value is kept so --repo / --issue can be read below.

    // ── Panic hook ────────────────────────────────────────────────────────────────────
    // Restore the terminal to a sane state before printing any panic message.
    // Without this, a panic in TUI code leaves the terminal in raw mode and on
    // the alternate screen, causing subsequent shell output to be garbled.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        disable_raw_mode().ok();
        crossterm::execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture,).ok();
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend_term = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend_term)?;
    terminal.clear()?;

    let mut app = app::App::new();

    // Auto-insert the local git repo if we are inside one and it is not already listed.
    // Either way, remember the address so we can focus it on startup.
    let local_address: Option<String> =
        if let Ok(local_spec) = crate::backend::repo_spec_from_gitconfig(&app.repo_path) {
            let local_repo_addr = local_spec.address.clone();
            if !app
                .repo_specs
                .iter()
                .any(|repo_spec| repo_spec.address == local_repo_addr)
            {
                app.repo_specs.push(local_spec);
            }
            Some(local_repo_addr)
        } else {
            None
        };

    // Prepend the synthetic "All (configured)" entry so it is always index 0 in the list.
    app.repo_specs.insert(
        0,
        crate::model::RepoSpec {
            name: crate::model::ALL_CONFIGURED.to_string(),
            ..Default::default()
        },
    );

    // If we detected a local repo, point the cursor at it so the initial
    // fetch loads that repo's issues instead of ALL.
    if let Some(local_repo_addr) = local_address {
        if let Some(idx) = app
            .repo_specs
            .iter()
            .position(|repo_spec| repo_spec.address == local_repo_addr)
        {
            app.panel_cursor = crate::tui::repo_panel::PanelCursor::Repo(idx);
        }
    } else if cli.repo.is_none() {
        // Not inside a git repo and no --repo flag — show the Dashboard.
        app.activate_dashboard();
        // Kick off the issue-count fetch if we know the user's login.
        if let (Some(user), Some(source)) = (
            app.current_user.clone(),
            crate::backend::load_config()
                .ok()
                .and_then(|config_val| {
                    config_val
                        .sources
                        .into_iter()
                        .find(|source_cfg| source_cfg.enabled)
                })
                .map(|source_cfg| source_cfg.name),
        ) {
            let _ = app
                .backend_tx_for_main()
                .send(crate::worker::BackendRequest::FetchUserStats {
                    source_name: source,
                    username: user,
                });
        }
    }

    // --repo: override the cursor with the explicitly named repo.
    if let Some(ref name) = cli.repo {
        if let Some(idx) = app
            .repo_specs
            .iter()
            .position(|repo_spec| &repo_spec.name == name || &repo_spec.address == name)
        {
            app.panel_cursor = crate::tui::repo_panel::PanelCursor::Repo(idx);
        } else if let Some(entry) = app
            .repo_db
            .iter()
            .find(|entry| db_entry_matches(&entry.address, name))
        {
            app.panel_cursor = crate::tui::repo_panel::PanelCursor::DiscoveredRepo {
                address: entry.address.clone(),
                source: entry.source.clone(),
            };
        }
    }

    // --issue: store the target issue number for TUI startup; when multiple are
    // given only the first is used (the TUI shows one issue at a time).
    if let Some(&n) = cli.issue.first() {
        app.startup_issue_number = Some(n);
        app.filters.status = None;
    }

    // Auto-fetch on startup.
    app.fetch_selected_repo();

    let res = app.run(&mut terminal);

    disable_raw_mode().ok();
    crossterm::execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
    terminal.show_cursor().ok();

    res
}

// ── JSON output mode ─────────────────────────────────────────────────────────

/// Resolve `name` to a [`RepoSpec`], checking configured specs then the
/// org-discovery database. Returns an error if nothing matches.
fn find_repo_spec_for_json(
    name: &str,
    specs: &[crate::model::RepoSpec],
    db_entries: &[crate::model::RepoDbEntry],
) -> io::Result<crate::model::RepoSpec> {
    if let Some(spec) = specs
        .iter()
        .find(|repo_spec| repo_spec.name == name || repo_spec.address == name)
    {
        return Ok(spec.clone());
    }
    if let Some(entry) = db_entries
        .iter()
        .find(|entry| db_entry_matches(&entry.address, name))
    {
        return Ok(crate::model::RepoSpec::from_address(
            entry.address.clone(),
            entry.source.clone(),
        ));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("no configured or discovered repo matches '{}'", name),
    ))
}

/// Fetch the requested issue(s) (and optionally all comments) for `repo_name`
/// and print the result as JSON to stdout.
///
/// - Single issue: prints `{ "issue": …, "comments": […] }` (backward-compat)
/// - Multiple issues: prints a JSON array of the same objects
fn run_json_output(repo_name: &str, issue_numbers: &[u32], with_comments: bool) -> io::Result<()> {
    let specs = crate::backend::load_repo_specs().unwrap_or_default();
    let repo_db = crate::backend::load_repo_db().unwrap_or_default();
    let spec = find_repo_spec_for_json(repo_name, &specs, &repo_db.entries)?;

    // Fetch the full issue list once — individual lookups reuse this.
    let issue_list = crate::backend::fetch_issues(&spec, None, None, |_, _| {})
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

    #[derive(Serialize)]
    struct Output<'a> {
        issue: &'a crate::model::Issue,
        #[serde(skip_serializing_if = "Option::is_none")]
        comments: Option<Vec<crate::model::Comment>>,
    }

    let mut outputs: Vec<Output> = Vec::new();

    for &number in issue_numbers {
        let issue = issue_list
            .issue_data
            .iter()
            .find(|issue| issue.number == number)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("issue #{} not found in '{}'", number, repo_name),
                )
            })?;

        let comments: Option<Vec<crate::model::Comment>> =
            if with_comments && !issue.comments_url.is_empty() {
                const PER_PAGE: usize = 100;
                let mut all_comments = Vec::new();
                let mut page = 1usize;
                loop {
                    let (batch, _cached, has_more) = crate::backend::fetch_comments(
                        &issue.comments_url,
                        &spec.source,
                        page,
                        PER_PAGE,
                    )
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
                    all_comments.extend(batch);
                    if !has_more {
                        break;
                    }
                    page += 1;
                }
                Some(all_comments)
            } else {
                None
            };

        outputs.push(Output { issue, comments });
    }

    // Single issue → plain object (backward-compatible).
    // Multiple issues → JSON array.
    let json = if outputs.len() == 1 {
        serde_json::to_string_pretty(&outputs[0])
    } else {
        serde_json::to_string_pretty(&outputs)
    }
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

    println!("{}", json);
    Ok(())
}
