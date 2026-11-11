//! What the forge knows and the repository does not: the pull requests.
//!
//! Git stores nothing about them. A branch that is merely up on the remote and
//! a branch somebody is reviewing look exactly alike from here, so the only
//! way to tell them apart is to go and ask.
//!
//! The asking goes through the forge's own command line tool -- `gh` for
//! GitHub, `glab` for GitLab -- because it already holds the credentials and
//! already knows which repository this is. Shelling out to a tool the user
//! has configured is what gitrust does for credential helpers too: there is
//! no second copy of their settings to keep in step.
//!
//! Which of the two, and as whom, comes from the profile in use: the account
//! whose host matches the remote's decides it. With no profile, or none that
//! names this host, the host's own name decides -- and anything that is not
//! plainly GitLab is asked of `gh`, which is what every repository here was
//! asked before there were profiles.
//!
//! A merge request is a pull request under another name, and the type below
//! is called after GitHub's because that is the one the board was written
//! for. Everything a column needs is the same in both.
//!
//! Nothing here is required. With no tool, no login, or a remote on neither
//! forge, this fails with a sentence and the board goes back to reading the
//! graph on its own. What it must never do is guess: a pull request that is
//! invented is worse than one that is missing.

use std::collections::HashMap;
use std::process::Command;

use anyhow::{Context, Result};

use crate::profile::{host_of, Forge};
use crate::repo::Repo;

/// Where a pull request stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestState {
    /// Open, and so waiting for somebody.
    Open,
    /// Its work is in the main branch, however it got there.
    Merged,
    /// Closed without being merged.
    Closed,
}

/// A pull request, as much of it as a column needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u32,
    pub state: PullRequestState,
    /// A draft is the author saying it is not ready to be looked at.
    pub draft: bool,
    pub url: String,
}

impl PullRequest {
    /// Which of two pull requests on the same branch is the one that counts.
    ///
    /// A branch can carry several over its life: one merged long ago, one
    /// closed without going anywhere, one open right now. The open one is what
    /// the branch is doing; failing that, having been merged is the most it
    /// ever achieved. Lower wins.
    fn rank(&self) -> u8 {
        match self.state {
            PullRequestState::Open => 0,
            PullRequestState::Merged => 1,
            PullRequestState::Closed => 2,
        }
    }
}

/// The pull requests of a repository, by the branch each one comes from.
pub type PullRequests = HashMap<String, PullRequest>;

/// How many to ask for. Past this the oldest stop coming back, and the oldest
/// are the ones whose branches have long since been deleted.
const LIMIT: &str = "400";

/// The same, for GitLab, which counts in pages and will not hand out more
/// than a hundred in one.
const PAGE: &str = "100";

/// Which tool answers for a repository, where it is, and with what.
///
/// It is worked out once per asking rather than stored: a profile can be
/// switched between one press and the next, and an answer cached from the
/// last client is the wrong answer given confidently.
struct Asking {
    forge: Forge,
    /// The host the remote is on, which is what tells a company's own GitLab
    /// from gitlab.com. Empty when the repository has no remote at all.
    host: String,
    /// The account's token, if it has one. Empty leaves the tool to use
    /// whatever it is logged in as, which is what it did before profiles.
    token: String,
}

impl Asking {
    /// The tool, with the account's host and token in its environment.
    ///
    /// Environment rather than configuration: the tools both read these, and
    /// passing them this way means gitrust never writes into a file that
    /// belongs to another program.
    fn command(&self) -> Command {
        let mut command = Command::new(self.forge.tool());
        let (host_var, token_var) = match self.forge {
            Forge::GitHub => ("GH_HOST", "GH_TOKEN"),
            Forge::GitLab => ("GITLAB_HOST", "GITLAB_TOKEN"),
        };
        if !self.host.is_empty() {
            command.env(host_var, &self.host);
        }
        if !self.token.is_empty() {
            command.env(token_var, &self.token);
        }
        if self.forge == Forge::GitLab {
            // Nobody is at the other end of a prompt here: a question would
            // be a thread sitting for ever on an answer that never comes.
            command.env("GLAB_NO_PROMPT", "1");
        }
        command
    }
}

