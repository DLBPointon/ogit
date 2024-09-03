use crate::processors::from_cache::print_cache;
use crate::processors::generics::{build_github_call, check_terminal_length_arg};
use crate::processors::issue_structs::{Config, Issue, IssueList, Repo};
use reqwest::blocking::Client;
use serde_json;
use std::fs::File;
use std::io::Write;

fn call_issues(repo_data: Repo, config_data: Config, debug: &bool) -> IssueList {
    let bearer = format!("Bearer {}", config_data.token);
    let repo = format!(
        "https://api.github.com/repos/{}/{}/issues",
        repo_data.organisation, repo_data.repo
    );
    let result = Client::new()
        .get(&repo)
        .query(&[
            ("filter", "created"),
            ("state", "open"),
            ("orgs", &config_data.user),
            ("sort", "updated"),
            ("per_page", "100"),
            //("page", "1"), // Now see whether we get the link field
        ])
        .header("Accept", "application/vnd.github.raw+json")
        .header("Connection", "keep-alive")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "request")
        .header("Authorization", bearer)
        .send()
        .unwrap();
    // IF THERE'S A PAGE 2.. THEN THOSE ISSUES NEED TO BE INCLUDED

    let issues = result.text().unwrap();

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

fn save_to_json(data: IssueList) -> Result<(), std::io::Error> {
    let json_data = serde_json::to_string(&data)?;
    let mut file = File::create(".git/issue_cache.json")?;
    file.write_all(json_data.as_bytes())?;

    Ok(())
}

pub fn view_issues(
    config_file: &String,
    repo: &String,
    repo_override: &String,
    terminal_length: &usize,
    cache_bool: &bool,
    from_cache: &bool,
    debug: &bool,
) -> () {
    let terminal_length = check_terminal_length_arg(terminal_length);
    if from_cache.eq(&true) {
        let xx = print_cache();
        match xx {
            Ok(mut d) => {
                println!("{}", d.trim_titles(&terminal_length))
            }
            Err(e) => {
                println!("{}", e)
            }
        }
    } else {
        let (repo_data, config) = build_github_call(config_file, repo_override, repo);
        let mut full_issue_data = call_issues(repo_data, config, debug);

        if cache_bool.eq(&true) {
            // Should absolutely be done better!
            let _xx = save_to_json(full_issue_data);
        } else {
            println!("{}", full_issue_data.trim_titles(&terminal_length))
        }
    }
}
