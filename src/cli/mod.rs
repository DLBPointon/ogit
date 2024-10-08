use clap::{Args, Parser, Subcommand};

const OGIT_LOGO: &str = "
=====================================
   _______  _______  ___   _______
  |       ||       ||   | |       |
  |   _   ||    ___||   | |_     _|
  |  | |  ||   | __ |   |   |   |
  |  |_|  ||   ||  ||   |   |   |
  |       ||   |_| ||   |   |   |
  |_______||_______||___|   |___|
=====================================
";

// CLI for GIT parsing
#[derive(Parser, Debug)]
#[command(name = "OGit", author="Damon-Lee Pointon (DLBPointon)", version="v1.0.0", about = format!("{}\nA simple program for playing with GitHub Issues both On and Offline", OGIT_LOGO), long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub global_args: GlobalArgs,

    // command is optional (TODO: Make this not optional)
    // Reference: https://docs.rs/clap/latest/clap/_derive/_tutorial/chapter_2/index.html#defaults
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Args, Clone, Debug)]
#[group(required = false, multiple = true)]
pub struct GlobalArgs {
    #[arg(
        short = 'd',
        long = "debug",
        default_value_t = false,
        help = "It's Debugging time!"
    )]
    pub debug: bool,

    // Print explainers as to why validation fails, if it does fail
    #[arg(short = 'r', long = "repo", default_value = ".git/config")]
    pub repo: String,

    // Default git config for current dir
    #[arg(
        short = 'o',
        long = "overide_repo",
        default_value = "-NA-",
        help = "Override -r and get deets from another repo"
    )]
    pub repo_override: String,

    #[arg(
        long = "from_cache",
        default_value_t = false,
        help = "Print data from cache rather than attempt reaching GitHub."
    )]
    pub from_cache: bool,
}

// Reference: https://docs.rs/clap/latest/clap/_derive/_tutorial/chapter_2/index.html
#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(
        name = "view",
        about = format!("{}\nTabulate the issues associated with a GitHub repository.\nThis can also cache issue information.", OGIT_LOGO),
        long_about = None
    )]
    View {
        // Path to the TreeVal yaml file generated by the user
        #[arg(
            short = 'c',
            long,
            default_value = "./etc/config.toml",
            help = "Path to toml file containing GitHub auth data"
        )]
        config_file: String,

        // THIS SHOULD HAVE A DEFAULT OF 20
        #[arg(
            short = 't',
            long = "terminal_length",
            default_value_t = 40,
            value_parser = clap::value_parser!(u16).range(11..100),
            help="length of output - Currently only effects the title values"
        )]
        // Clap value parser stops values smaller than "ISSUE TITLE"
        // which is the column header in the output.
        // Going smaller than the header causes alignment issue which
        // would be a pain to try and fix
        terminal_length: u16,

        // Cache flag
        // mutually exclusive with from_cache
        #[arg(
            long = "cache_issues",
            default_value_t = false,
            help = "Cache issues for project under .git/issue_cache.json. NOTE: this will always overwrite existing file."
        )]
        cache_issues: bool,
    },
    #[command(
        name = "info",
        about = format!("{}\nPrint the details of a specific issue, can also print the comments (INTERNET CONNECTION REQUIRED).", OGIT_LOGO)
    )]
    Info {
        #[arg(
            short = 'i',
            long = "issue_number",
            required = true,
            help = "The issue number of the issue you want to know more about, you can use multiples of -i <int>!"
        )]
        issue_number: Vec<u16>,

        #[arg(
            long = "comments",
            default_value_t = false,
            help = "View the comments attached to issue",
            requires = "issue_number"
        )]
        comments: bool,

        #[arg(
            long = "labels",
            help = "A CSV list of issue labels you want the output filtered by.",
            default_value = "",
            requires = "issue_number"
        )]
        labels: String,

        // Path to the TreeVal yaml file generated by the user
        #[arg(
            short = 'c',
            long,
            default_value = "/Users/dp24/Documents/ogit/etc/config.toml"
        )]
        config_file: String,
    },
}
