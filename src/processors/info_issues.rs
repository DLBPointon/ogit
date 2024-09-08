use super::generics::build_github_call;
use super::get_comments::get_comments;
use crate::cli::GlobalArgs;
use crate::processors::from_cache::print_cache;
use crate::processors::issue_structs::{Config, Issue, IssueList, Repo};
use reqwest::blocking::Client;

const GITHUB_API: &str = "https://api.github.com/repos";

fn info_from_cache(issue: &u16, mut issue_json: IssueList) {
    // fix data normally takes terminal length in the issue_utils mod
    // but here we want detailed information so set to max
    issue_json.fix_data(&100).filter_on_issue(issue)
}

fn call_issues(repo_data: &Repo, config_data: &Config, debug: &bool) -> IssueList {
    let bearer = format!("Bearer {}", config_data.token);
    let repo = format!(
        "https://api.github.com/repos/{}/{}/issues",
        repo_data.organisation, repo_data.repo
    );
    let result = Client::new()
        .get(&repo)
        .query(&[
            ("filter", "all"),
            ("orgs", &config_data.user),
            ("sort", &"updated".to_string()),
            ("per_page", &"100".to_string()),
            ("page", &"1".to_string()), // Now see whether we get the link field
        ])
        .header("Accept", "application/vnd.github.raw+json")
        .header("Connection", "keep-alive")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "request")
        .header("Authorization", bearer)
        .send()
        .unwrap();

    // IF THERE'S A PAGE 2.. THEN THOSE ISSUES NEED TO BE INCLUDED
    // THIS WOULD DIRECTLY USE THE RESULT WHICH CONTAINS THE HEADERS AND MAYBE LINKS TO PAGE2
    // IF THERE ARE ENOUGH ISSUES TO CAUSE A PAGE 2
    let issues = &result.text().unwrap().to_owned();

    // You can use these like f-strings? But sometimes?
    if debug.eq(&true) {
        println!("{issues:?} \n Raw response text data is /\\")
    };

    // Better error messages here
    // how would we get a "fields x not in data"
    let issues_struct: Vec<Issue> = serde_json::from_str(&issues).unwrap();

    if issues_struct.len() <= 0 {
        panic!("There are no issues!")
    }

    let issue_list = IssueList {
        issue_data: issues_struct,
        meta_data: format!("{}/{}", repo_data.organisation, repo_data.repo),
    };

    issue_list
}

pub fn info_issues(
    issues_number_vec: &Vec<u16>,
    comments: &bool,
    labels: &String,
    config_file: &String,
    globals: &GlobalArgs,
) {
    if globals.debug.eq(&true) {
        println!("DEBUGGING")
    }

    if globals.from_cache.eq(&true) {
        for issue in issues_number_vec {
            let cached_data = print_cache();
            match cached_data {
                Ok(d) => {
                    info_from_cache(issue, d);
                }
                Err(_) => println!("Dang"),
            }
        }
    } else {
        let (repo_data, config) =
            build_github_call(config_file, &globals.repo_override, &globals.repo);
        let issue_data = call_issues(&repo_data, &config, &globals.debug);

        for issue_number in issues_number_vec {
            issue_data.filter_on_issue(issue_number); // This prints the object

            if comments.eq(&true) {
                let comment_url = format!(
                    "{}/{}/{}/issues/{}/comments",
                    GITHUB_API, repo_data.organisation, repo_data.repo, issue_number
                );
                get_comments(comment_url, config.to_owned());
            }
        }
    }
}
