//! Issue filtering and search logic.
//!
//! This module provides:
//! - `FilterState`: Transient UI state for the filter modal
//! - `matches_filter`: Check if a single issue passes all active filters
//! - `filtered_issues`: Get all issues matching active filters
//! - `make_filter_state`: Build filter UI state from current filters and loaded issues
//!
//! All logic is pure: zero I/O and no rendering. Fully unit-testable.

use crate::model::{ALL_CONFIGURED, Comment, Issue, IssueList, IssueSource, RepoSpec};

/// Tracks which field in the filter modal currently has keyboard focus.
///
/// Used to determine which filter value to update when the user types,
/// and which field to highlight in the rendered UI.
#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub enum FilterStage {
    #[default]
    Author,
    Tag,
    Status,
    Kind,
    Search,
    /// Repo multi-select (only reachable in the ALL view).
    Repo,
}

impl FilterStage {
    /// Advances to the next filter field, cycling back to Author after the last.
    ///
    /// The Repo field only appears when `has_repo` is true, which happens in the
    /// ALL view when multiple repos are available. In single-repo views, Search
    /// cycles directly back to Author.
    ///
    /// # Arguments
    ///
    /// * `has_repo` - True if the repo panel is visible (i.e., in the ALL view)
    ///
    /// # Returns
    ///
    /// The next filter stage in the cycle.
    pub fn next(self, has_repo: bool) -> Self {
        match self {
            Self::Author => Self::Tag,
            Self::Tag => Self::Status,
            Self::Status => Self::Kind,
            Self::Kind => Self::Search,
            Self::Search => {
                if has_repo {
                    Self::Repo
                } else {
                    Self::Author
                }
            }
            Self::Repo => Self::Author,
        }
    }
}

/// UI state for the filter modal dialog.
///
/// Holds both the current filter values and the dropdown lists for each field.
/// Built fresh whenever the filter dialog is opened, and discarded when closed.
///
/// # Fields
///
/// - `stage`: Which field currently has focus
/// - `author`, `tag`, `status`, `kind`, `query`: Current filter values
/// - `author_items`, `tag_items`, etc.: Available options for dropdowns
/// - `*_selected`: Currently selected index in each dropdown
/// - `repo_items`, `repo_cursor`, `repo_toggles`: Multi-select for repos (ALL view only)
#[derive(Debug)]
pub struct FilterState {
    /// Which field currently has focus.
    pub stage: FilterStage,
    pub author: String,
    pub tag: String,
    pub status: String,
    pub kind: String,
    pub query: String,

    // author dropdown (single-select)
    pub author_items: Vec<String>,
    pub author_selected: usize,

    // tag dropdown (single-select)
    pub tag_items: Vec<String>,
    pub tag_selected: usize,

    // status dropdown (single-select, fixed options)
    pub status_items: Vec<String>,
    pub status_selected: usize,

    // kind dropdown (single-select, fixed options)
    pub kind_items: Vec<String>,
    pub kind_selected: usize,

    // repo list (multi-select, only populated in ALL view)
    pub repo_items: Vec<String>,
    pub repo_cursor: usize,
    pub repo_toggles: Vec<bool>,
}

impl Default for FilterState {
    fn default() -> Self {
        FilterState {
            stage: FilterStage::Author,
            author: String::new(),
            tag: String::new(),
            status: "open".to_string(),
            kind: String::new(),
            query: String::new(),
            author_items: Vec::new(),
            author_selected: 0,
            tag_items: Vec::new(),
            tag_selected: 0,
            status_items: vec!["".to_string(), "open".to_string(), "closed".to_string()],
            status_selected: 1,
            kind_items: vec!["".to_string(), "issue".to_string(), "pr".to_string()],
            kind_selected: 0,
            repo_items: Vec::new(),
            repo_cursor: 0,
            repo_toggles: Vec::new(),
        }
    }
}

/// Collects unique, deduplicated, and sorted names from an iterator.
///
/// Deduplication is case-insensitive, but the original case is preserved.
/// Empty names are skipped.
///
/// # Arguments
///
/// * `names` - Iterator of names to collect
///
/// # Returns
///
/// A sorted vector with unique names (case-insensitive dedup).
fn collect_unique_sorted(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen_lower: std::collections::HashSet<String> = Default::default();
    let mut out: Vec<String> = Vec::new();
    for name in names {
        let name = name.trim().to_string();
        if !name.is_empty() && seen_lower.insert(name.to_lowercase()) {
            out.push(name);
        }
    }
    out.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    out
}

