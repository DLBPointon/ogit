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
            terminal_length,
            cache_issues,
        }) => view_issues(config_file, terminal_length, cache_issues, &cli.global_args),
        Some(Commands::Info {
            issue_number,
            comments,
            labels,
            config_file,
        }) => info_issues(
            issue_number,
            comments,
            labels,
            config_file,
            &cli.global_args,
        ),
        None => {
            println!("No command provided");
        }
    }
    Ok(())
}
