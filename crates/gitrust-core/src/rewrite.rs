//! Rewriting a run of commits: squashing, rewording, dropping, reordering.
//!
//! What `git rebase -i` is for, without the editor and without the file full
//! of lines beginning `pick`. You are handed the commits, you say what should
//! become of each, and the whole thing is rebuilt.
//!
//! # All of it or none of it
//!
//! Everywhere else in gitrust that writes history now stops where it clashes
//! and waits to be decided. This does not, and the reason is worth saying
//! plainly rather than discovering.
//!
//! A merge is one clash. A rebase is one clash per commit, and libgit2 keeps
//! the sequence in `.git` so that the application can put it down and pick it
//! up -- and so that Git itself knows what is going on if you look at the
//! repository with anything else. A plan like this one is not something
//! libgit2 has: squashing and reordering are not in its rebase at all. To
//! stop halfway through one, gitrust would have to invent its own sequencer
//! and write it somewhere in `.git` that nothing else understands, and then a
//! repository stopped in the middle of a rewrite would be a repository
//! `git status` could not explain.
//!
//! So this is worked out from beginning to end in memory, and the branch is
//! moved in one step at the very end. If anything clashes on the way, nothing
//! has been written and nothing has been touched: the plan is handed back
//! with the commit that would not go, to be changed or given up.
//!
//! # What it does to history
//!
//! Every commit in the range comes out with a new identity, even the ones
//! kept exactly as they were, because their parents changed. Anything already
//! pushed and already pulled by somebody else is a different thing afterwards
//! and they will feel it. That is true of rebasing in general; it is only
//! more true here, where the point is to change what the commits say.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, Oid, Signature};

use crate::repo::Repo;

/// How far back a rewrite may reach.
///
/// Not a technical limit. Rewriting is read commit by commit and held in
/// memory, and a plan nobody can read is a plan nobody should be running: a
/// list of five hundred commits with a dropdown on each is not a decision, it
/// is a dare.
pub const REACH: usize = 500;

/// What should become of one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Keep it as it is.
    Pick,
    /// Keep it, saying something else.
    Reword,
    /// Fold it into the one above, keeping both messages.
    Squash,
    /// Fold it into the one above, throwing its message away.
    Fixup,
    /// Leave it out. What it did is undone.
    Drop,
}

impl Step {
    /// What it is called, for a screen.
    pub fn says(self) -> &'static str {
        match self {
            Step::Pick => "Keep",
            Step::Reword => "Reword",
            Step::Squash => "Squash",
            Step::Fixup => "Fixup",
            Step::Drop => "Drop",
        }
    }

    /// What it actually does, for somebody who has not done this before.
    pub fn means(self) -> &'static str {
        match self {
            Step::Pick => "leave this commit as it is",
            Step::Reword => "keep what it did, say something else about it",
            Step::Squash => "fold it into the one above, keeping both messages",
            Step::Fixup => "fold it into the one above and throw its message away",
            Step::Drop => "leave it out: what it did is undone",
        }
    }

    /// Whether it disappears into the commit above rather than being one.
    pub fn folds(self) -> bool {
        matches!(self, Step::Squash | Step::Fixup)
    }
}

/// One commit, and what is to become of it.
#[derive(Debug, Clone)]
pub struct Line {
    pub commit: Oid,
    pub short: String,
    pub summary: String,
    pub author: String,
    pub time: i64,
    pub step: Step,
    /// What this commit will say. It starts as what it already says, and is
    /// only read when the step is [`Step::Reword`] -- so typing into it and
    /// then changing your mind costs nothing.
    pub message: String,
}

/// A run of commits and what is to become of each.
///
/// Oldest first, which is the order it is replayed in and the order the
/// screen reads in. It is the other way round from the graph, and that is on
/// purpose: "fold into the one above" has to mean the one above on the
/// screen, or nobody can use it.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The commit all of this is rebuilt on top of: the parent of the oldest.
    pub onto: Oid,
    /// The branch being rewritten, checked again before anything is written.
    pub branch: String,
    pub lines: Vec<Line>,
}

