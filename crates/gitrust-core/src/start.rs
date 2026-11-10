//! Making a repository, or bringing one down.
//!
//! The two ways a repository arrives on a machine. Everything else in this
//! crate works on one that is already there, which is why these are free
//! functions rather than methods: there is nothing to be a method on yet.
//!
//! Both of them refuse to write into a folder that already has something in
//! it. Git itself is more forgiving -- `git init` in a folder with files is
//! ordinary, and `git clone` into an empty-but-existing folder works -- but
//! the window asking for this has one box with a path in it, and the most
//! likely thing somebody types into it by mistake is a folder full of
//! something else.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{build::RepoBuilder, FetchOptions, RemoteCallbacks, Repository};

use crate::remote::credentials;
use crate::Profile;

/// Makes an empty repository at a path.
///
/// The folder is made if it is not there. If it is there and has anything in
/// it, nothing happens: see the note above about what gets typed by mistake.
pub fn init(path: &Path) -> Result<String> {
    guard(path)?;
    std::fs::create_dir_all(path)
        .with_context(|| format!("could not make {}", path.display()))?;

    Repository::init(path).with_context(|| format!("could not start a repository at {}", path.display()))?;
    Ok(format!("{} is a repository now", name_of(path)))
}

/// Brings a repository down into a folder of its own.
///
/// The folder is worked out from the address when one is not given, the way
/// `git clone` does it: the last part of the URL with any `.git` taken off.
///
/// It is the slowest thing in this crate by a wide margin, and the caller is
/// expected to be running it somewhere that is not painting the window.
///
/// The profile is handed in rather than looked up, because this is the one
/// thing here that happens before there is a repository to hang one on -- and
/// the private repository of a client is exactly the kind that cannot be
/// cloned without their key.
pub fn clone(url: &str, into: &Path, profile: Option<Profile>) -> Result<String> {
    let url = url.trim();
    anyhow::ensure!(!url.is_empty(), "there is no address to clone from");
    guard(into)?;

    let mut callbacks = RemoteCallbacks::new();
    // The same ladder as fetching and pushing: the profile's key and token
    // first, then the agent and whatever the credential helper has. A clone
    // over https from a public host asks for none of it and never calls this.
    let config = git2::Config::open_default().context("could not read your Git configuration")?;
    callbacks.credentials(credentials(config, profile));

    let mut fetch = FetchOptions::new();
    fetch.remote_callbacks(callbacks);

    RepoBuilder::new()
        .fetch_options(fetch)
        .clone(url, into)
        .with_context(|| format!("could not clone {url}"))?;

    Ok(format!("{} is here now", name_of(into)))
}

/// The folder a clone of this address would go into, beside `beside`.
///
/// `https://github.com/someone/picstar.git` and `git@github.com:someone/picstar.git`
/// both give `picstar`, which is what `git clone` would have called it.
pub fn folder_for(url: &str, beside: &Path) -> Option<PathBuf> {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        return None;
    }

    // Whatever comes after the last slash, and after the last colon for the
    // `git@host:owner/name` form that has no slash before the owner.
    let leaf = url.rsplit(['/', ':']).next()?;
    let leaf = leaf.strip_suffix(".git").unwrap_or(leaf);
    if leaf.is_empty() {
        return None;
    }
    Some(beside.join(leaf))
}

/// Refuses a folder that already has anything in it.
///
/// A folder that is not there at all is fine, and so is one that is there and
/// empty: those are both somebody saying where they want it.
fn guard(path: &Path) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "there is no folder to put it in"
    );

    let Ok(mut inside) = std::fs::read_dir(path) else {
        // Not there, or not readable. Not there is the ordinary case and the
        // next step makes it; not readable will say so itself, in words about
        // the real problem rather than about emptiness.
        return Ok(());
    };
    anyhow::ensure!(
        inside.next().is_none(),
        "{} already has something in it",
        path.display()
    );
    Ok(())
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_lands_in_the_folder_git_would_have_called_it() {
        let beside = Path::new("/home/someone/Projects");
        for url in [
            "https://github.com/someone/picstar.git",
            "https://github.com/someone/picstar",
            "git@github.com:someone/picstar.git",
            "/srv/mirrors/picstar.git",
            "https://github.com/someone/picstar/",
        ] {
            assert_eq!(
                folder_for(url, beside),
                Some(PathBuf::from("/home/someone/Projects/picstar")),
                "{url}"
            );
        }
    }

    #[test]
    fn an_address_that_names_nothing_gives_no_folder() {
        let beside = Path::new("/tmp");
        for url in ["", "   ", "/", ".git"] {
            assert_eq!(folder_for(url, beside), None, "{url:?}");
        }
    }
}
