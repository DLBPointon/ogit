//! Backend API interactions, caching, and configuration.
//!
//! This module provides the public facade for all backend operations:
//!
//! - **Fetching**: Issue, comment, organization, and user data from forges
//! - **Configuration**: Loading and saving config files, repo specs, and discovery database
//! - **Cloning**: Repository cloning and color generation
//! - **Caching**: Local storage of issues and comments to reduce API calls
//! - **Normalization**: Converting between URL formats and API responses
//!
//! All public functions are re-exported here from focused sub-modules.

mod cache;
mod clone;
mod config;
mod fetch;
mod normalise;

pub use clone::clone_repo;
pub use config::{
    load_config, load_repo_db, load_repo_specs, normalise_remote_url, repo_spec_from_gitconfig,
    save_repo_db, save_repo_specs,
};
pub use fetch::{
    fetch_collaborator_repos, fetch_comments, fetch_issues, fetch_issues_many, fetch_org_repos,
    fetch_user_issue_count, fetch_user_orgs,
};
