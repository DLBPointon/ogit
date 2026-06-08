//! Domain types and data structures.
//!
//! This module defines all core data types with zero I/O or rendering logic.
//! It serves as the shared foundation:
//!
//! - `backend/` constructs these types from API responses
//! - `tui/` reads and renders them
//! - `filter/` and `app/` operate on them for filtering and state management
//!
//! Being dependency-free and I/O-free makes these types easily testable
//! and reusable across different contexts.

use serde::{Deserialize, Serialize};

/// A generic boxed error type used throughout the application.
///
/// Allows for easy error propagation without specifying concrete error types.
pub type GenericError = Box<dyn std::error::Error + Send + Sync>;

/// The display name of the synthetic "fetch everything" sentinel.
///
/// This sentinel is always at index 0 of `repo_specs` and represents a
/// multi-repo view showing issues from all configured repositories.
/// Defined as a constant so all comparisons stay in sync.
pub const ALL_CONFIGURED: &str = "All (configured)";

/// Default value function for serde deserialization.
/// Used to default boolean fields to `true` when not specified in JSON.
fn default_true() -> bool {
    true
}

/// Represents the type of issue forge/platform.
///
/// Determines API endpoints, authentication schemes, and available features.
/// Defaults to GitHub.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
    GitHub,
    GitLab,
    Gitea,
}

/// Configuration for a single issue forge source.
///
/// Each source represents a GitHub, GitLab, or Gitea instance that ogit
/// can connect to. Multiple sources allow browsing issues across different
/// platforms from the same application.
///
/// # Fields
///
/// - `name`: Identifier for this source (e.g., "github", "company-gitea")
/// - `platform`: The forge type (GitHub, GitLab, Gitea)
/// - `enabled`: Whether this source is active (default: true)
/// - `token`: Authentication token (Personal Access Token, API token, etc.)
/// - `api_url`: Base API endpoint (e.g., "https://api.github.com")
/// - `username`: Optional per-source username for the authenticated user
/// - `allow_insecure_tls`: Skip TLS verification (for self-hosted with self-signed certs)
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SourceConfig {
    pub name: String,
    pub platform: Platform,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(alias = "personal_access_token", deserialize_with = "null_to_default")]
    pub token: String,
    #[serde(deserialize_with = "null_to_default")]
    pub api_url: String,
    /// Per-source login name for the authenticated user.
    /// Takes precedence over the top-level `user` field when set.
    #[serde(default)]
    pub username: Option<String>,
    /// Disable TLS certificate verification for this source.
    /// Set to `true` for self-hosted instances that use self-signed certificates.
    /// Defaults to `false`.
    #[serde(default)]
    pub allow_insecure_tls: bool,
}

/// Global application configuration.
///
/// Loaded from `~/.ogit/config.json`. Contains authentication tokens,
/// configured repositories, and UI preferences.
///
/// Supports both legacy single-token format and new multi-source format
/// for backward compatibility.
///
/// # Fields
///
/// - `token`: Legacy flat token (for backward compatibility)
/// - `user`: Default username for all sources
/// - `sources`: List of configured forge sources
/// - `details_fields`: Custom fields to display in issue details
/// - `user_colour`: Custom color for the current user's name
/// - `clone_root`: Base directory for `ogit clone` (defaults to `~/code`)
/// - `comments_per_page`: Comments fetched per API request (default: 10)
/// - `max_worker_threads`: Max concurrent background threads (default: 4)
#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    /// Legacy flat token — accepted for backward compatibility
    #[serde(alias = "personal_access_token", default)]
    pub token: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    /// New multi-platform sources list
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
    #[serde(default)]
    pub details_fields: Option<Vec<String>>,
    #[serde(rename = "user_colour", alias = "user_color", default)]
    pub user_colour: Option<String>,
    /// Root directory for `ogit clone` operations.
    /// Repos are cloned to `{clone_root}/{owner}/{repo}`.
    /// Defaults to `$HOME/code` if not set.
    #[serde(default)]
    pub clone_root: Option<String>,
    /// How many comments to fetch per API page. Defaults to 10.
    #[serde(default = "default_comments_per_page")]
    pub comments_per_page: usize,
    /// Maximum number of background worker threads that may run concurrently.
    /// Increase for faster parallel fetching; decrease to reduce load on the host.
    /// Defaults to 4.
    #[serde(default = "default_max_worker_threads")]
    pub max_worker_threads: usize,
}

