//! The work in hand, read off the graph.
//!
//! A branch is a unit of work, and the repository already knows where each one
//! stands. This module stores nothing and asks nothing: it looks at where each
//! branch falls against the main one and works out the column. A board that is
//! worked out cannot fall out of date, which is what happens to every board
//! somebody has to keep by hand.
//!
//! What it cannot know, and had better not pretend to: Git does not record
//! when a branch was made. A finished branch and one that never started both
//! end up with nothing of their own, and the only thing telling them apart is
//! whether the main branch swallowed it in some merge. So "done" is read off
//! the merge commits, not off the distance: a branch that went in by fast
//! forward, or squashed into one commit, stays in "to do". Better that than
//! telling you that you finished something you did not.

use std::collections::HashSet;

use anyhow::{Context, Result};
use git2::{BranchType, ObjectType, Oid};

use crate::repo::Repo;

/// Where a branch stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Made, and still in the same place as the main branch.
    Todo,
    /// It has commits of its own and has not left this machine.
    InProgress,
    /// It has commits of its own and is on the remote, waiting for somebody.
    InReview,
    /// Everything of its own is inside the main branch.
    Done,
}

impl Lane {
    /// All four, in the order they are read from left to right.
    pub const ALL: [Lane; 4] = [Lane::Todo, Lane::InProgress, Lane::InReview, Lane::Done];
}

/// A branch seen as work.
#[derive(Debug, Clone)]
pub struct Card {
    pub branch: String,
    /// The folder in its name: `theme` in `theme/colors`. It is what stands in
    /// for a project, because it is what people already use to say that two
    /// branches are the same job.
    pub project: Option<String>,
    pub lane: Lane,
    /// Commits of its own that the main branch does not have.
    pub ahead: usize,
    /// Commits of the main branch that it is missing.
    pub behind: usize,
    /// When the last thing it has was written.
    pub time: i64,
    /// Whether it is the branch that is checked out right now.
    pub current: bool,
    pub target: Oid,
}

impl Card {
    /// The name without the folder, which is what the branch is called aloud.
    pub fn leaf(&self) -> &str {
        self.branch.rsplit_once('/').map_or(self.branch.as_str(), |(_, leaf)| leaf)
    }
}

/// The whole board.
#[derive(Debug, Clone)]
pub struct Board {
    pub cards: Vec<Card>,
    /// The branch everything else is measured against.
    pub trunk: String,
}

impl Board {
    /// The cards in one column, in the order they already come in.
    pub fn lane(&self, lane: Lane) -> impl Iterator<Item = &Card> {
        self.cards.iter().filter(move |card| card.lane == lane)
    }

    /// The projects there are, without repeats and in order.
    pub fn projects(&self) -> Vec<&str> {
        let mut names: Vec<&str> =
            self.cards.iter().filter_map(|card| card.project.as_deref()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

impl Repo {
    /// Sorts each local branch by where it falls against the main one.
    pub fn board(&self) -> Result<Board> {
        let trunk = self.trunk()?;
        let tip = self
            .inner
            .find_branch(&trunk, BranchType::Local)
            .with_context(|| format!("no branch called '{trunk}'"))?
            .get()
            .peel(ObjectType::Commit)?
            .id();

        let head = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string));

        // The tips the main branch took in at some merge. It is the only thing
        // telling a finished branch from one that never got started.
        let mut swallowed = HashSet::new();
        let mut walk = self.inner.revwalk()?;
        walk.push(tip)?;
        for oid in walk.flatten() {
            let Ok(commit) = self.inner.find_commit(oid) else {
                continue;
            };
            if commit.parent_count() > 1 {
                swallowed.extend(commit.parent_ids().skip(1));
            }
        }

        // Which branches some remote knows, in one pass: asking branch by
        // branch would walk the whole list that many times over.
        let mut published = HashSet::new();
        for reference in self.inner.references()? {
            let Some(name) = reference?.name().map(str::to_string) else {
                continue;
            };
            if let Some(rest) = name.strip_prefix("refs/remotes/") {
                if let Some((_, branch)) = rest.split_once('/') {
                    published.insert(branch.to_string());
                }
            }
        }

        let mut cards = Vec::new();
        for branch in self.inner.branches(Some(BranchType::Local))? {
            let (branch, _) = branch?;
            let Some(name) = branch.name()?.map(str::to_string) else {
                continue;
            };
            if name == trunk {
                continue;
            }
            let Ok(object) = branch.get().peel(ObjectType::Commit) else {
                continue;
            };

            let target = object.id();
            let (ahead, behind) = self.inner.graph_ahead_behind(target, tip)?;

            // With something of its own, the column depends on whether it has
            // left here. With nothing of its own, on whether the main branch
            // swallowed it or the branch has simply not done anything yet.
            let lane = if ahead > 0 {
                match published.contains(&name) {
                    true => Lane::InReview,
                    false => Lane::InProgress,
                }
            } else if swallowed.contains(&target) {
                Lane::Done
            } else {
                Lane::Todo
            };

            let time = self
                .inner
                .find_commit(target)
                .map(|commit| commit.time().seconds())
                .unwrap_or(0);

            cards.push(Card {
                project: name.rsplit_once('/').map(|(folder, _)| folder.to_string()),
                current: head.as_deref() == Some(name.as_str()),
                branch: name,
                lane,
                ahead,
                behind,
                time,
                target,
            });
        }

        // The last thing touched, first: a board is read from the top.
        cards.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.branch.cmp(&b.branch)));
        Ok(Board { cards, trunk })
    }

    /// The branch the rest of the work is measured against.
    ///
    /// The remote is asked first, because it is the one that really knows: its
    /// HEAD points at the repository's default branch. With no remote, the
    /// usual names are tried. And failing that, the branch that is checked out,
    /// which is at least not an invention.
    pub fn trunk(&self) -> Result<String> {
        for remote in self.inner.remotes()?.iter().flatten() {
            let Ok(reference) = self.inner.find_reference(&format!("refs/remotes/{remote}/HEAD"))
            else {
                continue;
            };
            let Some(target) = reference.symbolic_target() else {
                continue;
            };
            let Some(name) = target.strip_prefix(&format!("refs/remotes/{remote}/")) else {
                continue;
            };
            if self.inner.find_branch(name, BranchType::Local).is_ok() {
                return Ok(name.to_string());
            }
        }

        for name in ["main", "master", "trunk", "develop"] {
            if self.inner.find_branch(name, BranchType::Local).is_ok() {
                return Ok(name.to_string());
            }
        }

        self.inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string))
            .context("the repository has no branches")
    }
}
