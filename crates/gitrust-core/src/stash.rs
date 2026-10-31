//! The pile of changes set aside.

use anyhow::{Context, Result};
use git2::{Oid, StashFlags};

use crate::repo::Repo;

/// One thing set aside. The index is its place in the pile: 0 is the last one
/// stashed, and it changes as soon as any of the ones above it is popped or
/// dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub id: Oid,
    pub time: i64,
}

impl Repo {
    /// The whole pile, newest first.
    pub fn stashes(&mut self) -> Result<Vec<StashEntry>> {
        let mut out = Vec::new();
        self.inner.stash_foreach(|index, message, id| {
            out.push(StashEntry {
                index,
                message: message.to_string(),
                id: *id,
                time: 0,
            });
            true
        })?;

        // The date lives in the stash's commit, not in the pile, so it has to
        // be fetched. If it cannot be read it stays at zero and the interface
        // says no date rather than making one up.
        for entry in &mut out {
            if let Ok(commit) = self.inner.find_commit(entry.id) {
                entry.time = commit.time().seconds();
            }
        }

        Ok(out)
    }

    /// Sets aside everything half-done and leaves the tree as the last commit.
    ///
    /// Untracked files stay where they are, as `git stash` does: they are not
    /// in the way of fetching from the remote or switching branches, which is
    /// what the rest is set aside for.
    pub fn stash_save(&mut self, message: &str) -> Result<String> {
        let signature = self
            .inner
            .signature()
            .context("set user.name and user.email in Git first")?;

        let message = message.trim();
        let message = (!message.is_empty()).then_some(message);

        self.inner
            .stash_save2(&signature, message, Some(StashFlags::DEFAULT))
            .map_err(|err| match err.code() {
                // libgit2 calls having nothing to stash "not found".
                git2::ErrorCode::NotFound => anyhow::anyhow!("there is nothing to stash"),
                _ => anyhow::Error::from(err),
            })?;

        Ok("changes stashed".to_string())
    }

    /// Puts something set aside back in the working tree and off the pile.
    pub fn stash_pop(&mut self, index: usize) -> Result<String> {
        self.inner
            .stash_pop(index, None)
            .context("could not restore: your changes clash with the stashed ones")?;
        Ok("changes restored".to_string())
    }

    /// The same, but leaving it on the pile.
    pub fn stash_apply(&mut self, index: usize) -> Result<String> {
        self.inner
            .stash_apply(index, None)
            .context("could not apply: your changes clash with the stashed ones")?;
        Ok("changes applied, and still stashed".to_string())
    }

    /// Drops one. Whatever was in it cannot be got back.
    pub fn stash_drop(&mut self, index: usize) -> Result<String> {
        self.inner.stash_drop(index)?;
        Ok("stash dropped".to_string())
    }
}