fn default_comments_per_page() -> usize {
    10
}

fn default_max_worker_threads() -> usize {
    4
}

impl Config {
    /// Finds the appropriate source configuration for a given repository.
    ///
    /// Uses the following priority order:
    /// 1. If `spec.source` is set, use the matching enabled source
    /// 2. Use the first enabled source in the list
    /// 3. Fall back to a synthetic source created from the legacy flat token
    ///
    /// # Arguments
    ///
    /// * `spec` - The repository specification
    ///
    /// # Returns
    ///
    /// `Some(SourceConfig)` if a source is available, `None` otherwise.
    pub fn source_for_repo(&self, spec: &RepoSpec) -> Option<SourceConfig> {
        if !spec.source.trim().is_empty() {
            if let Some(source_cfg) = self
                .sources
                .iter()
                .find(|source_cfg| source_cfg.name == spec.source && source_cfg.enabled)
            {
                return Some(source_cfg.clone());
            }
        }
        if let Some(source_cfg) = self.sources.iter().find(|source_cfg| source_cfg.enabled) {
            return Some(source_cfg.clone());
        }
        self.token.as_ref().map(|token_val| SourceConfig {
            name: "legacy".to_string(),
            platform: Platform::GitHub,
            enabled: true,
            token: token_val.clone(),
            api_url: "https://api.github.com".to_string(),
            username: None,
            allow_insecure_tls: false,
        })
    }

    /// Looks up a source configuration by name.
    ///
    /// Searches for a source with the matching name. If no enabled source
    /// with that name is found, falls back to a synthetic source from the
    /// legacy flat token.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the source to look up
    ///
    /// # Returns
    ///
    /// `Some(SourceConfig)` if a matching source is found, `None` otherwise.
    pub fn source_by_name(&self, name: &str) -> Option<SourceConfig> {
        if let Some(source_cfg) = self
            .sources
            .iter()
            .find(|source_cfg| source_cfg.name == name)
        {
            return Some(source_cfg.clone());
        }
        self.token.as_ref().map(|token_val| SourceConfig {
            name: "legacy".to_string(),
            platform: Platform::GitHub,
            enabled: true,
            token: token_val.clone(),
            api_url: "https://api.github.com".to_string(),
            username: None,
            allow_insecure_tls: false,
        })
    }
}

/// Represents the author/creator of an issue or comment.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Creator {
    pub login: String,
}

/// A single label/tag applied to an issue.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Labels {
    pub id: Option<u64>,

    #[serde(deserialize_with = "null_to_default")]
    pub name: String,

    // GitHub labels use the key "color" in their JSON; map that to our `colour` field
    #[serde(rename = "color", deserialize_with = "null_to_default")]
    pub colour: String,

    #[serde(deserialize_with = "null_to_default")]
    pub description: String,
}

/// A list of labels/tags.
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct LabelsList(pub Vec<Labels>);

/// A user assigned to an issue.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Assignee {
    #[serde(deserialize_with = "null_to_default")]
    pub login: String,
}

/// A list of assignees.
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct AssigneeList(pub Vec<Assignee>);

/// Metadata for a pull request (when an issue is also a PR).
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct PullRequest {
    #[serde(deserialize_with = "null_to_default")]
    pub url: String,

    #[serde(deserialize_with = "null_to_default")]
    pub html_url: String,

    #[serde(deserialize_with = "null_to_default")]
    pub diff_url: String,

    #[serde(deserialize_with = "null_to_default")]
    pub patch_url: String,

    #[serde(deserialize_with = "null_to_default")]
    pub merged_at: String,
}

