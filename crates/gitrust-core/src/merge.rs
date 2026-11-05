//! Joining a branch with the current one.
//!
//! For a long time what was not here mattered as much as what was: a clash
//! touched nothing, because a half-done repository is a place you have to
//! know how to get out of and there was no way out in the application. There
//! is one now -- [`crate::conflict`] -- so both ways of joining stop where
//! they clash and leave it standing to be decided.
//!
//! The two stop differently, because they are different shapes. A merge is
//! one clash with one answer: decide the files, write the commit, done. A
//! rebase is one clash *per commit*, so it stops at the first, waits to be
//! carried on, and may well stop again at the next.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, ErrorCode, ObjectType};

use crate::repo::Repo;

impl Repo {
    /// Merges a branch into the current one. A local or a remote one will do.
    ///
    /// If the current one has not moved since the other left it, there are not
    /// two histories to join but one to carry on, and the branch moves up
    /// without writing any commit.
    ///
    /// If they really do have to be joined, the merge is worked out in memory
    /// first. Only when it comes out clean is the commit written and the tree
    /// brought up to date, so that a clash leaves the repository exactly as it was.
    pub fn merge(&self, name: &str) -> Result<String> {
        // One at a time. Starting a merge on top of a half-done anything
        // means two sets of markers in one working tree and no way to tell
        // which decision belongs to which.
        self.nothing_half_done()?;

        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let current = head.shorthand().unwrap_or("HEAD").to_string();
        anyhow::ensure!(current != name, "{name} is the branch you are on");

        let reference = self
            .inner
            .resolve_reference_from_short_name(name)
            .with_context(|| format!("no branch called '{name}'"))?;
        let theirs = self.inner.reference_to_annotated_commit(&reference)?;

        let (analysis, _) = self.inner.merge_analysis(&[&theirs])?;
        if analysis.is_up_to_date() {
            return Ok(format!("{name} is already inside {current}"));
        }

        let commit = self.inner.find_commit(theirs.id())?;

        if analysis.is_fast_forward() {
            // Here safe mode will do: what you have half-done and does not get
            // in the way travels with you, as it does when switching branches.
            self.inner
                .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
                .context("unsaved changes are in the way; commit them or stash them")?;
            self.inner
                .find_reference(&refname)?
                .set_target(theirs.id(), "merge: fast-forward")?;

            return Ok(format!("{current} moves up to {name}, with no merge to write"));
        }

        // From here on a commit has to be written and the whole tree rewritten,
        // and that cannot be done with half-finished work on top.
        anyhow::ensure!(
            self.status()?.is_clean(),
            "commit or stash what you have half-done before merging"
        );

        let ours = head.peel(ObjectType::Commit)?.id();
        let ours = self.inner.find_commit(ours)?;
        let mut index = self.inner.merge_commits(&ours, &commit, None)?;

        // It clashes. For a long time this refused here and touched nothing,
        // because a half-merged repository is a place you have to know how to
        // get out of and there was no way out in the application. There is
        // now, so the merge is really started: the index gets both sides, the
        // files get their markers, and MERGE_HEAD says what is coming in.
        if index.has_conflicts() {
            let files = conflicting(&index);
            self.inner
                .merge(&[&theirs], None, Some(CheckoutBuilder::new().safe()))
                .with_context(|| format!("could not start the merge of {name}"))?;
            return Ok(format!(
                "{name} clashes with {current} in {files}. Decide each one and \
                 the merge can be written"
            ));
        }

        let tree = index.write_tree_to(&self.inner)?;
        let tree = self.inner.find_tree(tree)?;
        let signature = self
            .inner
            .signature()
            .context("set user.name and user.email in Git first")?;

        self.inner.commit(
            Some(&refname),
            &signature,
            &signature,
            &format!("Merge {name} into {current}"),
            &tree,
            &[&ours, &commit],
        )?;

        // The branch is already where it belongs; now the disk. Forcing here
        // walks over nothing: the tree was clean before we started.
        self.inner
            .checkout_head(Some(CheckoutBuilder::new().force()))
            .context("the merge was written but the working tree could not be updated")?;

        Ok(format!("{name} merged into {current}"))
    }
}

