//! Domain types — no I/O, no rendering.
//!
//! This module is the shared foundation: `backend/` constructs these types,
//! `tui/` reads them, and everything else flows upward.  `model.rs` has no
//! project-internal imports.

use serde::{Deserialize, Serialize};

pub type GenericError = Box<dyn std::error::Error + Send + Sync>;

/// The display name of the synthetic "fetch everything" sentinel at index 0
/// of `repo_specs`. Defined as a constant so all comparisons stay in sync.
pub const ALL_CONFIGURED: &str = "All (configured)";

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
    GitHub,
    GitLab,
    Gitea,
}

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
    /// Return the SourceConfig that should be used for a given RepoSpec.
    /// Priority: spec.source name match → first enabled source → legacy flat token.
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

    /// Return a SourceConfig by name, with legacy fallback.
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

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Creator {
    pub login: String,
}

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

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct LabelsList(pub Vec<Labels>);

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Assignee {
    #[serde(deserialize_with = "null_to_default")]
    pub login: String,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct AssigneeList(pub Vec<Assignee>);

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

/// Whether an [`IssueList`] holds issues from all repositories or a single named repo.
///
/// Serialises/deserialises as a plain string for backward-compatibility with
/// existing cache files: `"all"` ↔ `All`, any other string ↔ `Repo(string)`.
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

/// A repository entry stored in the global config to populate the left-hand repo list.
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
    /// Construct a minimal [`RepoSpec`] from a bare `"owner/repo"` address string.
    /// `org`, `name`, and `organisation` are all derived from the address.
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

// Taken from Steven Marnachs answer here:
// https://stackoverflow.com/questions/69225348/transforming/null-in-json-to-empty-string-instead-of-none
pub fn null_to_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let key = Option::<T>::deserialize(de)?;
    Ok(key.unwrap_or_default())
}

// ── Org-discovery database ────────────────────────────────────────────────────

/// One row in the repo discovery database — a repo found via an org API call
/// that may not yet be in the user's `config.json`.
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

/// The full repo-discovery database, stored at `$HOME/.ogit/repo_db.json`.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RepoDb {
    #[serde(default)]
    pub entries: Vec<RepoDbEntry>,
}
