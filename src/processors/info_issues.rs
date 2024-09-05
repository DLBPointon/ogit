use crate::cli::GlobalArgs;
use crate::processors::issue_structs::IssueList;

use crate::processors::from_cache::print_cache;

use super::generics::build_github_call;
use super::get_comments::get_comments;

const GITHUB_API: &str = "https://api.github.com/repos";

fn info_from_cache(issue: &u16, mut issue_json: IssueList) {
    // fix data normally takes terminal length in the issue_utils mod
    // but here we want detailed information so set to max
    issue_json.fix_data(&100).filter_on_issue(issue)
}

pub fn info_issues(issues: &Vec<u16>, comments: &bool, config_file: &String, globals: &GlobalArgs) {
    if globals.debug.eq(&true) {
        println!("DEBUGGING")
    }

    for issue in issues {
        if globals.from_cache.eq(&true) {
            let cached_data = print_cache();
            match cached_data {
                Ok(d) => {
                    info_from_cache(issue, d);
                }
                Err(_) => println!("Dang"),
            }
        } else {
            let (repo_data, config) =
                build_github_call(config_file, &globals.repo_override, &globals.repo);

            if comments.eq(&true) {
                let comment_url = format!(
                    "{}/{}/{}/issues/{}/comments",
                    GITHUB_API, repo_data.organisation, repo_data.repo, issue
                );
                get_comments(comment_url, config);
            }
        }
    }
}
