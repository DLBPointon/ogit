//! Backend public facade — re-exports from focused sub-modules.

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
