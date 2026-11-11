//! The repositories kept inside this one.
//!
//! A submodule is a folder whose contents are another repository, and what
//! this repository records about it is one commit: the one that folder is
//! meant to be at. Moving this repository -- a pull, a switch to another
//! branch -- moves that record and leaves the folder where it was, which is
//! how a tree comes to be built against last month's copy of a library.
//!
//! This is the part of `git submodule update --init --recursive` that puts
//! the folder back where the record says. It is asked for by the window,
//! after something has moved, when the preferences say to keep them up to
//! date; nothing in here decides that for itself.

use anyhow::{Context, Result};
use git2::{FetchOptions, Oid, RemoteCallbacks, Repository, SubmoduleUpdateOptions};

use crate::remote::credentials;
use crate::{Profile, RemoteRepo, Repo};

/// How deep a submodule's own submodules are followed. Far past anything
/// real, and short of for ever in a repository that contains itself.
const DEEPEST: usize = 8;

/// One submodule, and how it stands against what is recorded for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sub {
    /// Where it is in the working tree.
    pub path: String,
    /// The commit this repository says it should be at.
    pub recorded: Option<Oid>,
    /// The commit it is at, or nothing when it has never been brought down.
    pub checked_out: Option<Oid>,
}

impl Sub {
    /// Whether the folder is somewhere other than where the record says.
    pub fn behind(&self) -> bool {
        self.recorded.is_some() && self.recorded != self.checked_out
    }

    /// Whether there is a record and no folder to go with it: a clone made
    /// without its submodules, or a submodule that arrived in a pull.
    pub fn missing(&self) -> bool {
        self.recorded.is_some() && self.checked_out.is_none()
    }
}

impl Repo {
    /// The submodules of this repository. None is the usual answer.
    ///
    /// A repository whose submodules cannot be read has none as far as this
    /// is concerned: it is asked on every reload, and a broken `.gitmodules`
    /// is not a reason for the history above it not to draw.
    pub fn submodules(&self) -> Vec<Sub> {
        let Ok(found) = self.inner.submodules() else {
            return Vec::new();
        };
        found
            .iter()
            .map(|sub| Sub {
                path: sub.path().to_string_lossy().into_owned(),
                // What is staged and nothing else: somebody who moved a
                // submodule and staged the move has said where it belongs,
                // and somebody who staged its removal has said it belongs
                // nowhere -- which the last commit still disagrees with.
                recorded: sub.index_id(),
                checked_out: sub.workdir_id(),
            })
            .collect()
    }

    /// Brings the named submodules to the commit recorded for them, and
    /// their own submodules after them.
    ///
    /// One that has never been brought down is cloned first. One that has
    /// work in it that the move would write over is not moved: the checkout
    /// refuses, and that submodule is named in what comes back as an error
    /// once the others have been done.
    pub fn update_submodules(&self, paths: &[String]) -> Result<String> {
        let mut done = 0;
        let mut refused = Vec::new();
        bring(&self.inner, Some(paths), self.profile.as_ref(), DEEPEST, &mut done, &mut refused);

        anyhow::ensure!(
            refused.is_empty(),
            "could not update {}",
            refused.join("; ")
        );
        Ok(match done {
            0 => "the submodules were already up to date".to_string(),
            1 => "1 submodule updated".to_string(),
            n => format!("{n} submodules updated"),
        })
    }
}

/// Updates the submodules of one repository and goes down into each.
///
/// `only` limits the first level to the ones asked for. Below that every
/// submodule that is not where it should be is moved: nobody asked for
/// those by name, and a library left half-updated is worse than either end.
fn bring(
    repo: &Repository,
    only: Option<&[String]>,
    profile: Option<&Profile>,
    depth: usize,
    done: &mut usize,
    refused: &mut Vec<String>,
) {
    let Ok(found) = repo.submodules() else {
        return;
    };
    for mut sub in found {
        let path = sub.path().to_string_lossy().into_owned();
        if only.is_some_and(|wanted| !wanted.contains(&path)) {
            continue;
        }
        let recorded = sub.index_id();
        if recorded.is_some() && recorded != sub.workdir_id() {
            match update(repo, &mut sub, profile) {
                Ok(()) => *done += 1,
                Err(err) => {
                    refused.push(format!("{path}: {err:#}"));
                    continue;
                }
            }
        }
        if depth > 0 {
            if let Ok(inside) = sub.open() {
                bring(&inside, None, profile, depth - 1, done, refused);
            }
        }
    }
}

fn update(repo: &Repository, sub: &mut git2::Submodule<'_>, profile: Option<&Profile>) -> Result<()> {
    let mut callbacks = RemoteCallbacks::new();
    // The same ladder as any other fetch: a private library is as likely to
    // need the client's key as the repository it lives in.
    callbacks.credentials(credentials(repo.config()?, profile.cloned()));
    let mut fetch = FetchOptions::new();
    fetch.remote_callbacks(callbacks);

    let mut options = SubmoduleUpdateOptions::new();
    options.fetch(fetch);
    // `true` is "and set it up first if it never was".
    sub.update(true, Some(&mut options))
        .context("it would not move to the recorded commit")?;
    Ok(())
}

impl RemoteRepo {
    /// Whether the checkout over there has submodules at all, asked of the
    /// one file that says so.
    pub fn has_submodules(&self) -> bool {
        self.session
            .exists(&format!("{}/.gitmodules", self.path().trim_end_matches('/')))
    }

    /// The same as [`Repo::update_submodules`], done by the Git on the
    /// server and with the server's own credentials.
    pub fn update_submodules(&self) -> Result<String> {
        self.git(&["submodule", "update", "--init", "--recursive"])?;
        Ok("the submodules are up to date".to_string())
    }
}
