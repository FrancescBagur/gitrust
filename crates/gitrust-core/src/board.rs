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
//!
//! # Subtasks
//!
//! A branch cut from another branch is a piece of that branch's work, and
//! that is all a subtask is here. It is read off the graph like everything
//! else: a branch is a child of whichever other branch it shares more history
//! with than it shares with the trunk. So the board inside a card is the same
//! board measured against a different branch, and "done" for a subtask is
//! having gone into its parent rather than into the trunk -- which is exactly
//! what being finished means for a piece of a larger job.
//!
//! Git records none of this, so it is a reading and not a fact, and it has
//! two known wrong answers. A rebase moves the fork, so a replayed branch may
//! come back the child of something else or of nothing. And a subtask that
//! went into its parent by fast forward leaves two names on one commit, which
//! says nothing about which came from which, so both sit outside. Both are
//! the same blindness the columns already own up to, and both are pinned by a
//! test rather than left to be discovered.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use git2::{BranchType, ObjectType, Oid, Repository};

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
    /// How many branches were cut from this one. They are not on this board:
    /// they are on the one inside this card.
    pub children: usize,
}

impl Card {
    /// The name without the folder, which is what the branch is called aloud.
    pub fn leaf(&self) -> &str {
        self.branch.rsplit_once('/').map_or(self.branch.as_str(), |(_, leaf)| leaf)
    }
}

/// Which branch each branch was cut from.
///
/// `None` is the trunk, which is where most work starts and is the only
/// answer that needs no evidence.
pub type Parents = HashMap<String, Option<String>>;

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

/// Which branch each branch was cut from.
///
/// Git does not record this. A branch is a name pointing at a commit, and
/// nothing anywhere says where the name came from, so this is worked out from
/// the shape of the history and is a reading, not a fact.
///
/// The reading: a branch is a child of whichever other branch it shares more
/// history with than it shares with the trunk. Two branches cut from the same
/// place on the trunk share exactly the trunk's history and nothing more, so
/// they are siblings and both answer `None`. One cut from a branch that had
/// already left the trunk shares that branch's extra commits, so it is a
/// child of it.
///
/// Where it is wrong, said here rather than found out later: **a rebase moves
/// the fork.** Replaying a branch somewhere else leaves no trace of where it
/// used to hang from, so a rebased branch may come back the child of something
/// else, or of nothing. Git's own `merge-base --fork-point` has the same
/// trouble and answers it with the reflog, which never leaves your machine and
/// expires, so it is no use for a board the whole team reads.
fn parentage(inner: &Repository, trunk_tip: Oid, tips: &[(String, Oid)]) -> Parents {
    let merges = merges(inner, tips);
    tips.iter()
        .map(|(name, tip)| {
            let parent = cut_from(inner, name, *tip, trunk_tip, tips, &merges);
            (name.clone(), parent)
        })
        .collect()
}

/// Every commit that was taken into something by a merge, and the merge
/// commits that took it.
///
/// One walk over every branch, because the alternative is one walk for each
/// pair of branches, and the pairs are the square of the branches.
fn merges(inner: &Repository, tips: &[(String, Oid)]) -> HashMap<Oid, Vec<Oid>> {
    let mut out: HashMap<Oid, Vec<Oid>> = HashMap::new();
    let Ok(mut walk) = inner.revwalk() else {
        return out;
    };
    for (_, tip) in tips {
        let _ = walk.push(*tip);
    }

    for oid in walk.flatten() {
        let Ok(commit) = inner.find_commit(oid) else {
            continue;
        };
        // The first parent is where the branch was; the rest are what it took in.
        for parent in commit.parent_ids().skip(1) {
            out.entry(parent).or_default().push(oid);
        }
    }
    out
}

/// Whether `tip` was merged into something that `other_tip` has.
///
/// This is what tells apart the two shapes that look identical from the fork:
/// a branch cut from another, and a branch finished into another. Both leave
/// one holding everything the other has, and both leave the same merge base,
/// so the merge commit is the only evidence there is. Without it, finishing a
/// subtask would take it off its parent's board -- the one place anybody would
/// go looking for it.
fn took_in(
    inner: &Repository,
    other_tip: Oid,
    tip: Oid,
    merges: &HashMap<Oid, Vec<Oid>>,
) -> bool {
    merges.get(&tip).into_iter().flatten().any(|merge| {
        *merge == other_tip || inner.graph_descendant_of(other_tip, *merge).unwrap_or(false)
    })
}

