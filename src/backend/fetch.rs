use crate::model::{
    Comment, Config, Creator, GenericError, Issue, IssueList, IssueSource, Platform, RepoDbEntry,
    RepoSpec, SourceConfig,
};
use chrono::Utc;
use reqwest::blocking::Client;
use serde_json;
use std::fs;
use std::sync::OnceLock;

use super::cache::{
    cache_path_for_repo, read_comments_cache, read_issues_cache, write_comments_cache,
    write_issues_cache,
};
use super::config::{extract_remote_url, find_git_config, load_config, normalise_remote_url};
use super::normalise::{normalise_comment, normalise_issue};

/// Maximum items per page accepted by GitHub, GitLab, and Gitea.
const API_PAGE_SIZE: usize = 100;

fn resolve_owner_repo(spec: &RepoSpec) -> Result<(String, String), GenericError> {
    let addr = spec.address.trim();
    if !addr.is_empty() {
        let parts: Vec<&str> = addr.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            let owner = parts[parts.len() - 2].to_string();
            let repo = parts[parts.len() - 1].trim_end_matches(".git").to_string();
            return Ok((owner, repo));
        }
    }
    if !spec.path.trim().is_empty() {
        let git_config_path = find_git_config(&spec.path)?;
        let git_conf = fs::read_to_string(&git_config_path)?;
        let url =
            extract_remote_url(&git_conf).ok_or("Could not find remote URL in .git/config")?;
        return normalise_remote_url(&url);
    }
    Err(format!("Cannot determine owner/repo for spec '{}'", spec.name).into())
}

fn build_issues_url(source: &SourceConfig, owner: &str, repo: &str) -> String {
    let base = source.api_url.trim_end_matches('/');
    match source.platform {
        Platform::GitHub => format!("{}/repos/{}/{}/issues", base, owner, repo),
        Platform::GitLab => {
            let encoded = format!("{}%2F{}", owner, repo);
            format!("{}/api/v4/projects/{}/issues", base, encoded)
        }
        Platform::Gitea => format!("{}/api/v1/repos/{}/{}/issues", base, owner, repo),
    }
}

fn apply_auth(
    req: reqwest::blocking::RequestBuilder,
    source: &SourceConfig,
) -> reqwest::blocking::RequestBuilder {
    match source.platform {
        Platform::GitHub => req
            .header("Authorization", format!("Bearer {}", source.token))
            .header("Accept", "application/vnd.github.raw+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "ogit-rs")
            .header("Connection", "keep-alive"),
        Platform::GitLab => req
            .header("PRIVATE-TOKEN", &source.token)
            .header("Accept", "application/json")
            .header("User-Agent", "ogit-rs")
            .header("Connection", "keep-alive"),
        Platform::Gitea => req
            .header("Authorization", format!("token {}", source.token))
            .header("Accept", "application/json")
            .header("User-Agent", "ogit-rs")
            .header("Connection", "keep-alive"),
    }
}

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static HTTP_CLIENT_INSECURE: OnceLock<Client> = OnceLock::new();

/// Return a shared HTTP client for the given source.
/// The client is constructed once and reused across all API calls on the same thread pool,
/// enabling connection-pool reuse as recommended by the `reqwest` docs.
/// When `allow_insecure_tls` is `true`, certificate verification is disabled.
fn build_client(source: &SourceConfig) -> &'static Client {
    if source.allow_insecure_tls {
        HTTP_CLIENT_INSECURE.get_or_init(|| {
            Client::builder()
                .danger_accept_invalid_certs(true)
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to build HTTP client (insecure)")
        })
    } else {
        HTTP_CLIENT.get_or_init(|| {
            Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to build HTTP client")
        })
    }
}

