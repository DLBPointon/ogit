use crate::cli::GlobalArgs;
use crate::processors::from_cache::print_cache;
use crate::processors::generics::build_github_call;
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
            ("filter", "all"),
            //("state", "open"),
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

    // match result {OK(proceed), Err(force from cache)}
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

fn save_to_json(data: IssueList) -> Result<(), std::io::Error> {
    let json_data = serde_json::to_string(&data)?;

    // TODO: This should be based on where the script is called from!!!!
    let mut file = File::create(".git/issue_cache.json")?;
    file.write_all(json_data.as_bytes())?;

    Ok(())
}

pub fn view_issues(
    config_file: &String,
    terminal_length: &u16,
    cache_issues: &bool,
    globals: &GlobalArgs,
) -> () {
    if globals.debug.eq(&true) {
        println!("DEBUGGING")
    }

    if globals.from_cache.eq(&true) {
        let xx = print_cache();
        match xx {
            Ok(mut d) => {
                println!("{}", d.fix_data(terminal_length))
            }
            Err(e) => {
                println!("{}", e)
            }
        }
    } else {
        let (repo_data, config) =
            build_github_call(config_file, &globals.repo_override, &globals.repo);
        let mut full_issue_data = call_issues(repo_data, config, &globals.debug);

        if cache_issues.eq(&true) {
            // Should absolutely be done better!
            let _xx = save_to_json(full_issue_data);
        } else {
            println!("{}", full_issue_data.fix_data(terminal_length))
        }
    }
}