/// The one branch this one most likely came out of, or `None` for the trunk.
fn cut_from(
    inner: &Repository,
    name: &str,
    tip: Oid,
    trunk_tip: Oid,
    tips: &[(String, Oid)],
    merges: &HashMap<Oid, Vec<Oid>>,
) -> Option<String> {
    let trunk_base = inner.merge_base(tip, trunk_tip).ok()?;
    let mut best: Option<(String, Oid, usize)> = None;

    for (other, other_tip) in tips {
        if other == name {
            continue;
        }
        // Work this branch took in is finished work inside it, not where it
        // came from.
        if took_in(inner, tip, *other_tip, merges) {
            continue;
        }
        // And a branch that already holds everything this one has did not give
        // rise to it -- unless the reason it holds it is that it took it in,
        // which is a subtask being finished and leaves the parent a parent.
        if *other_tip == tip {
            continue;
        }
        if inner.graph_descendant_of(*other_tip, tip).unwrap_or(false)
            && !took_in(inner, *other_tip, tip, merges)
        {
            continue;
        }

        let Ok(shared) = inner.merge_base(tip, *other_tip) else {
            continue;
        };
        // Sharing no more with that branch than with the trunk means they left
        // the trunk together: siblings, not parent and child.
        if shared == trunk_base || !inner.graph_descendant_of(shared, trunk_base).unwrap_or(false) {
            continue;
        }

        // How far that branch has moved since the fork. It breaks a tie
        // between two candidates that fork at the same commit, and it breaks
        // it the right way: you cut from a branch where it was, so the parent
        // is usually the one still standing at the fork, while a sibling has
        // walked on.
        let moved = inner
            .graph_ahead_behind(*other_tip, shared)
            .map(|(ahead, _)| ahead)
            .unwrap_or(usize::MAX);

        let better = match &best {
            None => true,
            Some((_, kept, kept_moved)) => match shared == *kept {
                true => moved < *kept_moved,
                false => inner.graph_descendant_of(shared, *kept).unwrap_or(false),
            },
        };
        if better {
            best = Some((other.clone(), shared, moved));
        }
    }

    best.map(|(name, _, _)| name)
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
        self.board_against(&self.trunk()?, pulls)
    }

    /// The work cut from one particular branch.
    ///
    /// This is all a subtask is. The board the window opens on is this one
    /// measured against the trunk; going into a card is the same board
    /// measured against that card's branch. The columns do not change what
    /// they mean, they change what they are measured against, and that is the
    /// whole of it: "done" for a subtask is having gone into its parent, not
    /// into the trunk, which is exactly what being finished means for a piece
    /// of a larger job.
    ///
    /// A branch appears on one board only, the one of the branch it was cut
    /// from. Which is why a card says how many were cut from it: otherwise a
    /// branch could be nowhere anybody thought to look.
    pub fn board_against(&self, base: &str, pulls: Option<&PullRequests>) -> Result<Board> {
        let tip = self.tip_of(base)?;
        let trunk = self.trunk()?;
        let trunk_tip = self.tip_of(&trunk)?;

        let head = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string));

        // The tips the base took in at some merge. It is the only thing
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

        let tips = self.branch_tips()?;
        let parents = parentage(&self.inner, trunk_tip, &tips);

        let mut children: HashMap<&str, usize> = HashMap::new();
        for parent in parents.values().flatten() {
            *children.entry(parent.as_str()).or_default() += 1;
        }

        // The trunk is not a branch anybody was cut from on purpose, so the
        // cards that belong to it are the ones with no parent at all.
        let wanted = (base != trunk).then(|| base.to_string());

        let mut cards = Vec::new();
        for (name, target) in &tips {
            if name == base {
                continue;
            }
            if parents.get(name).cloned().flatten() != wanted {
                continue;
            }

            let (name, target) = (name.clone(), *target);
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
                children: children.get(name.as_str()).copied().unwrap_or(0),
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
            trunk: base.to_string(),
            asked: pulls.is_some(),
        })
    }

    /// Which branch each branch was cut from, as far as the graph can tell.
    pub fn parents(&self) -> Result<Parents> {
        let trunk_tip = self.tip_of(&self.trunk()?)?;
        Ok(parentage(&self.inner, trunk_tip, &self.branch_tips()?))
    }

    /// Every local branch and where it points, in order by name.
    ///
    /// In order on purpose. Two equally good candidates for a parent have to
    /// be settled the same way every time, and the order branches come out of
    /// libgit2 in is not something to build an answer on.
    fn branch_tips(&self) -> Result<Vec<(String, Oid)>> {
        let mut tips = Vec::new();
        for branch in self.inner.branches(Some(BranchType::Local))? {
            let (branch, _) = branch?;
            let Some(name) = branch.name()?.map(str::to_string) else {
                continue;
            };
            let Ok(object) = branch.get().peel(ObjectType::Commit) else {
                continue;
            };
            tips.push((name, object.id()));
        }
        tips.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(tips)
    }

    fn tip_of(&self, name: &str) -> Result<Oid> {
        Ok(self
            .inner
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("no branch called '{name}'"))?
            .get()
            .peel(ObjectType::Commit)?
            .id())
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
