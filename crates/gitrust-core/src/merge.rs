//! Joining a branch with the current one.
//!
//! What is not here matters as much as what is: if the merge comes out with
//! conflicts, nothing is touched. A half-merged repository is a place you have
//! to know how to get out of, and while the app has no conflict editor it
//! cannot leave anyone in there.

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
        let half = self.half_done();
        anyhow::ensure!(
            half == crate::conflict::Half::None,
            "{} is half-done here. Finish it or abandon it first",
            half.says()
        );

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

        match (note, moved) {
            (Ok(note), true) => Ok(format!("{note}, and you are now on {onto}")),
            (Ok(note), false) => Ok(note),
            (Err(err), true) => Err(err.context(format!("you are now on {onto}"))),
            (Err(err), false) => Err(err),
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
    /// It leaves the history straight, which is what it is for. If replaying a
    /// commit clashes with something, the whole rebase is aborted: libgit2 knows
    /// how to undo it and put the branch back, so there is no way to be left
    /// halfway. Finishing it would need the conflict editor.
    pub fn rebase_onto(&self, upstream: &str) -> Result<String> {
        anyhow::ensure!(
            self.status()?.is_clean(),
            "commit or stash what you have half-done before rebasing"
        );

        let head = self.inner.head().context("the repository has no commits")?;
        let current = head.shorthand().unwrap_or("HEAD").to_string();
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
        let mut replayed = 0;

        while let Some(step) = rebase.next() {
            if let Err(err) = step {
                let _ = rebase.abort();
                return Err(err.into());
            }

            let index = self.inner.index()?;
            if index.has_conflicts() {
                let files = conflicting(&index);
                let _ = rebase.abort();
                anyhow::bail!(
                    "{current} clashes with {upstream} in {files}. There is no conflict \
                     editor yet, so the rebase was undone and nothing changed"
                );
            }

            match rebase.commit(None, &signature, None) {
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

        rebase.finish(Some(&signature))?;

        Ok(match replayed {
            0 => format!("{current} moved onto {upstream}, with nothing to replay"),
            1 => format!("1 commit replayed on top of {upstream}"),
            n => format!("{n} commits replayed on top of {upstream}"),
        })
    }
}

/// The files that clash, named the way you would name them aloud: three and
/// whatever is left. The whole list fits in no status bar, and is not needed.
fn conflicting(index: &git2::Index) -> String {
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