/// What dropping one branch onto another is asking for.
///
/// All three read the same way round, and it is the way round the gesture
/// means: the branch you dropped **on** is the one that moves, and the branch
/// you dragged is where the commits come from. Drag `feature` onto `main` and
/// it is `main` that ends up different.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// Write a commit that has both sides.
    Merge,
    /// Replay the target's own commits on top of the other, leaving one line
    /// instead of two and a knot.
    Rebase,
    /// Move the target up, writing nothing. Only possible when it has not
    /// moved on its own since the two parted.
    FastForward,
}

impl Join {
    /// What it would do, in the words somebody deciding would want. `from` is
    /// the branch dragged, `onto` the one it was dropped on.
    pub fn says(self, from: &str, onto: &str) -> String {
        match self {
            Join::Merge => format!("Merge {from} into {onto}"),
            Join::Rebase => format!("Rebase {onto} onto {from}"),
            Join::FastForward => format!("Move {onto} up to {from}"),
        }
    }

    /// The same thing while it is happening, for whatever says what the
    /// application is busy with.
    pub fn doing(self, from: &str, onto: &str) -> String {
        match self {
            Join::Merge => format!("merging {from} into {onto}"),
            Join::Rebase => format!("rebasing {onto} onto {from}"),
            Join::FastForward => format!("moving {onto} up to {from}"),
        }
    }
}

impl Repo {
    /// Does to two named branches what the three above do to the current one.
    ///
    /// Every one of them works on whatever branch you are standing on, so this
    /// stands on the right one first. That is the whole of it: there is no
    /// second way of merging, only a different place to be standing when it
    /// happens.
    ///
    /// If the switch fails nothing has been touched. If what comes after fails
    /// you are left on `onto`, which is where you were going anyway -- and the
    /// warning says so, because the next thing you do would otherwise happen
    /// on a branch you did not know you were on.
    pub fn join(&self, from: &str, onto: &str, how: Join) -> Result<String> {
        anyhow::ensure!(from != onto, "{from} is the branch it would go into");

        let moved = self.head_name() != onto;
        if moved {
            self.checkout_branch(onto)?;
        }

        let note = match how {
            Join::Merge => self.merge(from),
            Join::Rebase => self.rebase_onto(from),
            Join::FastForward => self.fast_forward_to(from),
        };

        // Only said when it is still true. A rebase that stopped at a clash
        // leaves HEAD off any branch at all until it is carried on or thrown
        // away, and telling somebody they are on `onto` would have them
        // reading the next thing they do on a branch they are not standing on.
        let standing = self.head_name() == onto;

        match (note, moved) {
            (Ok(note), true) if standing => Ok(format!("{note}, and you are now on {onto}")),
            (Ok(note), _) => Ok(note),
            (Err(err), true) if standing => Err(err.context(format!("you are now on {onto}"))),
            (Err(err), _) => Err(err),
        }
    }

    /// Whether one branch could move up to another without writing anything.
    ///
    /// True only when the first has nothing of its own since the two parted.
    /// It is what somebody has to know *before* they press anything: offering
    /// a button that can only answer "there is a merge to write here" is
    /// asking a question whose answer was already known.
    pub fn has_room(&self, branch: &str, up_to: &str) -> Result<bool> {
        let ours = self
            .inner
            .resolve_reference_from_short_name(branch)
            .with_context(|| format!("no branch called '{branch}'"))?
            .peel_to_commit()?
            .id();
        let theirs = self
            .inner
            .resolve_reference_from_short_name(up_to)
            .with_context(|| format!("no branch called '{up_to}'"))?
            .peel_to_commit()?
            .id();

        // A commit is not its own descendant, and two branches on the same
        // commit have no room between them either way.
        Ok(ours != theirs && self.inner.graph_descendant_of(theirs, ours)?)
    }