impl Plan {
    /// How many commits this would leave behind.
    pub fn kept(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| matches!(line.step, Step::Pick | Step::Reword))
            .count()
    }

    /// What it would do, in one line, for somebody about to press it.
    pub fn says(&self) -> String {
        let was = self.lines.len();
        match (was, self.kept()) {
            (was, 0) => format!("{} would be left out altogether", commits(was)),
            (was, now) if was == now => format!("{} rewritten, still {now}", commits(was)),
            (was, 1) => format!("{was} commits become 1"),
            (was, now) => format!("{was} commits become {now}"),
        }
    }

    /// Why this plan cannot be run, when it cannot.
    ///
    /// Asked before the button is drawn rather than after it is pressed: a
    /// plan with nothing above the first commit to fold into is a mistake
    /// made while arranging, and the place to say so is while arranging.
    pub fn trouble(&self) -> Option<String> {
        let first = self.lines.iter().find(|line| line.step != Step::Drop)?;
        first
            .step
            .folds()
            .then(|| format!("{} is the first one: there is nothing above it to fold it into", first.short))
    }
}

impl Repo {
    /// The commits from one up to the tip of the current branch, ready to be
    /// arranged.
    ///
    /// Everything comes back as [`Step::Pick`]: a plan that has not been
    /// touched yet does nothing, and running it would leave the same history
    /// with new hashes. That is the right starting point. The alternative is
    /// a screen that has already decided something on your behalf.
    pub fn plan_from(&self, oid: Oid) -> Result<Plan> {
        let (branch, head) = self.here()?;

        let oldest = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {}", short(oid)))?;
        anyhow::ensure!(
            oldest.parent_count() > 0,
            "{} is the first commit there is: \
             there is nothing underneath it to rebuild it on",
            short(oid)
        );

        // Walked from the tip down, following first parents, which is also
        // how the range is checked: if we fall off the beginning without
        // meeting it, it was never on this branch.
        let mut lines = Vec::new();
        let mut walk = head;
        loop {
            anyhow::ensure!(
                walk.parent_count() < 2,
                "{} is a merge, and a merge cannot be replayed one commit at a time",
                short(walk.id())
            );
            anyhow::ensure!(
                lines.len() < REACH,
                "that is more than {REACH} commits back. \
                 Pick something nearer the tip"
            );

            lines.push(line(&walk));
            if walk.id() == oid {
                break;
            }
            walk = walk
                .parent(0)
                .with_context(|| format!("{} is not on {branch}", short(oid)))?;
        }

        lines.reverse();
        Ok(Plan {
            onto: oldest.parent(0)?.id(),
            branch,
            lines,
        })
    }

    /// Rebuilds a run of commits the way a plan says, and moves the branch.
    ///
    /// Nothing at all is written until every commit has been worked out. See
    /// the note at the top of this module about why this one does not stop
    /// halfway.
    pub fn rewrite(&self, plan: &Plan) -> Result<String> {
        self.nothing_half_done()?;
        anyhow::ensure!(
            self.status()?.is_clean(),
            "commit or stash what you have half-done before rewriting history"
        );
        if let Some(trouble) = plan.trouble() {
            anyhow::bail!("{trouble}");
        }
        anyhow::ensure!(!plan.lines.is_empty(), "there is nothing in this plan");

        let (branch, head) = self.here()?;
        anyhow::ensure!(
            branch == plan.branch,
            "this plan is for {}, and you are on {branch} now",
            plan.branch
        );
        let newest = plan.lines.last().expect("just checked").commit;
        anyhow::ensure!(
            head.id() == newest,
            "{branch} has moved since this plan was made"
        );

        let committer = self.who()?;
        let mut base = self.inner.find_commit(plan.onto)?;
        let mut building: Option<Building> = None;
        let mut emptied = 0;

        for line in &plan.lines {
            let commit = self.inner.find_commit(line.commit)?;

            match line.step {
                Step::Drop => continue,
                Step::Pick | Step::Reword => {
                    if let Some(ready) = building.take() {
                        match self.write(ready, &base, &committer)? {
                            Some(written) => base = written,
                            None => emptied += 1,
                        }
                    }

                    let tree = self.lay(&base.tree()?, &commit, line)?;
                    building = Some(Building {
                        tree,
                        message: match line.step {
                            Step::Reword => line.message.clone(),
                            _ => said(&commit),
                        },
                        author: owned(&commit)?,
                    });
                }
                Step::Squash | Step::Fixup => {
                    // Refused while arranging, so reaching here would be a
                    // plan that passed `trouble` and should not have.
                    let ready = building
                        .as_mut()
                        .context("there is nothing above this one to fold it into")?;

                    let onto = self.inner.find_tree(ready.tree)?;
                    ready.tree = self.lay(&onto, &commit, line)?;
                    if line.step == Step::Squash {
                        ready.message = format!("{}\n\n{}", ready.message.trim_end(), said(&commit));
                    }
                }
            }
        }

        if let Some(ready) = building.take() {
            match self.write(ready, &base, &committer)? {
                Some(written) => base = written,
                None => emptied += 1,
            }
        }

        // Everything is written and nothing points at it yet. This one line
        // is the whole of the change as far as the branch is concerned, and
        // it is the first thing here that cannot be undone by walking away.
        let refname = self
            .inner
            .head()?
            .name()
            .context("HEAD has no name")?
            .to_string();
        self.inner
            .find_reference(&refname)?
            .set_target(base.id(), "rebase (gitrust): history rewritten")?;
        self.inner
            .checkout_head(Some(CheckoutBuilder::new().force()))
            .context("the history was rewritten but the files could not be brought up to it")?;

        Ok(match emptied {
            0 => format!("{} rewritten on {branch}", commits(plan.lines.len())),
            1 => format!(
                "{} rewritten on {branch}, and one of them had nothing left in it",
                commits(plan.lines.len())
            ),
            n => format!(
                "{} rewritten on {branch}, and {n} of them had nothing left in them",
                commits(plan.lines.len())
            ),
        })
    }

