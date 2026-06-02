use crate::model::{Comment, GenericError, IssueList, RepoSpec};
use chrono::Utc;
use serde_json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Sanitise an `"owner/repo"` address into a filename-safe string by replacing
/// `/` with `_`.  Other characters (hyphens, dots) are kept as-is.
fn sanitize_address(address: &str) -> String {
    address.replace('/', "_")
}

/// Returns the `~/.ogit/issues/{source}_{address}_issues.json` path for a repo
/// spec.  Returns `None` only when the spec has no `address` (e.g. the ALL sentinel).
pub(super) fn cache_path_for_repo(spec: &RepoSpec) -> Option<PathBuf> {
    if spec.address.trim().is_empty() {
        return None;
    }
    Some(issues_dir_path(
        spec.source.trim(),
        spec.address.trim(),
        "issues.json",
    ))
}

/// Returns the `~/.ogit/issues/{source}_{address}_comments.json` path for the
/// per-repo comment cache.  Returns `None` only if `$HOME` is unset.
fn comments_cache_path(source_name: &str, owner_repo: &str) -> Option<PathBuf> {
    Some(issues_dir_path(
        source_name.trim(),
        owner_repo,
        "comments.json",
    ))
}

/// Build a path inside `~/.ogit/issues/` for the given source, repo address, and suffix.
fn issues_dir_path(source: &str, address: &str, suffix: &str) -> PathBuf {
    let dir = super::config::ogit_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("issues");
    let addr = sanitize_address(address);
    let filename = if source.is_empty() {
        format!("{}_{}", addr, suffix)
    } else {
        format!("{}_{}_{}", source, addr, suffix)
    };
    dir.join(filename)
}

/// Reads a cached `IssueList` from disk, marking it as cached. Returns `None` on any error.
pub(super) fn read_issues_cache(cache_path: &PathBuf) -> Option<IssueList> {
    if !cache_path.exists() {
        return None;
    }
    let contents = fs::read_to_string(cache_path).ok()?;
    let mut cached: IssueList = serde_json::from_str(&contents).ok()?;
    cached.cached = true;
    Some(cached)
}

/// Writes an `IssueList` to disk, creating parent directories as needed.
pub(super) fn write_issues_cache(
    cache_path: &PathBuf,
    list: &IssueList,
) -> Result<(), GenericError> {
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(list)?;
    fs::write(cache_path, serialized)?;
    Ok(())
}

/// Read all cached comments for `issue_no` from the per-repo comment cache.
/// Returns `None` if no cached data exists for this issue.
pub(super) fn read_comments_cache(
    source_name: &str,
    owner_repo: &str,
    issue_no: &str,
) -> Option<Vec<Comment>> {
    let path = comments_cache_path(source_name, owner_repo)?;
    if !path.exists() {
        return None;
    }
    let contents = fs::read_to_string(&path).ok()?;
    let mut map: HashMap<String, Vec<Comment>> = serde_json::from_str(&contents).ok()?;
    map.remove(issue_no)
}

/// Write fetched comments for `issue_no` into the per-repo comment cache.
///
/// - `page == 1`: replaces any existing comments for this issue (fresh fetch).
/// - `page > 1`: appends to the existing cached comments, so previously-fetched
///   pages are preserved when the user is offline.
///
/// Other issues in the same per-repo file are never touched.
pub(super) fn write_comments_cache(
    source_name: &str,
    owner_repo: &str,
    issue_no: &str,
    comments: &[Comment],
    page: usize,
) -> Result<(), GenericError> {
    let path = comments_cache_path(source_name, owner_repo).ok_or("$HOME is not set")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut map: HashMap<String, Vec<Comment>> = if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    if page == 1 {
        map.insert(issue_no.to_string(), comments.to_vec());
    } else {
        map.entry(issue_no.to_string())
            .or_default()
            .extend_from_slice(comments);
    }
    fs::write(&path, serde_json::to_string_pretty(&map)?)?;
    Ok(())
}

#[allow(dead_code)]
const SECS_PER_MINUTE: i64 = 60;
const SECS_PER_HOUR: i64 = 3_600;
const SECS_PER_DAY: i64 = 86_400;
/// Approximate seconds in one calendar month (30 days).
const SECS_PER_MONTH: i64 = 30 * SECS_PER_DAY;
/// Approximate seconds in one calendar year (365 days).
const SECS_PER_YEAR: i64 = 365 * SECS_PER_DAY;

/// Compute a human-readable "X ago" string from an RFC-3339 timestamp.
/// Available for render-time use; call this from display code rather than
/// pre-baking the string into the cache.
#[allow(dead_code)]
pub(crate) fn humanize_timestamp(timestamp_str: &str) -> String {
    if let Ok(parsed_dt) = chrono::DateTime::parse_from_rfc3339(timestamp_str) {
        let dt_utc = parsed_dt.with_timezone(&Utc);
        let now = Utc::now();
        let dur = now.signed_duration_since(dt_utc);
        let secs = dur.num_seconds();
        if secs < 0 {
            return "just now".to_string();
        }
        if secs < 5 {
            "just now".to_string()
        } else if secs < SECS_PER_MINUTE {
            format!("{}s ago", secs)
        } else if secs < SECS_PER_HOUR {
            format!("{}m ago", secs / SECS_PER_MINUTE)
        } else if secs < SECS_PER_DAY {
            format!("{}h ago", secs / SECS_PER_HOUR)
        } else if secs < SECS_PER_MONTH {
            format!("{}d ago", secs / SECS_PER_DAY)
        } else if secs < SECS_PER_YEAR {
            format!("{}mo ago", secs / SECS_PER_MONTH)
        } else {
            format!("{}y ago", secs / SECS_PER_YEAR)
        }
    } else {
        timestamp_str.to_string()
    }
}
