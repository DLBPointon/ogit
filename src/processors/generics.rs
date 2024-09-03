use crate::processors::issue_structs::{Config, Repo};
use ansi_term::Style;
use config_file::FromConfigFile;
use ini::ini;

pub fn check_terminal_length_arg(tl: &usize) -> usize {
    if tl < &"ISSUE TITLE".to_string().len() {
        let error_msg = Style::new()
            .bold()
            .underline()
            .paint("Teminal length < 11 not allowed. Defaulting to 11");
        // Better to be from the actual log crate?
        println!("WARNING: {error_msg}");
        // This is the minimum length I want to support,
        // It is the length of the "ISSUE TITLE" header for the output table
        11
    } else {
        tl.to_owned()
    }
}

pub fn build_github_call(
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