/// A single issue or pull request from an issue tracker.
///
/// Represents either an issue or PR from a forge. The `pull_request` field
/// distinguishes between the two. Most fields map directly to API responses.
///
/// # Fields
///
/// - `number`: Issue number (unique within a repo)
/// - `title`: Issue title
/// - `state`: "open" or "closed"
/// - `user`: Author of the issue
/// - `labels`: Associated tags/labels
/// - `assignees`: Users assigned to the issue
/// - `comments`: Total number of comments
/// - `body`: Issue description/body text
/// - `pull_request`: Set if this is a PR (distinguishes PRs from issues)
/// - `repo_name`: Repo short name (populated only in multi-repo ALL view)
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Issue {
    pub number: u32,
    pub url: String,
    #[serde(deserialize_with = "null_to_default")]
    pub html_url: String,
    pub title: String,
    pub state: String,
    pub locked: bool,
    pub labels: LabelsList,
    pub assignees: AssigneeList,
    pub user: Creator,
    pub comments: u32,
    pub comments_url: String,

    // Should never be null
    pub author_association: String,
    pub created_at: String,
    pub updated_at: String,

    #[serde(deserialize_with = "null_to_default")]
    pub body: String,

    #[serde(deserialize_with = "null_to_default")]
    pub closed_at: String,

    pub pull_request: Option<PullRequest>,

    /// The name of the repo this issue belongs to.
    /// Populated by `fetch_issues_many` for the ALL view; empty for single-repo fetches.
    #[serde(default)]
    pub repo_name: String,
}

/// A single comment on an issue or PR.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Comment {
    pub id: u64,

    #[serde(deserialize_with = "null_to_default")]
    pub body: String,

    pub user: Creator,
    pub created_at: String,

    #[serde(deserialize_with = "null_to_default")]
    pub updated_at: String,
}

/// Indicates the scope of an [`IssueList`].
///
/// Distinguishes between aggregated multi-repo views and single-repo views.
/// Serializes/deserializes as a plain string for backward-compatibility:
/// - `"all"` ↔ `All` (multi-repo)
/// - Any other string ↔ `Repo(address)` (single repo)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum IssueSource {
    /// Aggregated multi-repo view.
    All,
    /// Issues from a single `"owner/repo"` repository.
    Repo(String),
}

impl From<String> for IssueSource {
    fn from(raw_str: String) -> Self {
        if raw_str == "all" {
            Self::All
        } else {
            Self::Repo(raw_str)
        }
    }
}

impl From<IssueSource> for String {
    fn from(src: IssueSource) -> String {
        match src {
            IssueSource::All => "all".to_string(),
            IssueSource::Repo(s) => s,
        }
    }
}

/// A collection of issues with metadata.
///
/// Returned from API calls, represents either a single repo or aggregated
/// multi-repo view. May be cached.
///
/// # Fields
///
/// - `issue_data`: The actual issues
/// - `meta_data`: Indicates whether this is All or Repo(address)
/// - `cached`: Whether this data came from local cache vs. fresh API call
/// - `last_updated_ts`: RFC-3339 timestamp of last sync
/// - `warnings`: Non-fatal warnings (e.g., repos skipped due to errors)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IssueList {
    pub issue_data: Vec<Issue>,
    pub meta_data: IssueSource,
    #[serde(default)]
    pub cached: bool,
    /// Precise RFC3339 timestamp of last sync.
    /// The human-readable string is computed on-the-fly from this field at render time.
    #[serde(default)]
    pub last_updated_ts: Option<String>,
    /// Non-fatal warnings collected during a multi-repo fetch (e.g. repos that
    /// were skipped due to missing config or network errors).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// A repository configured by the user.
