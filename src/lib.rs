use clap::Parser;

use cli::{Cli, Commands};
use std::io::Error;

use crate::processors::info_issues::info_issues;
use crate::processors::issue_utils::view_issues;

mod cli;
mod processors;

pub fn run() -> Result<(), Error> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::View {
            config_file,
            repo,
            terminal_length,
            repo_override,
            cache_issues,
            from_cache,
            debug,
        }) => view_issues(
            config_file,
            repo,
            repo_override,
            terminal_length,
            cache_issues,
            from_cache,
            debug,
        ),
        Some(Commands::Info {
            issue_number,
            comments,
            debug,
            from_cache,
            config_file,
            repo,
            repo_override,
        }) => info_issues(
            issue_number,
            comments,
            debug,
            from_cache,
            config_file,
            repo,
            repo_override,
        ),
        None => {
            println!("No command provided");
        }
    }
    Ok(())
}
