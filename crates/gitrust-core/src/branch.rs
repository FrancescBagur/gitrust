//! Making branches and switching between them.
//!
//! Switching branches writes more to your disk than anything else in here, so
//! everything in this module is arranged so that when something goes wrong, it
//! goes wrong before anything has been touched.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, BranchType, ErrorCode, ObjectType};

use crate::repo::Repo;

impl Repo {
    /// Makes a branch where HEAD is and switches to it.
    ///
    /// The working tree is left alone: the new branch points at the same commit
    /// as the old one, so whatever you had half-done is still there and can be
    /// committed straight onto the new branch. It is how you notice late that
    /// you were writing on the wrong branch.
    pub fn branch_here(&self, name: &str) -> Result<()> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "the branch needs a name");

        let commit = self
            .inner
            .head()
            .and_then(|head| head.peel_to_commit())
            .context("the repository has no commits to branch from")?;

        self.inner
            .branch(name, &commit, false)
            .with_context(|| format!("could not create '{name}'"))?;
        self.inner.set_head(&format!("refs/heads/{name}"))?;
        Ok(())
    }

    /// Makes a branch at a commit, without switching to it.
    ///
    /// The one you are on does not move and nothing is written to your disk:
    /// this only puts a name on a commit that was already there.
    pub fn branch_at(&self, name: &str, oid: git2::Oid) -> Result<String> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "the branch needs a name");
        anyhow::ensure!(
            self.inner.find_branch(name, BranchType::Local).is_err(),
            "there is already a branch called '{name}'"
        );

        let commit = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {oid}"))?;
        self.inner
            .branch(name, &commit, false)
            .with_context(|| format!("could not create '{name}'"))?;

        Ok(format!("{name} starts at {}", &oid.to_string()[..7]))
    }

    /// Gives a branch another name.
    ///
    /// A branch open in a folder of its own is refused rather than renamed:
    /// that folder's HEAD names the branch, and renaming it from here would
    /// leave the folder pointing at a name that is no longer there.
    pub fn rename_branch(&self, old: &str, new: &str) -> Result<String> {
        let new = new.trim();
        anyhow::ensure!(!new.is_empty(), "the branch needs a name");
        anyhow::ensure!(old != new, "that is the name it has already");
        anyhow::ensure!(
            self.inner.find_branch(new, BranchType::Local).is_err(),
            "there is already a branch called '{new}'"
        );
        if let Some(held) = self.held_by(old) {
            anyhow::bail!(
                "{old} is open in {}; close that folder first",
                held.path.display()
            );
        }

        // Worked out before anything moves, because afterwards the old name
        // is gone and there is nothing left to compare against.
        let on_it = self.head_name() == old;

        let mut branch = self
            .inner
            .find_branch(old, BranchType::Local)
            .with_context(|| format!("no branch called '{old}'"))?;
        branch
            .rename(new, false)
            .with_context(|| format!("could not rename '{old}'"))?;

        // The reference moved; HEAD is ours to keep pointing at something
        // that exists.
        if on_it {
            self.inner.set_head(&format!("refs/heads/{new}"))?;
        }

        Ok(format!("{old} is called {new} now"))
    }

    /// Goes to a commit without being on a branch.
    ///
    /// Git calls this a detached HEAD. The thing worth knowing about it is
    /// that a commit written here belongs to no branch and nothing will point
    /// at it: make a branch before writing anything worth keeping.
    pub fn checkout_commit(&self, oid: git2::Oid) -> Result<String> {
        let commit = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {oid}"))?;

        // The tree first, HEAD after, as everywhere else here: if unsaved
        // changes are in the way, HEAD never moved.
        self.inner
            .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
            .context("unsaved changes are in the way; commit them or stash them")?;
        self.inner.set_head_detached(oid)?;

        Ok(format!(
            "you are at {} now, on no branch",
            &oid.to_string()[..7]
        ))
    }

    /// Switches to a local branch that already exists.
    ///
    /// The tree first, HEAD after. If the tree fails -- because unsaved changes
    /// clash with what would have to be written -- HEAD stays where it was and
    /// there is nothing half-done to put right.
    ///
    /// The switch runs in safe mode, which is `git checkout`'s own: what you
    /// have changed and does not get in the way travels with you to the new
    /// branch, and what does get in the way stops the whole switch. Unsaved
    /// changes are yours; it is not our place to walk over them to get by.
    pub fn checkout_branch(&self, name: &str) -> Result<String> {
        let reference = self
            .inner
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("no branch called '{name}'"))?
            .into_reference();

        let refname = reference.name().context("the branch has no name")?.to_string();
        let commit = reference.peel_to_commit()?;

        // A branch open in another checkout cannot be opened here as well, and
        // Git is right about that: two folders writing to one branch are two
        // different ideas of what it holds. Saying where it is open is worth
        // more than saying no, because it is an answer you can act on.
        if let Some(held) = self.held_by(name) {
            anyhow::bail!("{name} is open in {}", held.path.display());
        }

        self.inner
            .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
            .map_err(|err| blocked(err, name))?;
        self.inner.set_head(&refname)?;

        Ok(format!("you are now on {name}"))
    }

    /// Switches to a remote branch by creating its local counterpart.
    ///
    /// It is what `git checkout branch` does when the branch only exists on the
    /// remote: it makes the local one where the remote one points and leaves it
    /// tracking it. If the local one already exists it is left alone and you go
    /// to it, because it may hold work the remote does not know about yet.
    pub fn checkout_remote(&self, name: &str) -> Result<String> {
        let (_, local) = name
            .split_once('/')
            .context("a remote branch is named remote/branch")?;

        if self.inner.find_branch(local, BranchType::Local).is_ok() {
            return self.checkout_branch(local);
        }

        let commit = self
            .inner
            .find_branch(name, BranchType::Remote)
            .with_context(|| format!("no remote branch called '{name}'"))?
            .into_reference()
            .peel_to_commit()?;

        self.inner
            .branch(local, &commit, false)
            .with_context(|| format!("could not create '{local}'"))?
            .set_upstream(Some(name))?;

        self.checkout_branch(local)
    }

    /// Deletes a local branch.
    ///
    /// Not the current one: deleting it would leave HEAD pointing at something
    /// that no longer exists, and there is no way out of that from the app.
    pub fn delete_branch(&self, name: &str) -> Result<()> {
        let head = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string));
        anyhow::ensure!(
            head.as_deref() != Some(name),
            "{name} is the branch you are on: switch to another one first"
        );

        self.inner
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("no branch called '{name}'"))?
            .delete()
            .with_context(|| format!("could not delete '{name}'"))
    }

    /// How many of that branch's commits are nowhere else.
    ///
    /// It is exactly what deleting it would lose. Zero means everything of its
    /// own is already somewhere else -- another branch, a remote, a tag -- and
    /// that deleting it takes nothing down with it.
    ///
    /// It is worked out by walking its history and hiding that of every other
    /// reference, which is what `git branch -d` does to decide whether to
    /// refuse.
    pub fn unmerged(&self, name: &str) -> Result<usize> {
        let refname = format!("refs/heads/{name}");
        let target = self
            .inner
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("no branch called '{name}'"))?
            .get()
            .peel(ObjectType::Commit)?
            .id();

        let mut walk = self.inner.revwalk()?;
        walk.push(target)?;

        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(other) = reference.name() else {
                continue;
            };
            if other == refname {
                continue;
            }
            if let Ok(object) = reference.peel(ObjectType::Commit) {
                walk.hide(object.id())?;
            }
        }

        Ok(walk.count())
    }
}

/// Turns libgit2's "N conflicts prevent checkout" into something you can act
/// on. The number of conflicts is no use to anyone; knowing a stash is one
/// click away is.
fn blocked(err: git2::Error, name: &str) -> anyhow::Error {
    if err.code() == ErrorCode::Conflict {
        anyhow::anyhow!(
            "unsaved changes clash with {name}: commit them, or set them aside with Stash"
        )
    } else {
        err.into()
    }
}