impl Repo {
    /// The address of the remote this repository is read against.
    ///
    /// "origin" when there is one, and otherwise the first remote there is:
    /// a repository with a single remote called something else is somebody's
    /// deliberate setup, not a repository with no forge.
    fn remote_url(&self) -> Option<String> {
        let remotes = self.inner.remotes().ok()?;
        let names: Vec<&str> = remotes.iter().flatten().collect();
        let name = names
            .iter()
            .find(|name| **name == "origin")
            .or_else(|| names.first())?;
        self.inner.find_remote(name).ok()?.url().map(str::to_string)
    }

    /// Which forge to ask about this repository, and as whom.
    fn asking(&self) -> Asking {
        let url = self.remote_url().unwrap_or_default();
        let host = host_of(&url).unwrap_or_default();

        // An account for this host settles it outright: that is the whole
        // point of writing one down.
        if let Some(account) = self.profile.as_ref().and_then(|profile| profile.account_on(&host)) {
            return Asking {
                forge: account.forge,
                host: account.host.clone(),
                token: account.token.clone(),
            };
        }

        // Failing that, the host's own name. GitHub is what is left, because
        // every repository that got here before there were profiles was
        // asked of `gh` and a guess that changes that is a regression.
        let forge = match host.starts_with("gitlab.") {
            true => Forge::GitLab,
            false => Forge::GitHub,
        };
        Asking { forge, host, token: String::new() }
    }

    /// Asks the forge which pull requests this repository has.
    ///
    /// It goes out to the network, so it belongs on a thread of its own. It is
    /// also the one thing in the whole crate that does not come out of the
    /// repository, which is why it is a separate call and not part of
    /// [`Repo::board`]: the board is drawn without waiting for it, and redrawn
    /// when the answer arrives.
    pub fn pull_requests(&self) -> Result<PullRequests> {
        let asking = self.asking();
        let tool = asking.forge.tool();

        // The two tools are asked the same question and told to answer in
        // the same shape: one line per request, tab separated, in the order
        // `parse` below reads them. Both carry a jq of their own, so the
        // shaping costs no second program and no JSON reader in here.
        let args: Vec<&str> = match asking.forge {
            Forge::GitHub => vec![
                "pr",
                "list",
                "--state",
                "all",
                "--limit",
                LIMIT,
                "--json",
                "number,state,isDraft,headRefName,url",
                "--jq",
                ".[] | [.number, .state, .isDraft, .headRefName, .url] | @tsv",
            ],
            Forge::GitLab => vec![
                "mr",
                "list",
                "--all",
                "--per-page",
                PAGE,
                "--output",
                "json",
                "--jq",
                ".[] | [.iid, .state, .draft, .source_branch, .web_url] | @tsv",
            ],
        };

        let output = asking
            .command()
            .args(args)
            .current_dir(self.workdir())
            .output()
            .with_context(|| format!("{tool} is not installed, so {}s cannot be read", asking.forge.request()))?;

        anyhow::ensure!(output.status.success(), "{}", complaint(&output.stderr));

        let mut requests = PullRequests::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some((branch, request)) = parse(line) else {
                continue;
            };
            // The same branch can have several. Keep the one that says most.
            match requests.get(&branch) {
                Some(kept) if kept.rank() <= request.rank() => {}
                _ => {
                    requests.insert(branch, request);
                }
            }
        }

        Ok(requests)
    }

    /// Opens a pull request for a branch, pushing it first if it needs it.
    ///
    /// The title and the body are filled from the branch's own commits, which
    /// is what `--fill` does. That is the right default here and not a
    /// shortcut: the commit messages in this repository are written to be
    /// read, so a pull request that repeats them says what it has to say.
    ///
    /// Everyone on the repository sees this, so it is asked about first. The
    /// asking lives in the application, as it does for everything else that
    /// cannot be taken back.
    pub fn open_pull_request(&self, branch: &str) -> Result<String> {
        self.push_branch(branch)?;

        let asking = self.asking();
        let tool = asking.forge.tool();
        let what = asking.forge.request();

        let args: Vec<&str> = match asking.forge {
            Forge::GitHub => vec!["pr", "create", "--fill", "--head", branch],
            // `--yes` and `--fill` between them are what stops it asking a
            // question into a window that is not listening.
            Forge::GitLab => vec!["mr", "create", "--fill", "--yes", "--source-branch", branch],
        };

        let output = asking
            .command()
            .args(args)
            .current_dir(self.workdir())
            .output()
            .with_context(|| format!("{tool} is not installed, so no {what} can be opened"))?;

        anyhow::ensure!(output.status.success(), "{}", complaint(&output.stderr));

        // The last thing it prints is the address of what it just made.
        let url = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("http"))
            .next_back()
            .unwrap_or_default()
            .to_string();

        Ok(match url.is_empty() {
            true => format!("{what} opened for {branch}"),
            false => format!("{what} opened: {url}"),
        })
    }
}

