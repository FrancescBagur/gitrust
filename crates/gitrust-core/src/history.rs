//! Taking a commit back, taking one along, and moving a branch.
//!
//! The three things you do to a commit that is already written. They have
//! very little in common except the thing that matters most here: each of
//! them can lose work, and each of them is arranged so that it cannot lose it
//! by halves.
//!
//! Taking along and taking back are worked out in memory first, exactly as a
//! merge is. If the result would clash, nothing is written and nothing is
//! checked out: a repository half-way through a cherry-pick is a place you
//! have to know how to get out of, and this application has no conflict
//! editor to get anybody out of it.
//!
//! Moving a branch is the one that cannot be made safe, only honest. Of its
//! three kinds, two keep your files and one does not, and the one that does
//! not is the only operation in gitrust that destroys work nothing else can
//! give back. It says so in its own name, and the caller is expected to ask
//! before it is used.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, Oid, ResetType};

use crate::repo::Repo;

/// How far a branch move takes the rest of the repository with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// The branch moves and nothing else does. Everything those commits held
    /// is left staged, ready to be written again as one commit. Git calls
    /// this `--soft`.
    Keep,
    /// The branch and the staged changes move; your files stay as they are.
    /// What those commits held is sitting in the working tree, unstaged.
    /// Git calls this `--mixed`, and it is what `git reset` does unasked.
    Unstage,
    /// The branch, the staged changes and your files all go back. Anything
    /// not committed is gone, and no stash holds it. Git calls this `--hard`.
    Discard,
}

impl Move {
    fn kind(self) -> ResetType {
        match self {
            Move::Keep => ResetType::Soft,
            Move::Unstage => ResetType::Mixed,
            Move::Discard => ResetType::Hard,
        }
    }

    /// What it does, in the words somebody deciding would want.
    pub fn says(self) -> &'static str {
        match self {
            Move::Keep => "keeping everything staged",
            Move::Unstage => "keeping your files, unstaging them",
            Move::Discard => "throwing away everything not committed",
        }
    }
}

impl Repo {
    /// Writes the change a commit made onto the current branch, again.
    ///
    /// The commit is not moved and nothing is taken from where it was: this
    /// writes a new commit here that does the same thing. Its message comes
    /// along, since the point of taking a change along is that it is the same
    /// change.
    pub fn cherry_pick(&self, oid: Oid) -> Result<String> {
        let (head, head_commit) = self.here()?;
        let commit = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {}", short(oid)))?;

        anyhow::ensure!(
            commit.parent_count() < 2,
            "{} is a merge, and there is no one change to take along",
            short(oid)
        );
        // The tip itself counts as had. Git's own answer to "is this commit a
        // descendant of itself" is no, which would have let the tip through
        // to be picked onto itself.
        anyhow::ensure!(
            head_commit.id() != oid && !self.inner.graph_descendant_of(head_commit.id(), oid)?,
            "{head} already has {}",
            short(oid)
        );

        // Worked out in memory. Only a clean result is written.
        let mut index = self
            .inner
            .cherrypick_commit(&commit, &head_commit, 0, None)
            .context("could not work the change out")?;
        anyhow::ensure!(
            !index.has_conflicts(),
            "{} clashes with {head}, and there is no conflict editor yet",
            short(oid)
        );

        let tree = self.inner.find_tree(index.write_tree_to(&self.inner)?)?;
        if tree.id() == head_commit.tree_id() {
            return Ok(format!("{head} already looks exactly like that"));
        }

        let signature = self.who()?;
        let message = commit.message().unwrap_or("a change taken along");
        self.inner.commit(
            Some("HEAD"),
            &commit.author(),
            &signature,
            message,
            &tree,
            &[&head_commit],
        )?;
        self.checkout_here()?;

        Ok(format!("{} is on {head} now too", short(oid)))
    }

