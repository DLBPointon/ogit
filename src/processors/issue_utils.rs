use crate::processors::from_cache::print_cache;
use crate::processors::structs::{Config, Issue, IssueList, Repo};
use config_file::FromConfigFile;
use ini::ini;
use reqwest::blocking::Client;
use serde_json;
use std::fs::File;
use std::io::Write;

fn call_issues(repo_data: Repo, config_data: Config) -> IssueList {
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
            ("page", "1"),
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
    //println!("{:?}", issues);

    // Better error messages here
    let issues_struct: Vec<Issue> = serde_json::from_str(&issues).unwrap();

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

fn build_github_call(
    config_file: &String,
    repo_override: &String,
    repo: &String,
) -> (Repo, Config) {
    let config = Config::from_config_file(config_file).unwrap();

    let repo_data = if repo_override.to_owned() == "-NA-".to_string() {
        let ini_file = ini!(repo);
        let actual_repo = ini_file["remote \"origin\""]["url"].clone().unwrap();
        let repo_split = &actual_repo.split("/").collect::<Vec<&str>>();
        let org = repo_split[repo_split.len() - 2];
        let repo = repo_split[repo_split.len() - 1]
            .split(".")
            .collect::<Vec<&str>>()[0];

        let x = Repo {
            url: actual_repo.to_owned(),
            organisation: org.to_string(),
            repo: repo.to_string(),
        };

        x
    } else {
        let override_split = repo_override.split("/").collect::<Vec<&str>>();

        let y = Repo {
            url: format!(
                "https://github.com/{}/{}.git",
                override_split[0], override_split[1]
            ),
            organisation: override_split[0].to_owned(),
            repo: override_split[1].to_owned(),
        };

        y
    };

    (repo_data, config)
}

pub fn view_issues(
    config_file: &String,
    repo: &String,
    repo_override: &String,
    terminal_length: &usize,
    cache_bool: &bool,
    from_cache: &bool,
) -> () {
    if from_cache.eq(&true) {
        let xx = print_cache();
        match xx {
            Ok(d) => {
                println!("{}", d)
            }
            Err(e) => {
                println!("{}", e)
            }
        }
    } else {
        let (repo_data, config) = build_github_call(config_file, repo_override, repo);
        let mut full_issue_data = call_issues(repo_data, config);

        if cache_bool.eq(&true) {
            // Should absolutely be done better!
            let _xx = save_to_json(full_issue_data);
        } else {
            println!("{}", full_issue_data.trim_titles(terminal_length))
        }
    }
}
