//! Background worker thread and message passing.
//!
//! This module manages concurrency by dispatching work to a background worker thread.
//! The main app sends requests to the worker and receives responses asynchronously.
//!
//! The worker uses a semaphore to limit concurrent threads, ensuring that large
//! fetches (e.g., multi-repo issues) don't overwhelm the system while still allowing
//! parallelism for fast operations like cloning.

use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::model::{Comment, IssueList, RepoDbEntry, RepoSpec};

/// A request sent to the background worker thread.
///
/// The main TUI thread sends these requests to work off the main thread.
/// Each variant dispatches to a different backend function.
///
/// # Variants
///
/// - `FetchIssues` — Fetch issues from a single repo
/// - `FetchIssuesMany` — Fetch issues from multiple repos (aggregated view)
/// - `FetchIssueDetails` — Fetch a page of comments for an issue
/// - `FetchOrgRepos` — Discover all repos in an organization
/// - `FetchUserOrgs` — List organizations the user belongs to
/// - `CloneRepo` — Clone a repo to the local filesystem
/// - `FetchCollaboratorRepos` — Fetch repos where user is a collaborator
/// - `FetchUserStats` — Fetch user's involvement count
pub enum BackendRequest {
    FetchIssues {
        repo_spec: RepoSpec,
        creator: Option<String>,
        state_filter: Option<String>,
    },
    FetchIssuesMany {
        repo_specs: Vec<RepoSpec>,
        state_filter: Option<String>,
    },
    FetchIssueDetails {
        comments_url: String,
        issue_number: u32,
        source_name: String,
        page: usize,
        per_page: usize,
    },
    /// Discover all repos that belong to `org` on the given source.
    FetchOrgRepos { org: String, source_name: String },
    /// Discover all organisations the authenticated user is a member of on the given source.
    FetchUserOrgs { source_name: String },
    /// Clone a repo to the local filesystem.
    /// Returns the absolute destination path and a generated colour on success.
    CloneRepo {
        address: String,
        source_name: String,
        clone_root: String,
    },
    /// Fetch repos the user is a collaborator on (not owned by user/their orgs).
    FetchCollaboratorRepos { source_name: String },
    /// Fetch dashboard stats (currently: total issues the user is involved in).
    FetchUserStats {
        source_name: String,
        username: String,
    },
}

/// A response sent back from the background worker thread.
///
/// The main TUI thread receives these in its event loop to update UI state.
/// Some requests may generate multiple responses (e.g., progress updates)
/// followed by a final result.
///
/// # Variants
///
/// - `Issues(Result<IssueList, String>)` — Fetch result (or error message)
/// - `IssueDetails(Result<(issue_num, comments, cached, has_more), String>)` — Comment page
/// - `OrgRepos { org, result }` — Organization discovery result
/// - `UserOrgs { source_name, result }` — List of org names
/// - `Cloned { address, result }` — Clone result with (path, color)
/// - `CollaboratorRepos { source_name, result }` — Collaborator repos
/// - `UserStats { result }` — User involvement count (or None if unsupported)
/// - `IssueProgress { fetched, total, current_repo }` — Progress update (sent during multi-repo fetch)
pub enum BackendResponse {
    Issues(Result<IssueList, String>),
    IssueDetails(Result<(u32, Vec<Comment>, bool, bool), String>),
    /// Result of an org-repo-discovery call.
    OrgRepos {
        org: String,
        result: Result<Vec<RepoDbEntry>, String>,
    },
    /// Result of a user-org-discovery call: the list of org names for the given source.
    UserOrgs {
        source_name: String,
        result: Result<Vec<String>, String>,
    },
    /// Result of a clone operation: `(absolute_path, colour)` on success.
    Cloned {
        address: String,
        result: Result<(String, String), String>,
    },
    /// Result of a collaborator-repo fetch.
    CollaboratorRepos {
        source_name: String,
        result: Result<Vec<RepoDbEntry>, String>,
    },
    /// Result of a user-stats fetch: total issues the authenticated user is involved in.
    /// `None` means the platform does not support a single-call total (e.g. GitLab).
    UserStats {
        result: Result<Option<u64>, String>,
    },
    /// Incremental progress during a multi-repo issue fetch.
    /// Sent once after each repo completes; the final `Issues` response follows when all are done.
    IssueProgress {
        fetched: usize,
        total: usize,
        current_repo: String,
    },
}

// ── Error formatting ────────────────────────────────────────────────────────────

/// Formats an error with its full cause chain.
///
/// Unlike `e.to_string()`, which only prints the outermost message,
/// this walks the error source chain to include all underlying causes.
/// For example: instead of "HTTP error", returns "HTTP error: connection refused: Network unreachable".
///
/// # Arguments
///
/// * `e` - The error to format
///
/// # Returns
///
/// A complete error message including all causes.
fn format_error(e: &dyn std::error::Error) -> String {
    use std::fmt::Write;
    let mut msg = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        let _ = write!(msg, ": {}", s);
        source = s.source();
    }
    msg
}

// ── Semaphore ─────────────────────────────────────────────────────────────────

/// A simple counting semaphore for bounding concurrent workers.
///
/// Limits the number of concurrently running threads to a configured maximum.
/// Excess requests queue in the mpsc channel until a slot becomes free.
struct Semaphore {
    max: usize,
    active: Mutex<usize>,
    cvar: Condvar,
}