///
/// Represents a repository that the user has added to their config
/// and wants to monitor. Stored in `~/.ogit/config.json`.
///
/// # Fields
///
/// - `name`: Display name (e.g., "rust", "my-project")
/// - `address`: Full address in `owner/repo` format
/// - `path`: Optional local file path (if cloned locally)
/// - `colour`: Optional custom color for display (CSS format)
/// - `source`: Name of the SourceConfig to use (empty = use default)
/// - `pinned`: Whether to float to the top of the repo panel
/// - `organisation`: The forge organisation name (e.g., "rust-lang")
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RepoSpec {
    #[serde(deserialize_with = "null_to_default")]
    pub name: String,

    #[serde(deserialize_with = "null_to_default")]
    pub address: String,

    #[serde(deserialize_with = "null_to_default")]
    pub path: String,

    /// Optional colour to display for this repo (e.g. "#ff0000"). Defaults to empty string.
    #[serde(
        rename = "colour",
        alias = "color",
        deserialize_with = "null_to_default"
    )]
    pub colour: String,

    /// Name of the SourceConfig entry this repo belongs to. Empty = use first enabled source.
    #[serde(default)]
    pub source: String,

    /// Whether this repo is pinned to the top of the repository panel.
    #[serde(default)]
    pub pinned: bool,

    /// The forge organisation this repo belongs to (e.g. "rust-lang").
    /// Empty for purely local repos with no forge remote.
    #[serde(default)]
    pub organisation: String,
}

impl RepoSpec {
    /// Constructs a RepoSpec from a bare `owner/repo` address.
    ///
    /// Automatically derives the display name and organisation from the address.
    /// Useful for creating minimal RepoSpecs from discovered repos or API responses.
    ///
    /// # Arguments
    ///
    /// * `address` - The repository address in `owner/repo` format
    /// * `source` - The name of the SourceConfig to use
    ///
    /// # Returns
    ///
    /// A RepoSpec with name, address, source, and organisation pre-populated.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let spec = RepoSpec::from_address("rust-lang/rust", "github");
    /// assert_eq!(spec.name, "rust");
    /// assert_eq!(spec.organisation, "rust-lang");
    /// ```
    pub fn from_address(address: impl Into<String>, source: impl Into<String>) -> Self {
        let address = address.into();
        let org = address.split('/').next().unwrap_or("").to_string();
        let name = address.split('/').last().unwrap_or(&address).to_string();
        Self {
            name,
            address,
            source: source.into(),
            organisation: org,
            ..Default::default()
        }
    }
}

/// Deserializer helper: converts JSON nulls to default values.
///
/// Used as a serde `deserialize_with` attribute to treat missing or null
/// JSON fields as the default value for the type rather than failing
/// deserialization.
///
/// # References
///
/// Based on Steven Marnach's answer:
/// https://stackoverflow.com/questions/69225348/transforming/null-in-json-to-empty-string-instead-of-none
pub fn null_to_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let key = Option::<T>::deserialize(de)?;
    Ok(key.unwrap_or_default())
}

// ── Org-discovery database ────────────────────────────────────────────────────

/// One row in the repository discovery database.
///
/// Represents a repository that was discovered via organization synchronization
/// (e.g., via `FetchOrgRepos`) but has not yet been added to the user's
/// config.json. Stored in `~/.ogit/repo_db.json`.
///
/// # Fields
///
/// - `address`: Full `owner/repo` address from the forge API
/// - `organisation`: The forge organisation this repo belongs to
/// - `source`: Name of the SourceConfig used for discovery
/// - `last_refreshed`: RFC-3339 timestamp of the last discovery sync
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RepoDbEntry {
    /// `"owner/repo"` address as returned by the forge API.
    #[serde(deserialize_with = "null_to_default")]
    pub address: String,
    /// Organisation / group name on the forge.
    #[serde(deserialize_with = "null_to_default")]
    pub organisation: String,
    /// Name of the `SourceConfig` entry that was used for discovery.
    #[serde(deserialize_with = "null_to_default")]
    pub source: String,
    /// RFC-3339 timestamp of when this entry was last refreshed.
    #[serde(default)]
    pub last_refreshed: String,
}

/// The repository discovery database.
///
/// Contains all repositories discovered via organization synchronization.
/// Stored at `~/.ogit/repo_db.json`. Acts as a cache of discovered repos
/// that the user hasn't yet configured.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RepoDb {
    #[serde(default)]
    pub entries: Vec<RepoDbEntry>,
}