    /// Writes a commit that undoes another one.
    ///
    /// Nothing is rewritten and nothing disappears: the commit being undone
    /// stays exactly where it is, and a new one on top says what it said
    /// backwards. It is the only way to take something back that has already
    /// gone out to other people.
    pub fn revert(&self, oid: Oid) -> Result<String> {
        let (head, head_commit) = self.here()?;
        let commit = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {}", short(oid)))?;

        anyhow::ensure!(
            commit.parent_count() < 2,
            "{} is a merge, and undoing one takes saying which side to keep",
            short(oid)
        );

        let mut index = self
            .inner
            .revert_commit(&commit, &head_commit, 0, None)
            .context("could not work the undo out")?;
        anyhow::ensure!(
            !index.has_conflicts(),
            "undoing {} clashes with what {head} has since, and there is no conflict editor yet",
            short(oid)
        );

        let tree = self.inner.find_tree(index.write_tree_to(&self.inner)?)?;
        if tree.id() == head_commit.tree_id() {
            return Ok(format!("{head} does not have what {} did", short(oid)));
        }

        let subject = commit.summary().unwrap_or("a commit").to_string();
        let signature = self.who()?;
        self.inner.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &format!("revert: {subject}\n\nThis undoes {}.\n", oid),
            &tree,
            &[&head_commit],
        )?;
        self.checkout_here()?;

        Ok(format!("{} is undone on {head}", short(oid)))
    }

    /// Moves the current branch to another commit.
    ///
    /// The commits between here and there are not deleted -- Git keeps them
    /// until it tidies up, and the reflog still points at where the branch
    /// was -- but nothing in gitrust will show them again, so for everybody
    /// using it they are gone.
    ///
    /// [`Move::Discard`] is the only thing in here that throws away work no
    /// stash and no reflog can give back, which is why it is named for what
    /// it does and why the caller is expected to have asked first.
    pub fn move_branch(&self, oid: Oid, how: Move) -> Result<String> {
        let (head, head_commit) = self.here()?;
        let target = self
            .inner
            .find_commit(oid)
            .with_context(|| format!("no commit {}", short(oid)))?;

        if target.id() == head_commit.id() {
            return Ok(format!("{head} is already there"));
        }

        // Anything other than discarding keeps your files, so it is only
        // worth stopping for when the files are what is about to go.
        if how != Move::Discard {
            anyhow::ensure!(
                self.status()?.is_clean() || how == Move::Unstage || how == Move::Keep,
                "commit or stash what you have half-done first"
            );
        }

        self.inner
            .reset(target.as_object(), how.kind(), None)
            .with_context(|| format!("could not move {head}"))?;

        let back = self.inner.graph_descendant_of(head_commit.id(), oid)?;
        Ok(match back {
            true => format!("{head} is back at {}, {}", short(oid), how.says()),
            false => format!("{head} is at {} now, {}", short(oid), how.says()),
        })
    }

    /// The branch we are on and the commit it points at.
    fn here(&self) -> Result<(String, git2::Commit<'_>)> {
        let head = self.inner.head().context("the repository has no commits")?;
        anyhow::ensure!(
            !self.inner.head_detached()?,
            "you are not on a branch: switch to one first"
        );
        let name = head.shorthand().unwrap_or("HEAD").to_string();
        let commit = head.peel_to_commit()?;
        Ok((name, commit))
    }

    /// Brings the working tree up to whatever HEAD now says.
    ///
    /// Forced, and that is safe here: it is run straight after writing a
    /// commit whose tree was worked out from what was already there, so there
    /// is nothing of yours it can be walking over.
    fn checkout_here(&self) -> Result<()> {
        let head = self.inner.head()?.peel_to_commit()?;
        self.inner
            .checkout_tree(head.as_object(), Some(CheckoutBuilder::new().force()))?;
        Ok(())
    }
}

fn short(oid: Oid) -> String {
    oid.to_string()[..7].to_string()
}
