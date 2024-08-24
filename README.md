# OGit
A Rust program to interact with the GitHub Issues and Repo's

Yes, there are things like gh, octorust and potentially many others, however, I want my own one that I can actually install on ther cluster where I work and that can cache issues when I go offline. This isn't super often but once or twice a week (the bus from work, train to cambridge for example).

I was debating on a TUI using ratatui but it is nice not needing to load it up, exit, do somthing, forget something, load back in.. ad infinum.

## Subcommands

### Authenticate (Planned)
This will be a very simple:
- Add Username
- Add Email
- Add Password
- Add Token - if needed

Could this be replaced by using global git values if available.

### issues
The issues subcommand will:
- enable viewing of issues in your repo (current repo by default, by using an initialised git repo's .git/config file).
  - `ogit issues view`
- enable viewing of issues in other repos, using a repo_override switch.
  - `ogit issues view --repo_override {user/organisation}/{repo}`
- caching of issues for offline reference <-- main reason for making this.
  - Should caching be automatic to be seamless
  - Should it be on flag like:
    - `ogit issues cache [--repo_overide]`
- enable creating an issue for a repo
  - `ogit issues create [--repo_override]`
    - enter a issue building thing
    - checks if online or not
      - tells user if offline, it wll be cached
      - When online use `ogit issues push`

### repos
A subcommand to get some basic overviews of a repo, again defaults to current directories .git/config:
- `ogit repos [--repo_override, --repo_all] --stats [stars, issues, releases, PRs, contributors]`