/// One line of `number \t state \t draft \t branch \t url`.
///
/// Anything that does not read as that is skipped rather than refused: one
/// odd line should not throw away four hundred good ones.
fn parse(line: &str) -> Option<(String, PullRequest)> {
    let mut fields = line.split('\t');
    let number = fields.next()?.parse().ok()?;
    // GitHub shouts and GitLab murmurs, and GitLab has a fourth word --
    // "locked" -- for a request nobody can add to. Locked is closed as far
    // as a column is concerned.
    let state = match fields.next()?.to_ascii_uppercase().as_str() {
        "OPEN" | "OPENED" => PullRequestState::Open,
        "MERGED" => PullRequestState::Merged,
        "CLOSED" | "LOCKED" => PullRequestState::Closed,
        _ => return None,
    };
    let draft = fields.next()? == "true";
    let branch = fields.next()?.to_string();
    let url = fields.next().unwrap_or("").to_string();

    if branch.is_empty() {
        return None;
    }

    Some((
        branch,
        PullRequest {
            number,
            state,
            draft,
            url,
        },
    ))
}

/// What the tool complained about, in one line.
///
/// Both write several, and the first is the one that says what to do: "no
/// git remotes found", "gh auth login required". The rest is usage text.
fn complaint(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("the forge could not be read")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(state: &str, draft: bool, branch: &str) -> String {
        format!("7\t{state}\t{draft}\t{branch}\thttps://example.invalid/7")
    }

    #[test]
    fn a_line_becomes_a_pull_request() {
        let (branch, request) = parse(&line("OPEN", false, "feat/login")).unwrap();

        assert_eq!(branch, "feat/login");
        assert_eq!(request.number, 7);
        assert_eq!(request.state, PullRequestState::Open);
        assert!(!request.draft);
        assert_eq!(request.url, "https://example.invalid/7");
    }

    #[test]
    fn a_draft_says_so() {
        let (_, request) = parse(&line("OPEN", true, "feat/login")).unwrap();
        assert!(request.draft);
    }

    #[test]
    fn gitlabs_words_for_the_three_states_count_too() {
        for (word, state) in [
            ("opened", PullRequestState::Open),
            ("merged", PullRequestState::Merged),
            ("closed", PullRequestState::Closed),
            // Locked is closed as far as a column can tell.
            ("locked", PullRequestState::Closed),
        ] {
            let (_, request) = parse(&line(word, false, "feat/login")).expect(word);
            assert_eq!(request.state, state, "{word}");
        }
    }

    #[test]
    fn a_line_that_does_not_read_is_skipped_not_refused() {
        assert!(parse("").is_none());
        assert!(parse("not a number\tOPEN\tfalse\tfeat\turl").is_none());
        assert!(parse("7\tQUEUED\tfalse\tfeat\turl").is_none());
        assert!(parse("7\tOPEN\tfalse\t\turl").is_none());
    }

    #[test]
    fn a_missing_url_is_not_a_missing_pull_request() {
        let (_, request) = parse("7\tMERGED\tfalse\tfeat/login").unwrap();
        assert_eq!(request.state, PullRequestState::Merged);
        assert!(request.url.is_empty());
    }

    #[test]
    fn the_open_one_beats_the_merged_and_the_closed_ones() {
        let open = PullRequest {
            number: 1,
            state: PullRequestState::Open,
            draft: false,
            url: String::new(),
        };
        let merged = PullRequest {
            state: PullRequestState::Merged,
            ..open.clone()
        };
        let closed = PullRequest {
            state: PullRequestState::Closed,
            ..open.clone()
        };

        assert!(open.rank() < merged.rank());
        assert!(merged.rank() < closed.rank());
    }

    #[test]
    fn the_tools_first_complaint_is_the_one_that_says_what_to_do() {
        let stderr = b"\nno git remotes found\n\nUsage: gh pr list [flags]\n";
        assert_eq!(complaint(stderr), "no git remotes found");
        assert_eq!(complaint(b""), "the forge could not be read");
    }
}
