//! Local `git clone` wrapper and colour-generation for newly cloned repos.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use super::config::load_config;
use crate::model::{GenericError, Platform, SourceConfig};

// ── Clone URL ─────────────────────────────────────────────────────────────────

/// Derive the HTTPS clone URL for `address` (`"owner/repo"`) given its source.
///
/// For GitHub.com the `api_url` is `https://api.github.com`; we strip that to
/// get `https://github.com`.  For GitHub Enterprise the `api_url` looks like
/// `https://hostname/api/v3`; we strip the API suffix to get `https://hostname`.
/// For GitLab and Gitea the `api_url` is already the web root.
fn build_clone_url(source: &SourceConfig, address: &str) -> String {
    let base = source.api_url.trim_end_matches('/');
    let web_base = match source.platform {
        Platform::GitHub => {
            if base.eq_ignore_ascii_case("https://api.github.com") {
                "https://github.com".to_string()
            } else {
                // GitHub Enterprise: strip /api/v3 or /api suffix
                base.trim_end_matches("/api/v3")
                    .trim_end_matches("/api")
                    .to_string()
            }
        }
        Platform::GitLab | Platform::Gitea => base.to_string(),
    };
    format!("{}/{}.git", web_base, address)
}

// ── Colour generation ─────────────────────────────────────────────────────────

/// Pick a colour for a newly-cloned repo from a fixed palette, keyed
/// deterministically by the repo address.
pub fn generate_colour(address: &str) -> String {
    const PALETTE: &[&str] = &[
        "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#d19a66", "#be5046",
        "#7ec8a4", "#f7a35c", "#8085e9", "#f15c80", "#e4d354", "#2b908f", "#f45b5b", "#91e8e1",
    ];
    let hash = address.bytes().fold(0usize, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    });
    PALETTE[hash % PALETTE.len()].to_string()
}

// ── Clone ─────────────────────────────────────────────────────────────────────

/// Clone `address` (in `"owner/repo"` form) from its forge into
/// `{clone_root}/{owner}/{repo}`, then return `(absolute_path, colour)`.
///
/// The parent directory (`{clone_root}/{owner}/`) is created automatically.
/// The colour is chosen deterministically from the palette above so that the
/// same repo always gets the same colour.
pub fn clone_repo(
    address: &str,
    source_name: &str,
    clone_root: &str,
) -> Result<(String, String), GenericError> {
    let config = load_config()?;
    let source = config
        .source_by_name(source_name)
        .ok_or_else(|| format!("No source '{}' found in config", source_name))?;

    let url = build_clone_url(&source, address);
    // Clone into clone_root/owner/repo
    let dest = Path::new(clone_root).join(address);

    // Ensure the owner sub-directory exists
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = Command::new("git")
        .arg("clone")
        .arg(&url)
        .arg(&dest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("exit code {:?}", output.status.code())
        } else {
            stderr
        };
        return Err(format!("git clone failed: {}", detail).into());
    }

    let colour = generate_colour(address);
    Ok((dest.to_string_lossy().into_owned(), colour))
}
