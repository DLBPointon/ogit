use crate::processors::issue_structs::IssueList;

use crate::processors::from_cache::print_cache;

use super::generics::build_github_call;
use super::get_comments::get_comments;

const GITHUB_API: &str = "https://api.github.com/repos";

fn info_from_cache(issue: &u16, issue_json: IssueList) {
    issue_json.filter_on_issue(issue)
}

pub fn info_issues(
    issue: &u16,
    comments: &bool,
    _debug: &bool,
    from_cache: &bool,
    config_file: &String,
    repo: &String,
    repo_override: &String,
) {
    if from_cache.eq(&true) {
        let cached_data = print_cache();
        match cached_data {
            Ok(d) => {
                info_from_cache(issue, d);
            }
            Err(_) => println!("Dang"),
        }
    } else {
        let (repo_data, config) = build_github_call(config_file, repo_override, repo);

        if comments.eq(&true) {
            let comment_url = format!(
                "{}/{}/{}/issues/{}/comments",
                GITHUB_API, repo_data.organisation, repo_data.repo, issue
            );
            get_comments(comment_url, config);
        }
    }
}
