use clap::Parser;

use cli::{Cli, Commands};
use std::io::Error;

use crate::processors::issue_utils::issues;

mod cli;
mod processors;

pub fn run() -> Result<(), Error> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Issues {
            config_file,
            repo,
            repo_override,
        }) => issues(config_file, repo, repo_override),
        None => {
            println!("No command provided");
        }
    }
    Ok(())
}