impl Semaphore {
    /// Creates a new semaphore with a maximum concurrent count.
    ///
    /// # Arguments
    ///
    /// * `max` - Maximum number of concurrent acquirers (will be at least 1)
    fn new(max: usize) -> Self {
        // Guard against a zero limit which would deadlock immediately.
        let max = max.max(1);
        Self {
            max,
            active: Mutex::new(0),
            cvar: Condvar::new(),
        }
    }

    /// Blocks until a slot is available, then takes it.
    ///
    /// Increments the active counter. If already at max, waits on the condition variable.
    fn acquire(&self) {
        let mut active = self.active.lock().unwrap();
        while *active >= self.max {
            active = self.cvar.wait(active).unwrap();
        }
        *active += 1;
    }

    /// Releases a slot and wakes a waiting acquirer.
    ///
    /// Decrements the active counter and notifies one waiting thread.
    fn release(&self) {
        let mut active = self.active.lock().unwrap();
        *active -= 1;
        self.cvar.notify_one();
    }
}

// ── Request dispatch ──────────────────────────────────────────────────────────

/// Executes a single backend request and sends responses.
///
/// Dispatches a BackendRequest to the appropriate backend function,
/// formats errors, and sends response(s) back on the channel.
/// Some requests may generate multiple responses (progress updates).
///
/// # Arguments
///
/// * `req` - The request to handle
/// * `tx` - Channel to send response(s) back on
fn handle_request(req: BackendRequest, tx: &mpsc::Sender<BackendResponse>) {
    match req {
        BackendRequest::FetchIssues {
            repo_spec,
            creator,
            state_filter,
        } => {
            let tx_progress = tx.clone();
            let res = crate::backend::fetch_issues(
                &repo_spec,
                creator.as_deref(),
                state_filter.as_deref(),
                |issues_so_far, page| {
                    let _ = tx_progress.send(BackendResponse::IssueProgress {
                        fetched: issues_so_far,
                        total: 0, // 0 = indeterminate (single repo, total pages unknown)
                        current_repo: format!("page {}", page),
                    });
                },
            )
            .map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::Issues(res));
        }
        BackendRequest::FetchIssuesMany {
            repo_specs,
            state_filter,
        } => {
            let tx_progress = tx.clone();
            let res = crate::backend::fetch_issues_many(
                &repo_specs,
                state_filter.as_deref(),
                |fetched, total, current_repo| {
                    let _ = tx_progress.send(BackendResponse::IssueProgress {
                        fetched,
                        total,
                        current_repo: current_repo.to_string(),
                    });
                },
            )
            .map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::Issues(res));
        }
        BackendRequest::FetchIssueDetails {
            comments_url,
            issue_number,
            source_name,
            page,
            per_page,
        } => {
            let res = crate::backend::fetch_comments(&comments_url, &source_name, page, per_page)
                .map(|(c, cached, has_more)| (issue_number, c, cached, has_more))
                .map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::IssueDetails(res));
        }
        BackendRequest::FetchOrgRepos { org, source_name } => {
            let result = crate::backend::fetch_org_repos(&org, &source_name)
                .map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::OrgRepos { org, result });
        }
        BackendRequest::FetchUserOrgs { source_name } => {
            let result =
                crate::backend::fetch_user_orgs(&source_name).map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::UserOrgs {
                source_name,
                result,
            });
        }
        BackendRequest::CloneRepo {
            address,
            source_name,
            clone_root,
        } => {
            let result = crate::backend::clone_repo(&address, &source_name, &clone_root)
                .map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::Cloned { address, result });
        }
        BackendRequest::FetchCollaboratorRepos { source_name } => {
            let result = crate::backend::fetch_collaborator_repos(&source_name)
                .map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::CollaboratorRepos {
                source_name,
                result,
            });
        }
        BackendRequest::FetchUserStats {
            source_name,
            username,
        } => {
            let result = crate::backend::fetch_user_issue_count(&source_name, &username)
                .map_err(|e| format_error(e.as_ref()));
            let _ = tx.send(BackendResponse::UserStats { result });
        }
    }
}

// ── Worker loop ───────────────────────────────────────────────────────────────

/// The background worker event loop.
///
/// Runs in a dedicated thread and processes requests from the main TUI thread.
/// Design:
/// - Each request is dispatched to its own `thread::spawn`'d thread
/// - A semaphore limits concurrent threads to `max_threads`
/// - Slow fetches (e.g., multi-page issues) don't block faster requests
/// - Excess requests queue in the mpsc channel
///
/// # Arguments
///
/// * `rx` - Channel to receive requests from the main thread
/// * `tx` - Channel to send responses back to the main thread
/// * `max_threads` - Maximum concurrent worker threads (from config, usually 4)
///
/// # Panics
///
/// Never returns (runs until the receiver is closed).
pub fn worker(
    rx: mpsc::Receiver<BackendRequest>,
    tx: mpsc::Sender<BackendResponse>,
    max_threads: usize,
) {
    let sem = Arc::new(Semaphore::new(max_threads));

    while let Ok(req) = rx.recv() {
        let thread_tx = tx.clone();
        let thread_sem = Arc::clone(&sem);

        // Block here (on the receiver thread) until a worker slot is free.
        // Pending requests remain buffered in the mpsc channel.
        thread_sem.acquire();

        thread::spawn(move || {
            handle_request(req, &thread_tx);
            thread_sem.release();
        });
    }
}
