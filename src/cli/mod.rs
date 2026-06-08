//! Command-line interface definition.
//!
//! Uses `clap` for argument parsing. Defines two modes of operation:
//! - **TUI mode**: Default behavior; launches the interactive terminal UI
//! - **JSON mode**: `--json` flag; exports issue data and exits without TUI
//!
//! Additional features:
//! - Repository filtering with `--repo` (configurable or discovered)
//! - Issue selection with `--issue` (single or multiple)
//! - Optional comment fetching with `--with-comments`
//! - `clone` subcommand for repository cloning

use clap::{Parser, Subcommand};

/// The ASCII art logo used in the TUI dashboard and CLI header.
pub const OGIT_LOGO: &str = r#"
   _______  _______  ___   _______
  |       ||       ||   | |       |
  |   _   ||    ___||   | |_     _|
  |  | |  ||   | __ |   |   |   |||
  |  |_|  ||   ||  ||   |   |   |||
  |       ||   |_| ||   |   |   |||
  |_______||_______||___|   |___|||
"#;

/// Main CLI argument structure.
///
/// Defines all supported command-line options and subcommands.
/// The default behavior (without subcommands) launches the TUI.
/// Use `--json` for non-interactive JSON export mode.
#[derive(Parser, Debug)]
#[command(
    name = "OGit",
    author = "Damon-Lee Pointon (DLBPointon)",
    version = clap::crate_version!(),
    about = format!("{}\nA simple program for playing with GitHub Issues both On and Offline", OGIT_LOGO),
    long_about = format!(
        "{}\nA TUI and CLI tool for browsing and exporting GitHub, GitLab, and Gitea issues.\n\n\
        Run without arguments to launch the TUI. Use --repo and --issue to jump straight \
        to a specific issue on startup, or combine --json to export issue data to stdout \
        without opening the TUI.",
        OGIT_LOGO
    ),
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Repo to open on startup (name, address, or owner/repo).
    ///
    /// Accepts the short name or full address of any repo that is either
    /// configured in ~/.ogit/config.json or has been discovered via org
    /// sync.  Overrides the automatic detection of a repo from the current
    /// working directory.
    ///
    /// Examples:
    ///   --repo my-repo
    ///   --repo owner/my-repo
    #[arg(long = "repo", value_name = "NAME")]
    pub repo: Option<String>,

    /// Issue number(s) to open or export (use with --repo).
    ///
    /// In TUI mode only the first number is used — the TUI will jump
    /// directly to that issue on startup.
    ///
    /// In JSON mode (--json) every listed number is fetched and included
    /// in the output.  A single issue produces a plain object; multiple
    /// issues produce a JSON array.
    ///
    /// Examples:
    ///   --issue 42
    ///   --issue 1 2 3
    #[arg(long = "issue", value_name = "NUMBER", num_args = 1..)]
    pub issue: Vec<u32>,

    /// Print issue data as JSON to stdout and exit (no TUI).
    ///
    /// Requires --repo and at least one --issue.  Useful for scripting or
    /// piping issue data into other tools.
    ///
    /// Output format:
    ///   Single issue  → { "issue": { … } }
    ///   Multiple      → [ { "issue": { … } }, … ]
    ///
    /// Add --with-comments to include the full comment thread for each issue.
    #[arg(long = "json", requires = "repo")]
    pub json: bool,

    /// Include the full comment thread in --json output.
    ///
    /// Fetches all comment pages for every requested issue and adds a
    /// "comments" array to each issue object in the output.
    /// Has no effect without --json.
    #[arg(long = "with-comments", requires = "json")]
    pub with_comments: bool,
}

/// Subcommands for ogit.
///
/// Currently only `clone` is supported. Subcommands are processed
/// inside the TUI for now; `--repo` and `--issue` can be used with subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Clones a repository and registers it in configuration.
    ///
    ///
    /// Clones the repo to <clone_root>/<owner>/<repo> (where clone_root is
    /// set in your config, defaulting to ~/code), then adds an entry so it
    /// appears in the TUI repo panel immediately.
    Clone {
        /// Repository to clone — URL or owner/repo shorthand.
        ///
        /// Accepted formats:
        ///   https://github.com/owner/repo.git
        ///   git@github.com:owner/repo.git
        ///   owner/repo
        repo: String,

        /// Directory to clone into instead of the default clone root.
        ///
        /// When omitted the repo is placed under the clone_root defined in
        /// ~/.ogit/config.json (default: ~/code/<owner>/<repo>).
        #[arg(long = "dir", value_name = "PATH")]
        dir: Option<String>,

        /// Display name to store in config for this repo.
        ///
        /// Useful when you want to add the same address more than once
        /// under different names, or when the auto-derived name is not
        /// descriptive enough.  Defaults to the repo part of the address.
        #[arg(long = "name", value_name = "NAME")]
        name: Option<String>,

        /// Accent colour for this repo in the TUI panel.
        ///
        /// Any CSS colour value is accepted.
        ///
        /// Examples:
        ///   --colour '#e06c75'
        ///   --colour 'tomato'
        ///   --colour 'ff0000'
        #[arg(long = "colour", value_name = "COLOUR")]
        colour: Option<String>,
    },
}
