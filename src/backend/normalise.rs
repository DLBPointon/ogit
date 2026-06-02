use crate::model::{Comment, GenericError, Issue, Platform};
use serde_json;

/// Remap the GitLab `author` field to a GitHub-compatible `user` object.
/// GitLab returns `author.username`; GitHub expects `user.login`.
/// This block appeared verbatim in both `normalise_issue` and
/// `normalise_comment` — extracted here to remove the duplication.
fn remap_gitlab_author(obj: &mut serde_json::Map<String, serde_json::Value>) {
    if obj.get("user").is_none() {
        if let Some(author) = obj.remove("author") {
            let user = if let serde_json::Value::Object(mut a) = author {
                if let Some(username) = a.remove("username") {
                    a.insert("login".to_string(), username);
                }
                serde_json::Value::Object(a)
            } else {
                serde_json::json!({ "login": "" })
            };
            obj.insert("user".to_string(), user);
        }
    }
}

pub(super) fn normalise_issue(
    mut obj: serde_json::Map<String, serde_json::Value>,
    platform: &Platform,
    api_base: &str,
    owner: &str,
    repo_name: &str,
) -> Result<Issue, GenericError> {
    match platform {
        Platform::GitHub | Platform::Gitea => {
            let issue: Issue = serde_json::from_value(serde_json::Value::Object(obj))?;
            Ok(issue)
        }
        Platform::GitLab => {
            if let Some(iid) = obj.remove("iid") {
                obj.entry("number".to_string()).or_insert(iid);
            }
            if let Some(desc) = obj.remove("description") {
                obj.entry("body".to_string()).or_insert(desc);
            }
            remap_gitlab_author(&mut obj);
            if let Some(count) = obj.remove("user_notes_count") {
                obj.entry("comments".to_string()).or_insert(count);
            }
            if let Some(url) = obj.remove("web_url") {
                obj.entry("url".to_string()).or_insert(url);
            }
            if let Some(state) = obj.get_mut("state") {
                if state == "opened" {
                    *state = serde_json::Value::String("open".to_string());
                }
            }
            if let Some(serde_json::Value::Array(labels)) = obj.get("labels").cloned() {
                let normalised: Vec<serde_json::Value> = labels
                    .iter()
                    .map(|label_val| match label_val {
                        serde_json::Value::String(name) => {
                            serde_json::json!({"name": name, "color": "", "description": ""})
                        }
                        other => other.clone(),
                    })
                    .collect();
                obj.insert("labels".to_string(), serde_json::Value::Array(normalised));
            }
            let iid = obj
                .get("number")
                .and_then(|json_val| json_val.as_u64())
                .unwrap_or(0);
            let encoded = format!("{}%2F{}", owner, repo_name);
            let comments_url = format!(
                "{}/api/v4/projects/{}/issues/{}/notes",
                api_base.trim_end_matches('/'),
                encoded,
                iid
            );
            obj.insert(
                "comments_url".to_string(),
                serde_json::Value::String(comments_url),
            );

            let issue: Issue = serde_json::from_value(serde_json::Value::Object(obj))?;
            Ok(issue)
        }
    }
}

pub(super) fn normalise_comment(
    mut obj: serde_json::Map<String, serde_json::Value>,
    platform: &Platform,
) -> Result<Comment, GenericError> {
    match platform {
        Platform::GitHub | Platform::Gitea => {
            let comment: Comment = serde_json::from_value(serde_json::Value::Object(obj))?;
            Ok(comment)
        }
        Platform::GitLab => {
            remap_gitlab_author(&mut obj);
            let comment: Comment = serde_json::from_value(serde_json::Value::Object(obj))?;
            Ok(comment)
        }
    }
}
