//! Tags: marking a commit with a name that does not move.

use anyhow::{Context, Result};
use git2::Oid;

use crate::repo::Repo;

impl Repo {
    /// Puts a tag on a commit.
    ///
    /// With a message it comes out annotated: it carries who put it there, when
    /// and why, and it is an object of the repository in its own right. Without
    /// a message it comes out lightweight, which is just a name pointing at the
    /// commit. It is the same difference there is between `git tag -a` and plain
    /// `git tag`, and here you choose it by writing or not, with no switch to
    /// explain.
    ///
    /// It does not walk over one that already exists. A tag is worth something
    /// precisely because it does not move; moving it quietly would take away the
    /// only thing it has.
    pub fn tag(&self, name: &str, target: Oid, message: &str) -> Result<()> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "the tag needs a name");

        let object = self
            .inner
            .find_object(target, None)
            .context("that commit is no longer in the repository")?;
        let message = message.trim();

        if message.is_empty() {
            self.inner.tag_lightweight(name, &object, false).map(|_| ())
        } else {
            let tagger = self
                .inner
                .signature()
                .context("set user.name and user.email in Git first")?;
            self.inner.tag(name, &object, &tagger, message, false).map(|_| ())
        }
        .with_context(|| format!("could not create '{name}'"))
    }

    /// Takes a tag off. The commit it pointed at stays where it was.
    ///
    /// Only the one here: one on a remote stays there until it is deleted there,
    /// and that is not done from the app yet.
    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.inner
            .tag_delete(name)
            .with_context(|| format!("could not delete '{name}'"))
    }
}