/// Builds a dropdown list with an empty sentinel and finds the selected index.
///
/// Prepends an empty string as the first option (for clearing the filter),
/// then finds the index of the item that best matches the current value.
/// Matching is case-insensitive substring match.
///
/// # Arguments
///
/// * `names` - Iterator of names to include in the dropdown
/// * `current` - The current filter value (used to find the selected index)
///
/// # Returns
///
/// A tuple of (dropdown items, selected index). Defaults to index 0 if no match.
fn build_dropdown(names: impl Iterator<Item = String>, current: &str) -> (Vec<String>, usize) {
    let items: Vec<String> = std::iter::once(String::new()).chain(names).collect();
    let selected = if current.is_empty() {
        0
    } else {
        items
            .iter()
            .position(|item| {
                !item.is_empty() && item.to_lowercase().contains(&current.to_lowercase())
            })
            .unwrap_or(0)
    };
    (items, selected)
}

/// Builds a fresh FilterState pre-populated from current filters and loaded issues.
///
/// Creates a complete FilterState ready to render and interact with. Includes:
/// - Current filter values
/// - Dropdown lists for Author and Tag (built from loaded issues)
/// - Fixed options for Status and Kind
/// - Multi-select list for repos (in ALL view only)
///
/// # Arguments
///
/// * `author_filter`, `tag_filter`, etc. - Current active filter values
/// * `issues` - Loaded issue data (used to populate Author and Tag dropdowns)
/// * `details_comments` - Comments from expanded issue (also contributes to Author list)
/// * `repo_specs` - All configured repos (for repo multi-select)
/// * `repo_filter` - Currently selected repos (for multi-select state)
///
/// # Returns
///
/// A complete FilterState ready for UI rendering.
pub fn make_filter_state(
    author_filter: &Option<String>,
    tag_filter: &Option<String>,
    status_filter: &Option<String>,
    search_filter: &Option<String>,
    kind_filter: &Option<String>,
    issues: Option<&IssueList>,
    details_comments: Option<&Vec<Comment>>,
    repo_specs: &[RepoSpec],
    repo_filter: &[String],
) -> FilterState {
    let mut filter_state = FilterState::default();

    // populate from current filters
    filter_state.author = author_filter.clone().unwrap_or_default();
    filter_state.tag = tag_filter.clone().unwrap_or_default();

    // Status dropdown — fixed options
    filter_state.status = status_filter.clone().unwrap_or_default();
    filter_state.status_selected = filter_state
        .status_items
        .iter()
        .position(|item| item == &filter_state.status)
        .unwrap_or(0);

    // Kind dropdown — fixed options
    filter_state.kind = kind_filter.clone().unwrap_or_default();
    filter_state.kind_selected = filter_state
        .kind_items
        .iter()
        .position(|item| item == &filter_state.kind)
        .unwrap_or(0);

    filter_state.query = search_filter.clone().unwrap_or_default();

    // gather unique authors
    let author_names =
        collect_unique_sorted(
            issues
                .iter()
                .flat_map(|issue_list| {
                    issue_list
                        .issue_data
                        .iter()
                        .map(|issue| issue.user.login.clone())
                })
                .chain(details_comments.iter().flat_map(|comments| {
                    comments.iter().map(|comment| comment.user.login.clone())
                })),
        );
    (filter_state.author_items, filter_state.author_selected) =
        build_dropdown(author_names.into_iter(), &filter_state.author);

    // gather unique tags
    let tag_names = collect_unique_sorted(
        issues
            .iter()
            .flat_map(|issue_list| issue_list.issue_data.iter())
            .flat_map(|issue| issue.labels.0.iter().map(|label| label.name.clone())),
    );
    (filter_state.tag_items, filter_state.tag_selected) =
        build_dropdown(tag_names.into_iter(), &filter_state.tag);

    // Populate repo multi-select only when in ALL view
    if matches!(
        issues.as_ref().map(|issue_list| &issue_list.meta_data),
        Some(IssueSource::All)
    ) && repo_specs.len() > 1
    {
        let repo_items: Vec<String> = repo_specs
            .iter()
            .filter(|repo_spec| repo_spec.name.as_str() != ALL_CONFIGURED)
            .map(|repo_spec| repo_spec.name.clone())
            .collect();
        let repo_toggles: Vec<bool> = repo_items
            .iter()
            .map(|name| repo_filter.contains(name))
            .collect();
        filter_state.repo_items = repo_items;
        filter_state.repo_toggles = repo_toggles;
        filter_state.repo_cursor = 0;
    }

    filter_state
}