    /// Moves the current branch up to another one, and nothing else.
    ///
    /// What it refuses is the point. A merge will write a merge commit when
    /// the two have both moved; this says there is no moving up to be done
    /// and leaves the repository alone, which is what somebody who asked for
    /// a fast forward and not a merge wanted to hear.
    pub fn fast_forward_to(&self, name: &str) -> Result<String> {
        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let current = head.shorthand().unwrap_or("HEAD").to_string();
        anyhow::ensure!(current != name, "{name} is the branch you are on");

        let reference = self
            .inner
            .resolve_reference_from_short_name(name)
            .with_context(|| format!("no branch called '{name}'"))?;
        let theirs = self.inner.reference_to_annotated_commit(&reference)?;

        let (analysis, _) = self.inner.merge_analysis(&[&theirs])?;
        if analysis.is_up_to_date() {
            return Ok(format!("{current} already has everything {name} has"));
        }
        anyhow::ensure!(
            analysis.is_fast_forward(),
            "{current} has moved since it left {name}: this would take a merge"
        );

        let commit = self.inner.find_commit(theirs.id())?;
        self.inner
            .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
            .context("unsaved changes are in the way; commit them or stash them")?;
        self.inner
            .find_reference(&refname)?
            .set_target(theirs.id(), "merge: fast-forward")?;

        Ok(format!("{current} moves up to {name}"))
    }

    pub fn land(&self, branch: &str) -> Result<String> {
        let trunk = self.trunk()?;
        anyhow::ensure!(branch != trunk, "{branch} is the main branch");
        self.land_into(&trunk, branch)
    }

    /// The same, into whichever branch the work actually belongs to.
    ///
    /// A subtask is finished when it goes into its parent, not when it reaches
    /// the trunk, so what it is merged into is the board you are looking at
    /// and not always the main branch. Dragging a card to "done" means the
    /// same thing at every depth; what changes is where "done" is.
    pub fn land_into(&self, base: &str, branch: &str) -> Result<String> {
        self.join(branch, base, Join::Merge)
    }

    /// Replays the current branch's commits on top of another.
    ///
    /// It leaves the history straight, which is what it is for. A commit that
    /// clashes no longer undoes everything: the rebase stops where it is,
    /// with that commit's clashing files in front of you and the rest of the
    /// sequence still waiting, and [`Repo::carry_on_rebase`] takes it from
    /// there. Undoing it was what this did for a long time, and it threw away
    /// every commit that had already gone up clean along with the one that
    /// could not.
    pub fn rebase_onto(&self, upstream: &str) -> Result<String> {
        // One at a time, for the same reason a merge is.
        self.nothing_half_done()?;
        anyhow::ensure!(
            self.status()?.is_clean(),
            "commit or stash what you have half-done before rebasing"
        );

        let head = self.inner.head().context("the repository has no commits")?;
        let branch = self.inner.reference_to_annotated_commit(&head)?;

        let onto = self
            .inner
            .resolve_reference_from_short_name(upstream)
            .with_context(|| format!("no branch called '{upstream}'"))?;
        let onto = self.inner.reference_to_annotated_commit(&onto)?;

        let signature = self
            .inner
            .signature()
            .context("set user.name and user.email in Git first")?;

        let mut rebase = self.inner.rebase(Some(&branch), Some(&onto), None, None)?;
        self.replay(&mut rebase, &signature, 0)
    }

    /// Picks up a rebase that stopped at a clash and carries it on.
    ///
    /// Everything still clashing has to be decided first, exactly as for a
    /// merge. What is different is what happens next: the decisions are
    /// written as *that* commit, and then the sequence goes on to the one
    /// after it, which may clash too. So this is not the end of anything. It
    /// is the step you take as many times as the rebase needs, and the last
    /// one of them is what finishes it.
    pub fn carry_on_rebase(&self) -> Result<String> {
        anyhow::ensure!(
            self.half_done() == crate::conflict::Half::Rebase,
            "there is no half-done rebase here"
        );

        self.nothing_left_to_decide()?;

        let signature = self
            .inner
            .signature()
            .context("set user.name and user.email in Git first")?;

        // Reopening is what makes this possible at all: the rebase is not
        // held in memory between one press and the next, it is on disk in
        // `.git`, which is also why it survives closing the application.
        let mut rebase = self
            .inner
            .open_rebase(None)
            .context("this rebase was not started here and cannot be carried on here")?;

        // How far the sequence had got, counted for the message at the end.
        // On a rebase picked up again this is all the count there is, and it
        // counts a commit that turned out to be up there already -- which the
        // same rebase run in one go would not have counted. Rather than lie
        // precisely, it says how far it got.
        let done = rebase.operation_current().map(|at| at + 1).unwrap_or(0);

        match rebase.commit(None, &signature, None) {
            Ok(_) => {}
            // Deciding every file the way the branch underneath already had
            // it leaves this commit with nothing in it. That is an answer,
            // not a failure: the commit falls away and the rest carries on.
            Err(err) if err.code() == ErrorCode::Applied => {}
            Err(err) => {
                return Err(anyhow::Error::from(err)
                    .context("the files are decided but that commit could not be written"))
            }
        }

        self.replay(&mut rebase, &signature, done)
    }