/// Return `true` if the HTTP response has a `Link` header containing `rel="next"`,
/// indicating that further pages of paginated results are available.
fn has_next_page(headers: &reqwest::header::HeaderMap) -> bool {
    headers
        .get("link")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.contains(r#"rel="next""#))
        .unwrap_or(false)
}

/// Resolve the login name of the authenticated Gitea user.
///
/// Priority: `source.username` → `config.user` → `GET /api/v1/user`
fn gitea_current_user(
    config: &Config,
    source: &SourceConfig,
    client: &Client,
) -> Result<String, GenericError> {
    // 1. Per-source username (highest priority)
    if let Some(ref source_username) = source.username {
        let trimmed_username = source_username.trim();
        if !trimmed_username.is_empty() {
            return Ok(trimmed_username.to_string());
        }
    }
    // 2. Global config username
    if let Some(ref user) = config.user {
        let user = user.trim();
        if !user.is_empty() {
            return Ok(user.to_string());
        }
    }
    // 3. Discover from the API
    let url = format!("{}/api/v1/user", source.api_url.trim_end_matches('/'));
    let resp = apply_auth(client.get(&url), source).send()?;
    let text = resp.text()?;
    let response_json: serde_json::Value = serde_json::from_str(&text)?;
    response_json
        .get("login")
        .and_then(|json_val| json_val.as_str())
        .map(|str_val| str_val.to_string())
        .ok_or_else(|| "Could not determine Gitea username from /api/v1/user".into())
}

fn fetch_issues_for_spec(
    config: &Config,
    spec: &RepoSpec,
    creator: Option<&str>,
    state_filter: Option<&str>,
    on_page: impl Fn(usize, usize),
) -> Result<IssueList, GenericError> {
    let source = config
        .source_for_repo(spec)
        .ok_or("No enabled source found for repo")?;

    let (owner, repo_name) = resolve_owner_repo(spec)?;
    let api_url = build_issues_url(&source, &owner, &repo_name);
    let cache_path = cache_path_for_repo(spec);

    let client = build_client(&source);
    let per_page = API_PAGE_SIZE;
    let mut combined: Vec<Issue> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut page = 1usize;

    // Read the existing cache (if any) to get the timestamp of the last successful
    // fetch.  When a timestamp is present we can request only issues updated since
    // then — an incremental refresh that returns a small delta instead of all pages.
    let existing_cache: Option<IssueList> =
        cache_path.as_ref().and_then(|cp| read_issues_cache(cp));
    let since_ts: Option<String> = existing_cache
        .as_ref()
        .and_then(|c| c.last_updated_ts.clone());
    let incremental = since_ts.is_some();

    // For incremental fetches we always request state=all so that issues whose
    // state changed since the last fetch (e.g. open→closed) are included in the
    // delta.  State filtering is applied client-side after merging.
    // For full fetches we translate the requested state to the platform's spelling.
    let api_state: &str = if incremental {
        "all"
    } else {
        match source.platform {
            Platform::GitLab => match state_filter {
                Some("open") => "opened",
                Some(other) => other,
                None => "all",
            },
            _ => state_filter.unwrap_or("all"),
        }
    };
    // Platform-specific query parameter name for the "updated since" filter.
    let since_param: &str = match source.platform {
        Platform::GitLab => "updated_after",
        _ => "since",
    };

    loop {
        let mut query: Vec<(&str, String)> = vec![
            ("filter", "all".to_string()),
            ("sort", "updated".to_string()),
            ("per_page", per_page.to_string()),
            ("page", page.to_string()),
            ("state", api_state.to_string()),
        ];
        if let Some(user) = creator {
            query.push(("creator", user.to_string()));
        }
        if let Some(ref ts) = since_ts {
            query.push((since_param, ts.clone()));
        }
        let req = apply_auth(client.get(&api_url).query(&query), &source);

        match req.send() {
            Ok(resp) => {
                let next_page = has_next_page(resp.headers());

                let text = resp.text()?;
                let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;
                if raw.is_empty() {
                    break;
                }
                for raw_item in raw {
                    if let serde_json::Value::Object(obj) = raw_item {
                        match normalise_issue(
                            obj,
                            &source.platform,
                            &source.api_url,
                            &owner,
                            &repo_name,
                        ) {
                            Ok(mut issue) => {
                                issue.repo_name = spec.name.clone();
                                combined.push(issue);
                            }
                            Err(e) => {
                                warnings.push(format!("Skipped issue: {}", e));
                            }
                        }
                    }
                }
                if next_page {
                    on_page(combined.len(), page);
                    page += 1;
                    continue;
                }
                on_page(combined.len(), page);
                break;
            }
            Err(e) => {
                if let Some(cp) = &cache_path {
                    if let Some(cached) = read_issues_cache(cp) {
                        return Ok(cached);
                    }
                }
                return Err(Box::new(e));
            }
        }
    }

    // ── Incremental merge ────────────────────────────────────────────────────
    // Merge the fetched delta into the full cached list.  We keep ALL known
    // issues in the cache (regardless of state) so future incremental fetches
    // always have a complete base to merge into.  The state filter is applied
    // client-side on the result that is returned to the caller.
    let all_issues: Vec<Issue> = if incremental {
        if let Some(mut cached) = existing_cache {
            // Replace each changed issue by number; append genuinely new ones.
            for updated in combined {
                match cached
                    .issue_data
                    .iter()
                    .position(|i| i.number == updated.number)
                {
                    Some(pos) => cached.issue_data[pos] = updated,
                    None => cached.issue_data.push(updated),
                }
            }
            warnings.extend(cached.warnings);
            cached.issue_data
        } else {
            combined
        }
    } else {
        combined
    };

    // Write the full (unfiltered) merged list to disk so the next incremental
    // fetch has a complete base.
    let mut issue_list = IssueList {
        issue_data: all_issues,
        meta_data: IssueSource::Repo(format!("{}/{}", owner, repo_name)),
        cached: false,
        last_updated_ts: None,
        warnings,
    };
    if let Some(cp) = &cache_path {
        issue_list.last_updated_ts = Some(Utc::now().to_rfc3339());
        let _ = write_issues_cache(cp, &issue_list);
    }

    // Apply the state filter client-side for the incremental path (the full-fetch
    // path relied on the API to do the filtering, but we requested state=all above).
    if incremental {
        if let Some(state_val) = state_filter {
            issue_list
                .issue_data
                .retain(|issue| issue.state.eq_ignore_ascii_case(state_val));
        }
    }

    Ok(issue_list)
}

pub fn fetch_issues(
    spec: &RepoSpec,
    creator: Option<&str>,
    state_filter: Option<&str>,
    on_page: impl Fn(usize, usize),
) -> Result<IssueList, GenericError> {
    let config = load_config()?;
    fetch_issues_for_spec(&config, spec, creator, state_filter, on_page)
}

pub fn fetch_issues_many(
    repo_specs: &[RepoSpec],
    state_filter: Option<&str>,
    on_progress: impl Fn(usize, usize, &str),
) -> Result<IssueList, GenericError> {
    let config = load_config()?;
    let total = repo_specs.len();
    let mut combined: Vec<Issue> = Vec::new();
    let mut used_cache = false;
    let mut warnings: Vec<String> = Vec::new();

    for (idx, spec) in repo_specs.iter().enumerate() {
        match fetch_issues_for_spec(&config, spec, None, state_filter, |_, _| {}) {
            Ok(list) => {
                if list.cached {
                    used_cache = true;
                }
                combined.extend(list.issue_data);
            }
            Err(e) => {
                warnings.push(format!("Skipped '{}': {}", spec.name, e));
            }
        }
        on_progress(idx + 1, total, &spec.name);
    }

    Ok(IssueList {
        issue_data: combined,
        meta_data: IssueSource::All,
        cached: used_cache,
        last_updated_ts: None,
        warnings,
    })
}

pub fn fetch_comments(
    comments_url: &str,
    source_name: &str,
    page: usize,
    per_page: usize,
) -> Result<(Vec<Comment>, bool, bool), GenericError> {
    let config = load_config()?;
    let source = config
        .source_by_name(source_name)
        .ok_or("No source found for comments fetch")?;
    let client = build_client(&source);

    let parts: Vec<&str> = comments_url.split('/').collect();
    let mut owner_repo_opt: Option<String> = None;
    // GitHub: /repos/{owner}/{repo}/issues/{n}/comments
    if let Some(pos) = parts.iter().position(|url_part| *url_part == "repos") {
        if parts.len() > pos + 2 {
            owner_repo_opt = Some(format!("{}/{}", parts[pos + 1], parts[pos + 2]));
        }
    }
    // GitLab: /api/v4/projects/{owner%2Frepo}/issues/{iid}/notes
    if owner_repo_opt.is_none() {
        if let Some(pos) = parts.iter().position(|url_part| *url_part == "projects") {
            if parts.len() > pos + 1 {
                // URL-decode %2F → /
                let decoded = parts[pos + 1].replace("%2F", "/").replace("%2f", "/");
                owner_repo_opt = Some(decoded);
            }
        }
    }
    let mut issue_number_opt: Option<String> = None;
    if let Some(pos) = parts.iter().position(|url_part| *url_part == "issues") {
        if parts.len() > pos + 1 {
            issue_number_opt = Some(parts[pos + 1].to_string());
        }
    } else {
        for url_part in parts.iter().rev() {
            if !url_part.is_empty()
                && url_part
                    .chars()
                    .all(|ascii_char| ascii_char.is_ascii_digit())
            {
                issue_number_opt = Some(url_part.to_string());
                break;
            }
        }
    }

    let request = apply_auth(
        client.get(comments_url).query(&[
            ("page", &page.to_string()),
            ("per_page", &per_page.to_string()),
        ]),
        &source,
    );

    match request.send() {
        Ok(resp_ok) => {
            let has_more_pages = has_next_page(resp_ok.headers());
            let text = resp_ok.text()?;
            let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;
            let comments_vec: Vec<Comment> = raw
                .into_iter()
                .filter_map(|json_val| {
                    if let serde_json::Value::Object(obj) = json_val {
                        normalise_comment(obj, &source.platform).ok()
                    } else {
                        None
                    }
                })
                .collect();

            if let (Some(owner_repo), Some(issue_no)) =
                (owner_repo_opt.as_deref(), issue_number_opt.as_deref())
            {
                let _ =
                    write_comments_cache(source_name, owner_repo, issue_no, &comments_vec, page);
            }

            Ok((comments_vec, false, has_more_pages))
        }
        Err(_e) => {
            if let (Some(owner_repo), Some(issue_no)) =
                (owner_repo_opt.as_deref(), issue_number_opt.as_deref())
            {
                if let Some(cached) = read_comments_cache(source_name, owner_repo, issue_no) {
                    return Ok((cached, true, false));
                }
            }
            Err(Box::new(_e))
        }
    }
}

/// Discover all repositories belonging to `org` on the given source platform.
///
/// - GitHub:  `GET /orgs/{org}/repos`
/// - GitLab:  `GET /groups/{group}/projects`
/// - Gitea:   `GET /orgs/{org}/repos`
///
/// Returns one [`RepoDbEntry`] per discovered repository.
pub fn fetch_org_repos(org: &str, source_name: &str) -> Result<Vec<RepoDbEntry>, GenericError> {
    let config = load_config()?;
    let source = config
        .source_by_name(source_name)
        .ok_or_else(|| format!("No source '{}' found in config", source_name))?;

    let base = source.api_url.trim_end_matches('/');
    // Start with the org/group endpoint; if the name turns out to be a personal
    // user account (GitHub/Gitea return 404 for /orgs/{user}/repos), we fall
    // back automatically to the user repos endpoint on the first 404.
    let mut url = match source.platform {
        Platform::GitHub => format!("{}/orgs/{}/repos", base, org),
        Platform::GitLab => {
            // Groups with slashes in the path must be percent-encoded
            let encoded = org.replace('/', "%2F");
            format!("{}/api/v4/groups/{}/projects", base, encoded)
        }
        Platform::Gitea => format!("{}/api/v1/orgs/{}/repos", base, org),
    };
    let mut tried_user_fallback = false;

    let client = build_client(&source);
    let mut entries: Vec<RepoDbEntry> = Vec::new();
    let mut page = 1usize;
    let now = Utc::now().to_rfc3339();

    loop {
        // Gitea uses `limit` (max 50) rather than `per_page`; the `type` filter
        // is GitHub/GitLab-specific and is harmlessly ignored by Gitea anyway.
        let query: Vec<(&str, String)> = match source.platform {
            Platform::Gitea => vec![("limit", "50".to_string()), ("page", page.to_string())],
            _ => vec![
                ("per_page", API_PAGE_SIZE.to_string()),
                ("page", page.to_string()),
                ("type", "all".to_string()),
            ],
        };
        let req = apply_auth(client.get(&url).query(&query), &source);

        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => return Err(Box::new(e)),
        };

        let status = resp.status();
        let next_page = has_next_page(resp.headers());

        let text = resp.text()?;

        // If the org endpoint returns a non-success status and we haven't yet
        // tried the user endpoint, switch to /users/{org}/repos and retry.
        // This handles the common case where the config lists a personal
        // account (e.g. "DLBPointon") under an org key.
        if !status.is_success() && !tried_user_fallback {
            match source.platform {
                Platform::GitHub => {
                    url = format!("{}/users/{}/repos", base, org);
                    tried_user_fallback = true;
                    page = 1;
                    continue;
                }
                Platform::Gitea => {
                    url = format!("{}/api/v1/users/{}/repos", base, org);
                    tried_user_fallback = true;
                    page = 1;
                    continue;
                }
                Platform::GitLab => {}
            }
        }

        if !status.is_success() {
            return Err(format!("HTTP {} fetching repos for '{}': {}", status, org, text).into());
        }

        let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;

        if raw.is_empty() {
            break;
        }

        for repo in &raw {
            // GitHub and Gitea use "full_name"; GitLab uses "path_with_namespace"
            let full_name = repo
                .get("full_name")
                .or_else(|| repo.get("path_with_namespace"))
                .and_then(|json_val| json_val.as_str())
                .map(|str_val| str_val.to_string());

            if let Some(address) = full_name {
                entries.push(RepoDbEntry {
                    address,
                    organisation: org.to_string(),
                    source: source_name.to_string(),
                    last_refreshed: now.clone(),
                });
            }
        }

        if next_page {
            page += 1;
        } else {
            break;
        }
    }

    Ok(entries)
}

