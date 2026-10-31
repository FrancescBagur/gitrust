//! The work in hand, read off the graph.
//!
//! A branch is a unit of work, and the repository already knows where each one
//! stands. This module stores nothing and asks nothing: it looks at where each
//! branch falls against the main one and works out the column. A board that is
//! worked out cannot fall out of date, which is what happens to every board
//! somebody has to keep by hand.
//!
//! What it cannot know, and had better not pretend to: Git does not record
//! when a branch was made, and it records nothing at all about pull requests.
//! A finished branch and one that never started both end up with nothing of
//! their own, and the only thing telling them apart is whether the main branch
//! swallowed it in some merge. So "done" is read off the merge commits, not
//! off the distance: a branch that went in by fast forward, or squashed into
//! one commit, stays in "to do". Better that than telling you that you
//! finished something you did not.
//!
//! When the forge has been asked, its answer overrules all of that, because it
//! is the one that knows. A merged pull request means done however the commits
//! got there, and "in review" means a pull request is open and not a draft —
//! not merely that the branch has been pushed. Unasked, the graph falls back
//! to reading a pushed branch as being in review, which is the best guess
//! there is when there is nobody to ask.

use std::collections::HashSet;

use anyhow::{Context, Result};
use git2::{BranchType, ObjectType, Oid};

use crate::forge::{PullRequest, PullRequestState, PullRequests};
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
    /// Its pull request, when the forge was asked and knew of one.
    pub pull: Option<PullRequest>,
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
    /// Whether the forge answered. Without it, "in review" is a guess made
    /// from the branch being pushed, and the columns say as much.
    pub asked: bool,
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

/// Which column a branch falls in, given everything known about it.
///
/// The forge goes first wherever it has something to say, because it is the
/// only one of the two that was told. A merged pull request means done no
/// matter how the commits travelled: squashed into one, fast forwarded, or
/// merged with a commit of its own. That case is the whole reason for asking,
/// since a squash leaves the graph with no evidence at all that the work
/// landed.
///
/// What it gets wrong, and says plainly here rather than in a bug report: a
/// branch picked up again after its pull request merged reads as done until a
/// new pull request opens on it. Telling that apart from a squash would mean
/// knowing which commits the merged one carried, and by then the branch has
/// usually been deleted.
fn lane_of(
    pull: Option<&PullRequest>,
    asked: bool,
    ahead: usize,
    published: bool,
    swallowed: bool,
) -> Lane {
    match pull.map(|pull| (pull.state, pull.draft)) {
        Some((PullRequestState::Merged, _)) => return Lane::Done,
        // A draft is the author saying it is not ready to be looked at, so it
        // is not in review however long it has been up there.
        Some((PullRequestState::Open, false)) => return Lane::InReview,
        _ => {}
    }

    if ahead > 0 {
        // Asked and given no open pull request, a pushed branch is just a
        // pushed branch. Unasked, being pushed is the only evidence there is.
        return match !asked && published {
            true => Lane::InReview,
            false => Lane::InProgress,
        };
    }

    match swallowed {
        true => Lane::Done,
        false => Lane::Todo,
    }
}

impl Repo {
    /// Sorts each local branch by where it falls against the main one.
    ///
    /// This is the board drawn without waiting for anybody: everything in it
    /// comes out of the repository, so it is there the instant the tab opens.
    pub fn board(&self) -> Result<Board> {
        self.board_with(None)
    }

    /// The same, with the forge's answer laid over the top.
    ///
    /// `None` means the forge was never asked, or could not answer. That is a
    /// different thing from an empty answer: asked and told there are no pull
    /// requests, a pushed branch is just a pushed branch, and stays in
    /// progress. Unasked, being pushed is the only evidence there is.
    pub fn board_with(&self, pulls: Option<&PullRequests>) -> Result<Board> {
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

            let pull = pulls.and_then(|pulls| pulls.get(&name)).cloned();
            let lane = lane_of(
                pull.as_ref(),
                pulls.is_some(),
                ahead,
                published.contains(&name),
                swallowed.contains(&target),
            );

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
                pull,
            });
        }

        // The last thing touched, first: a board is read from the top.
        cards.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.branch.cmp(&b.branch)));
        Ok(Board {
            cards,
            trunk,
            asked: pulls.is_some(),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(state: PullRequestState, draft: bool) -> PullRequest {
        PullRequest {
            number: 1,
            state,
            draft,
            url: String::new(),
        }
    }

    #[test]
    fn a_merged_pull_request_means_done_however_the_commits_travelled() {
        // Squashed: the commits are still ahead of the main branch and no
        // merge swallowed them, so the graph alone would say "in progress".
        let lane = lane_of(
            Some(&pull(PullRequestState::Merged, false)),
            true,
            3,
            true,
            false,
        );
        assert_eq!(lane, Lane::Done);
    }

    #[test]
    fn an_open_pull_request_means_in_review() {
        let lane = lane_of(
            Some(&pull(PullRequestState::Open, false)),
            true,
            1,
            true,
            false,
        );
        assert_eq!(lane, Lane::InReview);
    }

    #[test]
    fn a_draft_is_not_in_review_yet() {
        let lane = lane_of(
            Some(&pull(PullRequestState::Open, true)),
            true,
            1,
            true,
            false,
        );
        assert_eq!(lane, Lane::InProgress);
    }

    #[test]
    fn a_closed_pull_request_leaves_it_to_the_graph() {
        let lane = lane_of(
            Some(&pull(PullRequestState::Closed, false)),
            true,
            1,
            true,
            false,
        );
        assert_eq!(lane, Lane::InProgress);
    }

    #[test]
    fn asked_and_told_nothing_a_pushed_branch_is_still_in_progress() {
        assert_eq!(lane_of(None, true, 1, true, false), Lane::InProgress);
    }

    #[test]
    fn unasked_a_pushed_branch_is_the_best_guess_there_is() {
        assert_eq!(lane_of(None, false, 1, true, false), Lane::InReview);
        assert_eq!(lane_of(None, false, 1, false, false), Lane::InProgress);
    }

    #[test]
    fn with_nothing_of_its_own_a_swallowed_branch_is_done_and_the_rest_to_do() {
        assert_eq!(lane_of(None, true, 0, true, true), Lane::Done);
        assert_eq!(lane_of(None, true, 0, true, false), Lane::Todo);
    }
}
