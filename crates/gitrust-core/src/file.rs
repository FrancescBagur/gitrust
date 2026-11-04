//! One file, looked at four ways.
//!
//! The diff answers "what changed here", and for a long time that was the
//! only question this application could answer about a file. The other three
//! are the ones you ask when the diff is not enough: what the whole file said
//! at that point, who last touched each line and why, and everything that has
//! ever happened to it.
//!
//! # What they cost
//!
//! Reading a file out of a commit is free. Blame is not cheap but libgit2
//! does the work. The history of one file is the expensive one and it is
//! worth knowing why: Git stores no such thing. A file's history has to be
//! found by walking commits and diffing each against its parent, so the cost
//! is the length of the history, not the length of the file. There is a limit
//! on it for that reason, and the caller is told when it ran out rather than
//! being quietly handed a short answer.

use anyhow::{Context, Result};
use git2::{ApplyLocation, Delta, DiffOptions, Oid};

use crate::diff::HunkId;
use crate::model::Commit;
use crate::repo::Repo;

/// A file read out of a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Contents {
    /// Text, as it stood.
    Text(String),
    /// Something that is not text. Its size is given because it is the only
    /// useful thing there is to say about it here.
    Binary(usize),
    /// The file was not in that commit at all. Ordinary, and worth saying in
    /// its own words: a file has a first commit, and before it there is
    /// nothing rather than an empty file.
    Absent,
}

/// One line, and who is answerable for it.
#[derive(Debug, Clone)]
pub struct Line {
    pub number: usize,
    pub text: String,
    pub commit: Oid,
    pub short: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
    /// Whether this is the first line of a run from the same commit.
    ///
    /// Blame reads as blocks, not as lines: twenty lines from one commit want
    /// saying once. Which line starts a block is known here, while walking
    /// them in order, and working it out again in the view would mean
    /// comparing each line with the one above it to recover something that
    /// was already known.
    pub first: bool,
}

/// What a file's history came to, and whether that is all of it.
#[derive(Debug, Clone, Default)]
pub struct Story {
    pub commits: Vec<Commit>,
    /// True when the walk stopped at its limit rather than at the beginning.
    /// A list that might be missing its oldest entries has to say so.
    pub cut_short: bool,
}

impl Repo {
    /// A file as it stood at a commit.
    pub fn file_at(&self, oid: Oid, path: &str) -> Result<Contents> {
        let commit = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {}", short(oid)))?;
        let tree = commit.tree()?;

        let Ok(entry) = tree.get_path(std::path::Path::new(path)) else {
            return Ok(Contents::Absent);
        };
        let object = entry.to_object(&self.inner)?;
        let Some(blob) = object.as_blob() else {
            return Ok(Contents::Absent);
        };

        // libgit2's own guess, which is the same one Git uses when it decides
        // whether to print a diff or refuse to.
        if blob.is_binary() {
            return Ok(Contents::Binary(blob.size()));
        }
        Ok(Contents::Text(String::from_utf8_lossy(blob.content()).to_string()))
    }

    /// Who last touched each line of a file, and in which commit.
    ///
    /// `at` says which commit to look from. `None` means the working tree,
    /// which is what you want nine times out of ten: the question is usually
    /// about the file in front of you.
    ///
    /// Lines added since the last commit belong to no commit at all. They are
    /// given the null Oid and an empty author, the way the working tree's row
    /// in the graph is, so that one reading covers both.
    pub fn blame(&self, path: &str, at: Option<Oid>) -> Result<Vec<Line>> {
        let text = match at {
            Some(oid) => match self.file_at(oid, path)? {
                Contents::Text(text) => text,
                Contents::Binary(_) => anyhow::bail!("{path} is not text, so there is no blame"),
                Contents::Absent => anyhow::bail!("{path} is not in {}", short(oid)),
            },
            None => {
                let whole = self.inner.workdir().context("there is no working tree here")?;
                std::fs::read_to_string(whole.join(path))
                    .with_context(|| format!("could not read {path}"))?
            }
        };

        let mut options = git2::BlameOptions::new();
        if let Some(oid) = at {
            options.newest_commit(oid);
        }
        let blame = self
            .inner
            .blame_file(std::path::Path::new(path), Some(&mut options))
            .with_context(|| format!("could not work out the blame for {path}"))?;

        let mut lines = Vec::new();
        let mut last = None;
        for (index, text) in text.lines().enumerate() {
            let number = index + 1;
            let hunk = blame.get_line(number);

            let (commit, author, time, summary) = match &hunk {
                Some(hunk) => {
                    let id = hunk.final_commit_id();
                    let who = hunk.final_signature();
                    let summary = self
                        .inner
                        .find_commit(id)
                        .ok()
                        .and_then(|commit| commit.summary().map(str::to_string))
                        .unwrap_or_default();
                    (
                        id,
                        who.name().unwrap_or("(unknown)").to_string(),
                        who.when().seconds(),
                        summary,
                    )
                }
                // Not in any commit yet: written since the last one.
                None => (Oid::zero(), String::new(), 0, String::new()),
            };

            let first = last != Some(commit);
            last = Some(commit);

            lines.push(Line {
                number,
                text: text.to_string(),
                commit,
                short: match commit.is_zero() {
                    true => String::new(),
                    false => short(commit),
                },
                author,
                time,
                summary,
                first,
            });
        }

        Ok(lines)
    }