/// Fetch all organisations / groups that the authenticated user is a member of.
///
/// Returns a deduplicated list of org names (the `owner` portion used in
/// `"owner/repo"` addresses).  Paginates automatically.
///
/// For Gitea, there is no reliable org-membership endpoint with standard PAT
/// scopes.  Instead, we fetch the user's repos (`/api/v1/users/{username}/repos`),
/// extract any organisation names from `owner.type == "Organization"`, then
/// always add the username itself so that the user's personal repos are also
/// discovered via the `fetch_org_repos` user-endpoint fallback.
pub fn fetch_user_orgs(source_name: &str) -> Result<Vec<String>, GenericError> {
    let config = load_config()?;
    let source = config
        .source_by_name(source_name)
        .ok_or_else(|| format!("No source '{}' found in config", source_name))?;

    let base = source.api_url.trim_end_matches('/');
    let client = build_client(&source);
    let mut orgs: Vec<String> = Vec::new();
    let mut page = 1usize;

    // ── Gitea: use the dedicated org-membership endpoint ──────────────────────
    if matches!(source.platform, Platform::Gitea) {
        let username = gitea_current_user(&config, &source, client)?;
        // /api/v1/user/orgs is the correct authenticated endpoint for listing
        // org memberships — more reliable than inferring orgs from repo owner
        // fields, and works even when the user has no repos in an org.
        let orgs_url = format!("{}/api/v1/user/orgs", base);
        loop {
            let req = apply_auth(
                client
                    .get(&orgs_url)
                    .query(&[("limit", "50"), ("page", &page.to_string())]),
                &source,
            );
            let resp = match req.send() {
                Ok(r) => r,
                Err(e) => return Err(Box::new(e)),
            };
            let status = resp.status();
            let next_page = has_next_page(resp.headers());
            let text = resp.text()?;
            if !status.is_success() {
                return Err(format!("HTTP {} from {}: {}", status, orgs_url, text.trim()).into());
            }
            let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;
            if raw.is_empty() {
                break;
            }
            for org in &raw {
                // Gitea org objects use "name" as the login/username handle.
                if let Some(name) = org
                    .get("name")
                    .or_else(|| org.get("username"))
                    .and_then(|json_val| json_val.as_str())
                {
                    if !orgs
                        .iter()
                        .any(|org_entry: &String| org_entry.eq_ignore_ascii_case(name))
                    {
                        orgs.push(name.to_string());
                    }
                }
            }
            if next_page {
                page += 1;
            } else {
                break;
            }
        }
        // Always include the user's own namespace so their personal repos are
        // discovered via FetchOrgRepos → /api/v1/users/{username}/repos fallback.
        if !orgs
            .iter()
            .any(|org_entry: &String| org_entry.eq_ignore_ascii_case(&username))
        {
            orgs.push(username);
        }
        return Ok(orgs);
    }

    // ── GitHub / GitLab: dedicated org-membership endpoint ──────────────────────
    let url = match source.platform {
        Platform::GitHub => format!("{}/user/orgs", base),
        Platform::GitLab => format!("{}/api/v4/groups", base),
        Platform::Gitea => unreachable!(),
    };

    loop {
        let mut query: Vec<(&str, String)> = vec![
            ("per_page", API_PAGE_SIZE.to_string()),
            ("page", page.to_string()),
        ];
        // GitLab: restrict to groups the user is actually a member of
        if matches!(source.platform, Platform::GitLab) {
            query.push(("min_access_level", "10".to_string()));
        }

        let req = apply_auth(client.get(&url).query(&query), &source);

        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => return Err(Box::new(e)),
        };

        let status = resp.status();
        let next_page = has_next_page(resp.headers());

        let text = resp.text()?;

        // A non-2xx response means the body is a JSON error object, not an
        // array. Return a readable error rather than letting the Vec parse crash.
        if !status.is_success() {
            return Err(format!("HTTP {} from {}: {}", status, url, text.trim()).into());
        }

        let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;

        if raw.is_empty() {
            break;
        }

        for entry in &raw {
            // GitHub uses "login"; Gitea uses "name" (with "username" as alias);
            // GitLab uses "full_path".
            let name = entry
                .get("login")
                .or_else(|| entry.get("name"))
                .or_else(|| entry.get("username"))
                .or_else(|| entry.get("full_path"))
                .and_then(|json_val| json_val.as_str())
                .map(|str_val| str_val.to_string());

            if let Some(org_name) = name {
                if !orgs
                    .iter()
                    .any(|o: &String| o.eq_ignore_ascii_case(&org_name))
                {
                    orgs.push(org_name);
                }
            }
        }

        if next_page {
            page += 1;
        } else {
            break;
        }
    }

    // ── GitHub supplementary pass ────────────────────────────────────────────
    // GET /user/orgs only returns orgs with public membership (or requires the
    // read:org scope).  Most tokens only carry the `repo` scope, so we also
    // hit GET /user/repos?affiliation=organization_member which works with just
    // `repo` scope and lets us derive every org the token can see.
    if matches!(source.platform, Platform::GitHub) {
        let repos_url = format!("{}/user/repos", base);
        let mut repos_page = 1usize;
        loop {
            let page_size = API_PAGE_SIZE.to_string();
            let req = apply_auth(
                client.get(&repos_url).query(&[
                    ("affiliation", "organization_member"),
                    ("per_page", page_size.as_str()),
                    ("page", &repos_page.to_string()),
                ]),
                &source,
            );

            let resp = match req.send() {
                Ok(r) => r,
                Err(_) => break, // non-fatal — orgs from the first call are still usable
            };

            let next_page = has_next_page(resp.headers());

            let text = match resp.text() {
                Ok(resp_text) => resp_text,
                Err(_) => break,
            };
            let raw: Vec<serde_json::Value> = match serde_json::from_str(&text) {
                Ok(parsed) => parsed,
                Err(_) => break,
            };

            if raw.is_empty() {
                break;
            }

            for repo in &raw {
                // Each repo object has an "owner" sub-object with "login" and "type".
                if let Some(owner) = repo.get("owner") {
                    let owner_type = owner
                        .get("type")
                        .and_then(|json_val| json_val.as_str())
                        .unwrap_or("");
                    let login = owner
                        .get("login")
                        .and_then(|json_val| json_val.as_str())
                        .map(|str_val| str_val.to_string());
                    // Only add Organisation owners, not personal namespaces.
                    if owner_type.eq_ignore_ascii_case("Organization") {
                        if let Some(org_name) = login {
                            if !orgs
                                .iter()
                                .any(|org_entry| org_entry.eq_ignore_ascii_case(&org_name))
                            {
                                orgs.push(org_name);
                            }
                        }
                    }
                }
            }

            if next_page {
                repos_page += 1;
            } else {
                break;
            }
        }
    }

    Ok(orgs)
}

