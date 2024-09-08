use super::issue_structs::Creator;
use crate::processors::issue_structs::Config;
use ansi_term::Style;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs::File;
use std::io::Write;

#[derive(Deserialize, Debug, Serialize, Clone)]
struct Comments {
    id: u32,
    user: Creator,
    author_association: String,
    created_at: String,
    updated_at: String,
    body: String,
}

impl std::fmt::Display for Comments {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let comment_id = format!(
            "{}  {}",
            Style::new().underline().bold().paint("Comment no.:"),
            self.id
        );
        let creator_line = format!(
            "{} {} {}",
            Style::new().underline().bold().paint("Comment User:"),
            self.user,
            self.author_association
        );

        let datetime_line = format!(
            "{}   {}\t{} {}",
            Style::new().underline().bold().paint("Created on:"),
            self.created_at,
            Style::new().underline().bold().paint("Updated:"),
            self.updated_at
        );

        let body_line = format!(
            "{} {}",
            Style::new().underline().bold().paint("Comment Body:"),
            self.body
        );
        write!(
            f,
            "{}\n{}\n{}\n{}",
            comment_id, datetime_line, creator_line, body_line
        )?;
        Ok(())
    }
}

#[derive(Deserialize, Debug, Serialize, Clone)]
pub struct CommentList {
    comments: Vec<Comments>,
}

impl std::fmt::Display for CommentList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for comment in &self.comments {
            let comment_id = format!(
                "{} {}",
                Style::new().underline().bold().paint("Comment no.:"),
                comment.id
            );
            let creator_line = format!(
                "{} {}\t{} {}",
                Style::new().underline().bold().paint("User:"),
                comment.user,
                Style::new().underline().bold().paint("User Association:"),
                comment.author_association
            );
            write!(f, "{}\n{}", comment_id, creator_line)?
        }
        Ok(())
    }
}

fn save_to_json(data: CommentList) -> Result<(), std::io::Error> {
    let json_data = serde_json::to_string(&data)?;
    let mut file = File::create(".git/issue_cache.json")?;
    file.write_all(json_data.as_bytes())?;

    Ok(())
}

pub fn get_comments(comment_url: String, config_data: Config) {
    let bearer = format!("Bearer {}", config_data.token);

    let result = Client::new()
        .get(&comment_url)
        .header("Accept", "application/vnd.github.raw+json")
        .header("Connection", "keep-alive")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "request")
        .header("Authorization", bearer)
        .send()
        .unwrap();

    let comments = result.text().unwrap();

    let comments_struct: Vec<Comments> = serde_json::from_str(&comments).unwrap();

    let comment_list = CommentList {
        comments: comments_struct,
    };

    if comment_list.comments.len() == 0 {
        println!(
            "{} |\n\tNO COMMENTS AT: {}",
            Style::new().underline().bold().paint("Comments:"),
            comment_url
        )
    } else if comment_list.comments.len() <= 3 {
        for i in comment_list.comments {
            println!(
                "{}: |\n\t{}\n",
                Style::new().underline().bold().paint("Comments:"),
                i
            )
        }
    } else {
        let output_location = "comments.json";
        println!("MORE THAN 3 COMMENTS\nCould mean that there is a comment chain so i'll output to file: {}", output_location);

        let _x = save_to_json(comment_list);
    }
}