    /// Lays one commit's change onto a tree, in memory.
    ///
    /// This is a cherry-pick said in trees rather than in commits, which is
    /// what lets a whole plan be worked out without writing anything: the
    /// commit's own parent is what its change is measured against, the tree
    /// being built is the other side.
    fn lay(&self, onto: &git2::Tree, commit: &git2::Commit, line: &Line) -> Result<Oid> {
        let was = commit.parent(0)?.tree()?;
        let mut index = self
            .inner
            .merge_trees(&was, onto, &commit.tree()?, None)
            .with_context(|| format!("could not work {} out", line.short))?;

        anyhow::ensure!(
            !index.has_conflicts(),
            "{} ({}) clashes with what is underneath it now. \
             Nothing was written and nothing was touched",
            line.short,
            line.summary
        );

        Ok(index.write_tree_to(&self.inner)?)
    }

    /// Writes one commit of the plan, unless it turned out to say nothing.
    ///
    /// `None` means it was dropped for being empty. Git does the same, and
    /// for the same reason: a commit whose tree is its parent's is a commit
    /// that happened to nothing.
    fn write(
        &self,
        ready: Building,
        onto: &git2::Commit,
        committer: &Signature<'_>,
    ) -> Result<Option<git2::Commit<'_>>> {
        if ready.tree == onto.tree_id() {
            return Ok(None);
        }

        let tree = self.inner.find_tree(ready.tree)?;
        let oid = self.inner.commit(
            None,
            &ready.author,
            committer,
            ready.message.trim(),
            &tree,
            &[onto],
        )?;
        Ok(Some(self.inner.find_commit(oid)?))
    }
}

/// A commit being built up, before it is written.
struct Building {
    tree: Oid,
    message: String,
    author: Signature<'static>,
}

fn line(commit: &git2::Commit) -> Line {
    Line {
        commit: commit.id(),
        short: short(commit.id()),
        summary: commit.summary().unwrap_or("(no message)").to_string(),
        author: commit.author().name().unwrap_or("unknown").to_string(),
        time: commit.time().seconds(),
        step: Step::Pick,
        message: said(commit),
    }
}

fn said(commit: &git2::Commit) -> String {
    commit.message().unwrap_or("").trim().to_string()
}

/// The author, kept rather than borrowed.
///
/// Who wrote a change does not change because it was moved, so every commit
/// keeps its own author and only the committer becomes whoever is running
/// this. The signature has to outlive the commit it came from, which is the
/// whole reason this exists.
fn owned(commit: &git2::Commit) -> Result<Signature<'static>> {
    let author = commit.author();
    Ok(Signature::new(
        author.name().unwrap_or("unknown"),
        author.email().unwrap_or("unknown@example.invalid"),
        &author.when(),
    )?)
}

fn commits(count: usize) -> String {
    match count {
        1 => "1 commit".to_string(),
        n => format!("{n} commits"),
    }
}

fn short(oid: Oid) -> String {
    oid.to_string().chars().take(7).collect()
}