/// Fetch repos the authenticated user collaborates on but doesn't own.
///
/// - GitHub: `GET /user/repos?affiliation=collaborator`
/// - GitLab: `GET /api/v4/projects?membership=true&min_access_level=10`
/// - Gitea:  `GET /api/v1/users/{username}/repos` (all repos accessible to
///           the user; the panel filter in `build_panel_items` excludes
///           owned/org repos)
///
/// Returns one [`RepoDbEntry`] per repo, with `organisation` derived from the
/// owner portion of the repo's full name.
pub fn fetch_collaborator_repos(source_name: &str) -> Result<Vec<RepoDbEntry>, GenericError> {
    let config = load_config()?;
    let source = config
        .source_by_name(source_name)
        .ok_or_else(|| format!("No source '{}' found in config", source_name))?;

    let base = source.api_url.trim_end_matches('/');
    let client = build_client(&source);

    // For Gitea, resolve the authenticated username to build the correct URL.
    // /api/v1/users/{username}/repos is the reliable endpoint across Gitea versions.
    let url = match source.platform {
        Platform::GitHub => format!("{}/user/repos", base),
        Platform::GitLab => format!("{}/api/v4/projects", base),
        // /api/v1/user/repos is the authenticated-user endpoint — it returns
        // all repos the token can see (private + public), unlike the public
        // /users/{username}/repos endpoint which may only list public repos.
        Platform::Gitea => format!("{}/api/v1/user/repos", base),
    };

    let mut entries: Vec<RepoDbEntry> = Vec::new();
    let mut page = 1usize;
    let now = Utc::now().to_rfc3339();

    loop {
        // Gitea uses `limit` (max 50); GitHub and GitLab use `per_page`.
        let mut query: Vec<(&str, String)> = match source.platform {
            Platform::Gitea => vec![("limit", "50".to_string()), ("page", page.to_string())],
            _ => vec![
                ("per_page", API_PAGE_SIZE.to_string()),
                ("page", page.to_string()),
            ],
        };
        match source.platform {
            Platform::GitHub => query.push(("affiliation", "collaborator".to_string())),
            Platform::GitLab => {
                query.push(("membership", "true".to_string()));
                query.push(("min_access_level", "10".to_string()));
            }
            Platform::Gitea => {} // no affiliation filter; build_panel_items de-duplicates
        }

        let req = apply_auth(client.get(&url).query(&query), &source);
        let resp = match req.send() {
            Ok(r) => r,
            Err(e) => return Err(Box::new(e)),
        };

        let status = resp.status();
        let next_page = has_next_page(resp.headers());
        let text = resp.text()?;

        // A non-2xx response (e.g. auth error, unsupported endpoint) returns a
        // JSON object rather than an array — return a readable error instead of
        // letting the Vec parse crash.
        if !status.is_success() {
            return Err(format!("HTTP {} from {}: {}", status, url, text.trim()).into());
        }

        let raw: Vec<serde_json::Value> = serde_json::from_str(&text)?;

        if raw.is_empty() {
            break;
        }

        for repo in &raw {
            let full_name = repo
                .get("full_name")
                .or_else(|| repo.get("path_with_namespace"))
                .and_then(|json_val| json_val.as_str())
                .map(|str_val| str_val.to_string());

            if let Some(address) = full_name {
                let org = address.split('/').next().unwrap_or("").to_string();
                entries.push(RepoDbEntry {
                    address,
                    organisation: org,
                    source: source_name.to_string(),
                    last_refreshed: now.clone(),
                });
            }
        }

        if next_page {
            page += 1;
        } else {
            break;
        }
    }

    Ok(entries)
}