    /// Every commit that touched a file, newest first.
    ///
    /// Git keeps no such list. It is found by walking the history and asking
    /// of each commit whether its own diff mentions this path, so the work is
    /// proportional to how long the repository has been going and not to how
    /// big the file is. `limit` is how many commits are walked, not how many
    /// come back, and [`Story::cut_short`] says whether it ran out.
    ///
    /// A merge is left out unless it really changed the file. A merge that
    /// only joined two sides mentions every path either side touched, and
    /// listing it would put a commit in the history of a file it did nothing
    /// to.
    pub fn file_history(&self, path: &str, limit: usize) -> Result<Story> {
        let labels = self.ref_labels()?;
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        walk.push_head().context("the repository has no commits")?;

        let mut story = Story::default();
        let mut walked = 0;

        for oid in walk {
            if walked >= limit {
                story.cut_short = true;
                break;
            }
            walked += 1;

            let oid = oid?;
            let commit = self.inner.find_commit(oid)?;
            if !self.touched(&commit, path)? {
                continue;
            }

            let author = commit.author();
            story.commits.push(Commit {
                id: oid,
                short_id: short(oid),
                summary: commit.summary().unwrap_or("(no message)").to_string(),
                author: author.name().unwrap_or("(unknown)").to_string(),
                time: commit.time().seconds(),
                parents: commit.parent_ids().collect(),
                refs: labels.get(&oid).cloned().unwrap_or_default(),
            });
        }

        Ok(story)
    }

    /// Whether a commit changed a file.
    ///
    /// Against its first parent, which is the side the history is being
    /// walked down. A root commit, which has no parent, is compared with
    /// nothing: everything in it is new.
    fn touched(&self, commit: &git2::Commit<'_>, path: &str) -> Result<bool> {
        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(0);

        let parent = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };
        let diff = self.inner.diff_tree_to_tree(
            parent.as_ref(),
            Some(&commit.tree()?),
            Some(&mut options),
        )?;

        Ok(diff.deltas().any(|delta| delta.status() != Delta::Unmodified))
    }

    /// Undoes one hunk of a commit, in the working tree.
    ///
    /// Nothing in the history moves. The change is applied backwards to the
    /// files in front of you, and what to do with it after that -- commit it,
    /// look at it, throw it away again -- is left to you. That is what
    /// separates it from undoing the whole commit, which writes one.
    ///
    /// It is refused when there is already something uncommitted in that
    /// file. Applying a patch backwards over changes somebody is in the
    /// middle of is how you get a file that is neither what they wrote nor
    /// what they had.
    pub fn revert_hunk(&self, oid: Oid, path: &str, hunk: HunkId) -> Result<String> {
        let commit = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {}", short(oid)))?;
        anyhow::ensure!(
            commit.parent_count() < 2,
            "{} is a merge, and a hunk of one has no one side to undo",
            short(oid)
        );

        let status = self.status()?;
        let busy = status
            .staged
            .iter()
            .chain(status.unstaged.iter())
            .any(|change| change.path == path);
        anyhow::ensure!(
            !busy,
            "{path} has changes of its own that are not committed. \
             Commit or stash them first"
        );

        // Worked out backwards, as discarding a hunk is, and applied to the
        // tree: that way it undoes that one piece where it is written.
        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3).reverse(true);

        let parent = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };
        let diff = self.inner.diff_tree_to_tree(
            parent.as_ref(),
            Some(&commit.tree()?),
            Some(&mut options),
        )?;

        self.apply_hunk(&diff, hunk.reversed(), ApplyLocation::WorkDir)?;
        Ok(format!("that piece of {} is undone in {path}", short(oid)))
    }
}

fn short(oid: Oid) -> String {
    oid.to_string().chars().take(7).collect()
}