/// Checks if a single issue passes all active filters.
///
/// Tests the issue against each active filter:
/// - Status: Case-insensitive comparison with issue.state
/// - Author: Case-insensitive substring match with creator login
/// - Tag: Case-insensitive substring match against any label
/// - Search: Case-insensitive substring match against title or body
/// - Kind: Check if issue is a PR ("pr") or not ("issue"), empty = both
/// - Repo: Check if repo_name is in the filter list (only applies in ALL view)
///
/// All filters are AND'd together; all must pass for the issue to match.
///
/// # Arguments
///
/// * `issue` - The issue to test
/// * `status_filter`, `author_filter`, etc. - The active filter values (None = no filter)
/// * `repo_filter` - List of repo names to match (empty = all repos)
///
/// # Returns
///
/// `true` if the issue passes all filters, `false` otherwise.
pub fn matches_filter(
    issue: &Issue,
    status_filter: &Option<String>,
    author_filter: &Option<String>,
    tag_filter: &Option<String>,
    search_filter: &Option<String>,
    kind_filter: &Option<String>,
    repo_filter: &[String],
) -> bool {
    // status
    if let Some(status_val) = status_filter {
        if !issue.state.eq_ignore_ascii_case(status_val) {
            return false;
        }
    }
    // author
    if let Some(author_str) = author_filter {
        let author_lower = author_str.to_lowercase();
        if !issue.user.login.to_lowercase().contains(&author_lower) {
            return false;
        }
    }
    // tag
    if let Some(tag_str) = tag_filter {
        let tag_lower = tag_str.to_lowercase();
        if !issue
            .labels
            .0
            .iter()
            .any(|label| label.name.to_lowercase().contains(&tag_lower))
        {
            return false;
        }
    }
    // search
    if let Some(search_query) = search_filter {
        let query_lower = search_query.to_lowercase();
        if !issue.title.to_lowercase().contains(&query_lower)
            && !issue.body.to_lowercase().contains(&query_lower)
        {
            return false;
        }
    }
    // kind (issue vs PR)
    if let Some(kind_val) = kind_filter {
        let is_pr = issue.pull_request.is_some();
        match kind_val.as_str() {
            "pr" => {
                if !is_pr {
                    return false;
                }
            }
            "issue" => {
                if is_pr {
                    return false;
                }
            }
            _ => {} // "" or anything else = both
        }
    }
    // repo filter (multi-select; only applied when the issue has a repo_name, i.e. ALL view)
    if !repo_filter.is_empty() && !issue.repo_name.is_empty() {
        if !repo_filter
            .iter()
            .any(|repo_name| repo_name == &issue.repo_name)
        {
            return false;
        }
    }
    true
}

/// Returns all issues from a list that pass the active filters.
///
/// Filters issues using `matches_filter` and sorts the result:
/// - PRs float to the top
/// - Non-PRs follow
/// - Order within each group is preserved (stable sort)
///
/// # Arguments
///
/// * `issues` - The issue list to filter (None returns empty vector)
/// * `status_filter`, `author_filter`, etc. - Active filter values
/// * `repo_filter` - List of selected repos
///
/// # Returns
///
/// A vector of references to matching issues (PRs first, stable sort).
/// Returns empty vector if `issues` is None.
pub fn filtered_issues<'a>(
    issues: Option<&'a IssueList>,
    status_filter: &Option<String>,
    author_filter: &Option<String>,
    tag_filter: &Option<String>,
    search_filter: &Option<String>,
    kind_filter: &Option<String>,
    repo_filter: &[String],
) -> Vec<&'a Issue> {
    match issues {
        Some(list) => {
            let mut result: Vec<&'a Issue> = list
                .issue_data
                .iter()
                .filter(|issue| {
                    matches_filter(
                        issue,
                        status_filter,
                        author_filter,
                        tag_filter,
                        search_filter,
                        kind_filter,
                        repo_filter,
                    )
                })
                .collect();
            // PRs float to the top; order within each group is preserved (stable sort).
            result.sort_by_key(|issue| if issue.pull_request.is_some() { 0 } else { 1 });
            result
        }
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssigneeList, Creator, LabelsList};

    fn make_issue(state: &str) -> Issue {
        Issue {
            number: 1,
            url: String::new(),
            html_url: String::new(),
            title: "test".to_string(),
            state: state.to_string(),
            locked: false,
            labels: LabelsList(vec![]),
            assignees: AssigneeList(vec![]),
            user: Creator {
                login: "user".to_string(),
            },
            comments: 0,
            comments_url: String::new(),
            author_association: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            body: String::new(),
            closed_at: String::new(),
            pull_request: None,
            repo_name: String::new(),
        }
    }

    #[test]
    fn open_filter_excludes_closed() {
        let issue = make_issue("closed");
        let status = Some("open".to_string());
        assert!(!matches_filter(
            &issue,
            &status,
            &None,
            &None,
            &None,
            &None,
            &[]
        ));
    }

    #[test]
    fn open_filter_passes_open() {
        let issue = make_issue("open");
        let status = Some("open".to_string());
        assert!(matches_filter(
            &issue,
            &status,
            &None,
            &None,
            &None,
            &None,
            &[]
        ));
    }

    #[test]
    fn no_filter_passes_any() {
        let issue = make_issue("closed");
        assert!(matches_filter(
            &issue,
            &None,
            &None,
            &None,
            &None,
            &None,
            &[]
        ));
    }
}