/// Return the total number of issues the authenticated user is involved in.
///
/// - GitHub: queries the search API (`involves:{user} is:issue`).
/// - Gitea:  queries `/api/v1/issues/search` and reads `X-Total-Count`
///   (counts open issues created by the user as a proxy for "involved in").
/// - GitLab: not yet implemented; returns 0.
pub fn fetch_user_issue_count(
    source_name: &str,
    username: &str,
) -> Result<Option<u64>, GenericError> {
    let config = load_config()?;
    let source = config
        .source_by_name(source_name)
        .ok_or_else(|| format!("No source '{}' found in config", source_name))?;

    let base = source.api_url.trim_end_matches('/');
    let client = build_client(&source);

    // Prefer the per-source username; fall back to the caller-supplied value
    // (which comes from the global `config.user`).
    let username = source
        .username
        .as_deref()
        .map(str::trim)
        .filter(|username_str| !username_str.is_empty())
        .unwrap_or(username);

    match source.platform {
        Platform::GitHub => {
            let username = username.trim();
            if username.is_empty() {
                return Err("no GitHub username configured (set 'user' in config)".into());
            }
            let url = format!("{}/search/issues", base);
            // `is:issue` is required alongside `involves:` — GitHub rejects
            // the qualifier alone as too broad and returns 422.
            let search_query = format!("involves:{} is:issue", username);
            let req = apply_auth(
                client
                    .get(&url)
                    .query(&[("q", search_query.as_str()), ("per_page", "1")]),
                &source,
            );
            let resp = req.send()?;
            let status = resp.status();
            let text = resp.text()?;
            if !status.is_success() {
                return Err(format!(
                    "GitHub search API returned HTTP {} for involves:{}: {}",
                    status, username, text
                )
                .into());
            }
            let response_json: serde_json::Value = serde_json::from_str(&text)?;
            Ok(Some(
                response_json
                    .get("total_count")
                    .and_then(|json_val| json_val.as_u64())
                    .unwrap_or(0),
            ))
        }
        // GitLab: no equivalent single-call total — return None so the dashboard shows "N/A".
        Platform::GitLab => Ok(None),
        Platform::Gitea => {
            let username = username.trim();
            if username.is_empty() {
                return Err("no Gitea username configured (set 'user' in config)".into());
            }
            // Gitea's /api/v1/issues/search returns an X-Total-Count header.
            // We use `created_by` as a proxy for "involved in" — Gitea has no
            // single `involves:` qualifier equivalent to GitHub's.
            let url = format!("{}/api/v1/issues/search", base);
            let req = apply_auth(
                client.get(&url).query(&[
                    ("type", "issues"),
                    ("state", "open"),
                    ("created_by", username),
                    ("limit", "1"),
                ]),
                &source,
            );
            match req.send() {
                Ok(resp) => {
                    let count = resp
                        .headers()
                        .get("x-total-count")
                        .and_then(|header_val| header_val.to_str().ok())
                        .and_then(|str_val| str_val.parse::<u64>().ok())
                        .unwrap_or(0);
                    Ok(Some(count))
                }
                Err(e) => Err(Box::new(e)),
            }
        }
    }
}
