use colored::Colorize;
use std::path::Display;
use tabled::settings::Style;
use tabled::{Table, Tabled};

use config_file::FromConfigFile;
use ini::ini;
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use serde_json;

#[derive(Deserialize, Debug)]
struct Config {
    user: String,
    host: String,
    pass: String,
    token: String,
}

#[derive(Deserialize, Debug)]
struct Repo {
    url: String,
    organisation: String,
    repo: String,
}

#[derive(Deserialize, Debug)]
struct Assignee {
    login: String,
}

impl std::fmt::Display for Assignee {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{},", self.login)
    }
}

#[derive(Deserialize, Debug)]
struct AssigneeList(Vec<Assignee>);

impl AssigneeList {
    fn count_string(&self) -> usize {
        let mut counter = 0;
        for i in &self.0 {
            counter += i.login.len() + 1
        }
        counter
    }
}

impl std::fmt::Display for AssigneeList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for v in &self.0 {
            write!(f, "{}", v)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
struct Labels {
    id: u64,
    name: String,
    //colour: String,
    description: String,
}

#[derive(Deserialize, Debug)]
struct LabelsList(Vec<Labels>);

impl std::fmt::Display for LabelsList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for v in &self.0 {
            write!(f, "{},", v.name)?;
        }
        Ok(())
    }
}

impl LabelsList {
    fn count_string(&self) -> usize {
        let mut counter = 0;
        for i in &self.0 {
            counter += i.name.len() + 1
        }
        counter
    }
}

#[derive(Deserialize, Debug, Tabled)]
struct Issue {
    number: u16,
    title: String,
    labels: LabelsList,
    assignees: AssigneeList,
    updated_at: String,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let issue_no = format!("{}", self.number.to_string().blue());
        write!(
            f,
            "| {}\t | {}\t | {} |\n",
            issue_no, self.title, self.assignees
        )
    }
}

struct IssueList(Vec<Issue>);

impl IssueList {
    fn get_max_title(&self) -> (usize, String) {
        let longest_title = &self
            .0
            .iter()
            .map(|x| x.title.clone().len())
            .max()
            .unwrap_or(0);

        let len_title = "-".repeat(longest_title.to_owned() + 4);

        (longest_title.to_owned(), len_title)
    }

    fn get_max_number(&self) -> (usize, String) {
        let widest_number = self
            .0
            .iter()
            .map(|x| x.number.to_string().clone().len())
            .max()
            .unwrap_or(0);

        // Why 5 for a Tab for this data?
        let len_number = "-".repeat(widest_number + 5);

        (widest_number, len_number)
    }

    fn get_max_labels(&self) -> (usize, String) {
        let mut collection = Vec::new();
        for c in &self.0 {
            collection.push(c.labels.count_string());
        }

        // Dont see why this has to be done, when it isn;t required for the previous get_max... functions
        let usized_0 = usize::try_from(0).unwrap();
        let widest_labels = collection.iter().max().unwrap_or(&usized_0);

        let len_labels = "-".repeat(widest_labels.to_owned());

        (widest_labels.to_owned(), len_labels)
    }

    fn get_max_assignee(&self) -> (usize, String) {
        let mut collection = Vec::new();
        for c in &self.0 {
            collection.push(c.assignees.count_string());
        }

        // Dont see why this has to be done, when it isn;t required for the previous get_max... functions
        let usized_0 = usize::try_from(0).unwrap();
        let widest_assignee = collection.iter().max().unwrap_or(&usized_0);
        let len_assignee = "-".repeat(widest_assignee.to_owned());

        (widest_assignee.to_owned(), len_assignee)
    }
}

impl std::fmt::Display for IssueList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "GitHub Issues for {}\n", "SOMEWAY TO FIND THIS")?;

        let (longest_title, len_title) = &self.get_max_title();
        let (longest_number, len_number) = &self.get_max_number();
        let (longest_labels, len_labels) = &self.get_max_labels();
        let (longest_assignee, len_assignee) = &self.get_max_assignee();

        let label_string = format!("{}\t", &len_labels);
        let label_string_formatted = "-".repeat(label_string.len());

        let assignee_string = format!("{}\t", &len_assignee);
        let assignee_string_formatted = "-".repeat(assignee_string.len());

        for v in &self.0 {
            let len_inner_title = v.title.len();
            let len_title_remainder = longest_title - &len_inner_title;
            let title_space = " ".repeat(len_title_remainder);

            let len_inner_label = v.labels.count_string();
            let len_labels_remainder = longest_labels - len_inner_label;
            let label_space = " ".repeat(len_labels_remainder + 1);

            let len_inner_assignee = v.assignees.count_string();
            let len_assignee_remainder = longest_assignee - len_inner_assignee;
            let assignee_space = " ".repeat(len_assignee_remainder + 1);
            write!(
                f,
                "{}{}\t{}{}{}\t{}{}{}{}{}{}{}\n",
                "|".truecolor(105, 105, 105),
                v.number.to_string().blue(),
                "|".truecolor(105, 105, 105),
                v.title,
                title_space,
                "|".truecolor(105, 105, 105),
                v.labels,
                label_space,
                "|".truecolor(105, 105, 105),
                v.assignees,
                assignee_space,
                "|".truecolor(105, 105, 105),
            )?;
        }
        Ok(())
    }
}

fn call_issues(repo_data: Repo, config_data: Config) -> () {
    let bearer = format!("Bearer {}", config_data.token);
    let repo = format!(
        "https://api.github.com/repos/{}/{}/issues",
        repo_data.organisation, repo_data.repo
    );
    let result = Client::new()
        .get(repo)
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

    // Better error messages here
    let issues_struct: Vec<Issue> = serde_json::from_str(&issues).unwrap();

    let issue_list = IssueList { 0: issues_struct };
    println!("{}", issue_list);
}

pub fn issues(config_file: &String, repo: &String, repo_override: &String) -> () {
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

    let _xx = call_issues(repo_data, config);
}