    /// What the rebase is going onto, which git2 does not hand out.
    ///
    /// libgit2 writes it beside the rest of the rebase's state and keeps it
    /// to itself. Reading it off disk is reaching around the library, and it
    /// is done for one reason: that name is what every sentence on the screen
    /// about this rebase is about, and "the other one" is not a name. If the
    /// file is not there -- another backend, another version -- the sentence
    /// loses a name and nothing else.
    fn rebase_onto_name(&self) -> Option<String> {
        let text =
            std::fs::read_to_string(self.inner.path().join("rebase-merge/onto_name")).ok()?;
        let name = text.trim();
        (!name.is_empty()).then(|| shorten(Some(name)).unwrap_or_else(|| name.to_string()))
    }

    /// Works through what is left of a rebase, stopping at the first clash.
    ///
    /// Shared by starting one and carrying one on, because after the first
    /// commit is written there is no difference between the two: there is a
    /// sequence with some of it done and the rest to go.
    fn replay(
        &self,
        rebase: &mut git2::Rebase,
        signature: &git2::Signature,
        mut replayed: usize,
    ) -> Result<String> {
        let branch = shorten(rebase.orig_head_name()).unwrap_or_else(|| "this branch".to_string());
        let onto = self.rebase_onto_name().unwrap_or_else(|| "the other one".to_string());
        let total = rebase.len();

        while let Some(step) = rebase.next() {
            if let Err(err) = step {
                let _ = rebase.abort();
                return Err(anyhow::Error::from(err)
                    .context("the rebase could not go on, so it was undone"));
            }

            let index = self.inner.index()?;
            if index.has_conflicts() {
                // Left standing on purpose: the commits already replayed stay
                // replayed, and only this one is waiting on an answer.
                let files = conflicting(&index);
                let at = rebase.operation_current().map(|at| at + 1).unwrap_or(1);
                return Ok(format!(
                    "{branch} clashes with {onto} in {files}, at commit {at} of {total}. \
                     Decide each one and the rebase carries on"
                ));
            }

            match rebase.commit(None, signature, None) {
                Ok(_) => replayed += 1,
                // The change was already up there: that is not a failure, it is
                // one commit less to replay.
                Err(err) if err.code() == ErrorCode::Applied => {}
                Err(err) => {
                    let _ = rebase.abort();
                    return Err(err.into());
                }
            }
        }

        rebase.finish(Some(signature))?;

        Ok(match replayed {
            0 => format!("{branch} moved onto {onto}, with nothing to replay"),
            1 => format!("1 commit replayed on top of {onto}"),
            n => format!("{n} commits replayed on top of {onto}"),
        })
    }
}

/// A ref name the way somebody would say it aloud.
///
/// The rebase keeps full names, because that is what it has to put back when
/// it is done. Nobody wants to read `refs/heads/` in a sentence.
fn shorten(name: Option<&str>) -> Option<String> {
    let name = name?;
    Some(name.strip_prefix("refs/heads/").unwrap_or(name).to_string())
}

/// The files that clash, named the way you would name them aloud: three and
/// whatever is left. The whole list fits in no status bar, and is not needed.
pub(crate) fn conflicting(index: &git2::Index) -> String {
    let Ok(conflicts) = index.conflicts() else {
        return "some file".to_string();
    };

    let paths: Vec<String> = conflicts
        .filter_map(|conflict| conflict.ok())
        .filter_map(|conflict| conflict.our.or(conflict.their).or(conflict.ancestor))
        .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
        .collect();

    let shown: Vec<&str> = paths.iter().take(3).map(String::as_str).collect();
    let rest = paths.len() - shown.len();

    match rest {
        0 => shown.join(", "),
        1 => format!("{} and one more", shown.join(", ")),
        n => format!("{} and {n} more", shown.join(", ")),
    }
}
