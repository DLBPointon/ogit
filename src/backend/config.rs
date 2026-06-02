use crate::model::{Config, GenericError, RepoDb, RepoSpec};
use serde_json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Return the `$HOME/.ogit` directory as a [`PathBuf`].
/// Every function that touches the ogit data directory should go through here
/// so that the path and the missing-`$HOME` error message are defined once.
pub(super) fn ogit_dir() -> Result<PathBuf, GenericError> {
    let home = env::var("HOME").map_err(|_| "Cannot locate config: $HOME is not set")?;
    Ok(Path::new(&home).join(".ogit"))
}

/// Load repository specs from `$HOME/.ogit/config.json`.
/// Returns an empty `Vec` if the file has no `repos` section.
pub fn load_repo_specs() -> Result<Vec<RepoSpec>, GenericError> {
    let global = ogit_dir()?.join("config.json");
    if !global.exists() {
        return Ok(vec![]);
    }
    let contents = fs::read_to_string(&global)?;
    let v: serde_json::Value = serde_json::from_str(&contents)?;
    if let Some(repos_v) = v.get("repos") {
        let mut specs: Vec<RepoSpec> = serde_json::from_value(repos_v.clone())?;
        populate_organisations(&mut specs);
        return Ok(specs);
    }
    Ok(vec![])
}

/// For any `RepoSpec` whose `organisation` field is empty, derive it from the
/// `address` field (the owner portion of `"owner/repo"`).
fn populate_organisations(specs: &mut Vec<RepoSpec>) {
    for spec in specs.iter_mut() {
        if spec.organisation.is_empty() && spec.address.contains('/') {
            spec.organisation = spec.address.split('/').next().unwrap_or("").to_string();
        }
    }
}

/// Parse a remote URL out of a git config string.
/// Prefers `[remote "origin"]`; falls back to any other remote section.
pub(super) fn extract_remote_url(git_conf: &str) -> Option<String> {
    let mut in_origin = false;
    let mut in_any_remote = false;
    let mut fallback: Option<String> = None;
    for line in git_conf.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            let section = line.trim_start_matches('[').trim_end_matches(']').trim();
            in_origin = section == r#"remote "origin""#;
            in_any_remote = section.starts_with("remote ");
        } else if in_origin || in_any_remote {
            if line.starts_with("url") {
                if let Some(eq) = line.find('=') {
                    let url = line[eq + 1..].trim().trim_matches('"').to_string();
                    if !url.is_empty() {
                        if in_origin {
                            return Some(url);
                        }
                        if fallback.is_none() {
                            fallback = Some(url);
                        }
                    }
                }
            }
        }
    }
    fallback
}

pub fn repo_spec_from_gitconfig(repo_path: &str) -> Result<RepoSpec, GenericError> {
    let git_config_path = find_git_config(repo_path)?;
    let git_conf = fs::read_to_string(&git_config_path)?;

    let url = extract_remote_url(&git_conf)
        .ok_or_else(|| format!("Missing 'url' in [remote] of {}", git_config_path))?;

    let (org, repo) = normalise_remote_url(&url)?;

    let git_conf_path = Path::new(&git_config_path);
    let repo_root = git_conf_path
        .parent()
        .and_then(|parent_path| {
            if parent_path
                .file_name()
                .map(|filename| filename == ".git")
                .unwrap_or(false)
            {
                parent_path.parent()
            } else {
                Some(parent_path)
            }
        })
        .ok_or("Failed to compute repository root directory")?;

    Ok(RepoSpec {
        name: format!("{}/{}", org, repo),
        address: format!("{}/{}", org, repo),
        path: repo_root.to_string_lossy().to_string(),
        colour: String::new(),
        source: String::new(),
        pinned: false,
        organisation: org.clone(),
    })
}

pub fn save_repo_specs(specs: &[RepoSpec]) -> Result<(), GenericError> {
    let dir = ogit_dir()?;
    fs::create_dir_all(&dir)?;

    let file_path = dir.join("config.json");
    let mut root: serde_json::Value = if file_path.exists() {
        let file_contents = fs::read_to_string(&file_path)?;
        serde_json::from_str(&file_contents)?
    } else {
        serde_json::json!({})
    };

    root["repos"] = serde_json::to_value(specs)?;

    let out = serde_json::to_string_pretty(&root)?;
    fs::write(&file_path, out)?;
    Ok(())
}

/// Load configuration from `$HOME/.ogit/config.json`.
pub fn load_config() -> Result<Config, GenericError> {
    let global = ogit_dir()?.join("config.json");
    if global.exists() {
        let contents = fs::read_to_string(&global)?;
        let cfg: Config = serde_json::from_str(&contents)?;
        return Ok(cfg);
    }
    Err("No configuration file found at $HOME/.ogit/config.json".into())
}

/// Locate the `.git/config` file for a given path.
/// Exposed to other backend sub-modules so they can resolve git directories.
pub(super) fn find_git_config(repo_path: &str) -> Result<String, GenericError> {
    let path_obj = Path::new(repo_path);

    if path_obj.exists() {
        if path_obj.is_file() {
            return Ok(repo_path.to_string());
        }
        if path_obj.is_dir() {
            let candidate = path_obj.join(".git").join("config");
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }

    if repo_path.ends_with(".git/config") {
        let fallback_path = Path::new(repo_path);
        if fallback_path.exists() && fallback_path.is_file() {
            return Ok(repo_path.to_string());
        }
    }

    let mut dir = env::current_dir()?;
    loop {
        let candidate = dir.join(".git").join("config");
        if candidate.exists() {
            return Ok(candidate.to_string_lossy().to_string());
        }
        if !dir.pop() {
            break;
        }
    }

    Err("Could not find .git/config in current or parent directories".into())
}

// ── Repo discovery database ────────────────────────────────────────────────────

/// Load `$HOME/.ogit/repo_db.json`, returning an empty [`RepoDb`] if the file
/// does not exist yet.
pub fn load_repo_db() -> Result<RepoDb, GenericError> {
    let path = ogit_dir()?.join("repo_db.json");
    if path.exists() {
        let contents = fs::read_to_string(&path)?;
        let db: RepoDb = serde_json::from_str(&contents)?;
        return Ok(db);
    }
    Ok(RepoDb::default())
}

/// Persist `db` to `$HOME/.ogit/repo_db.json`.
pub fn save_repo_db(repo_db: &RepoDb) -> Result<(), GenericError> {
    let dir = ogit_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("repo_db.json");
    let out = serde_json::to_string_pretty(repo_db)?;
    fs::write(&path, out)?;
    Ok(())
}

pub fn normalise_remote_url(url: &str) -> Result<(String, String), GenericError> {
    let mut path = url.to_string();
    if url.contains(':') && !url.contains("://") {
        if let Some(colon) = url.find(':') {
            path = url[colon + 1..].to_string();
        }
    } else if url.contains("://") {
        if let Some(slash) = url.find("//") {
            let after = &url[slash + 2..];
            if let Some(first_slash) = after.find('/') {
                path = after[first_slash + 1..].to_string();
            }
        }
    }
    if path.ends_with(".git") {
        path.truncate(path.len() - 4);
    }
    let parts: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if parts.len() < 2 {
        return Err(format!("Cannot parse owner/repo from URL: {}", url).into());
    }
    Ok((
        parts[parts.len() - 2].to_string(),
        parts[parts.len() - 1].to_string(),
    ))
}
