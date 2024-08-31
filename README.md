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

Currently, using toml file with:
```
user = "username: String"
token = "token: String
```

I may add other args such as terminal_length here so that there is a default you can control.

### VIEW (In progress)
The issues subcommand will:
- enable viewing of issues in your repo (current repo by default, by using an initialised git repo's .git/config file).
  - `ogit issues`
![Image](assets/images/issues.png?raw=true)

- enable viewing of issues in other repos, using a repo_override switch.
  - `ogit issues view -o/--repo_override {user/organisation}/{repo}`
- caching of issues for offline reference <-- main reason for making this.
  - Should caching be automatic to be seamless?
  - Currently works as: `ogit issues --cache_issues`

- View cached issues with: `ogit issues --from_cache`
    - again, should this be automatic if no internet?

### CREATE (Planned)
    - `git create [--cache_pre / --push]`
    - enter a issue building thing
    - checks if online or not
      - tells user if offline, it wll be cached
      - When online use `ogit push` to throw each pre-issue (issues created locally, not on remote) to the repo.

### INFO (Planned)
    - `ogit info --issue [integer] --fields [default: all, status, title, body, assignees, labels, comments]
    - Printing all comments may cause an issue...
    - Should be cached
    - Get the details of an issue

### REPO (Planned)
A subcommand to get some basic overviews of a repo, again defaults to current directories .git/config:
    - `ogit repo [--repo_override, --repo_all] --stats [stars, issues, releases, PRs, contributors]`
