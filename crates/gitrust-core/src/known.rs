//! The repositories this machine knows about.
//!
//! The first thing gitrust keeps outside a repository, and that deserves
//! saying out loud. Everything else it remembers -- the backlog, the notes on
//! branches -- lives in `refs/gitrust/backlog`, inside the repository it is
//! about, so it travels with a clone and merges when two people write at
//! once. A list of folders on this computer cannot live there. It is about
//! this computer, and it means nothing on anybody else's.
//!
//! # Where
//!
//! `$XDG_CONFIG_HOME/gitrust/known`, or `$HOME/.config/gitrust/known`. No
//! crate is used to find it: the rule is two lines of `std::env`, and a
//! dependency for that would be a sixth in a project that has five on
//! purpose.
//!
//! # What
//!
//! One repository per line, oldest first:
//!
//! ```text
//! 1762300000 * /home/someone/Projects/picstar
//! 1762299000   /home/someone/Projects/gitrust
//! ```
//!
//! When it was last opened, whether it is a favourite, and the path. A line
//! it cannot read is dropped rather than refused: this file is a convenience,
//! and a convenience that stops the application starting is not one.
//!
//! # What it does not do
//!
//! It does not go looking. Nothing here walks the disk hunting for `.git`
//! folders -- a list that fills itself with everything you have ever checked
//! out is a list nobody can find anything in, and a program that reads your
//! home folder uninvited is a program doing something you did not ask for.
//! A repository is in here because it was opened.
//!
//! It does not tidy up either. A folder that has been deleted stays on the
//! list, marked as missing when it is read, because a disk that is not
//! plugged in right now is not the same thing as a repository that is gone.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// One repository the machine has seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub path: PathBuf,
    /// When it was last opened, in seconds since the epoch. It is what the
    /// list is sorted by, newest first.
    pub seen: i64,
    /// Kept at the top whatever its age.
    pub starred: bool,
}

impl Known {
    /// What to call it: the folder's own name.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// The folder it sits in, which is what tells two `web` folders apart.
    pub fn parent(&self) -> String {
        self.path
            .parent()
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    }

    /// Whether the folder is still there with a repository in it.
    ///
    /// Asked of the disk every time rather than remembered, because the
    /// answer changes without anybody telling us: a disk is unplugged, a
    /// folder is renamed.
    pub fn there(&self) -> bool {
        self.path.join(".git").exists() || self.path.join("HEAD").exists()
    }

    fn render(&self) -> String {
        format!(
            "{} {} {}\n",
            self.seen,
            match self.starred {
                true => '*',
                false => '-',
            },
            self.path.display()
        )
    }

    /// Reads one back, or nothing at all.
    ///
    /// Nothing at all is the ordinary answer for a line written by a later
    /// gitrust, or half-written by a machine that lost power. The caller
    /// drops it and carries on.
    fn parse(line: &str) -> Option<Self> {
        let (seen, rest) = line.split_once(' ')?;
        let (starred, path) = rest.split_once(' ')?;
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        Some(Known {
            seen: seen.parse().ok()?,
            starred: starred == "*",
            path: PathBuf::from(path),
        })
    }
}

/// Where the file lives, whether or not it is there yet.
pub fn file() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("gitrust").join("known"))
}

/// Every repository the machine knows, favourites first and newest after.
///
/// An unreadable file is an empty list, not an error. This is a convenience,
/// and a convenience that stops the window opening is not one.
pub fn all() -> Vec<Known> {
    let Some(file) = file() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Vec::new();
    };

    let mut known: Vec<Known> = text.lines().filter_map(Known::parse).collect();
    known.sort_by(|a, b| {
        b.starred
            .cmp(&a.starred)
            .then(b.seen.cmp(&a.seen))
            .then(a.path.cmp(&b.path))
    });
    known
}

/// Writes the list back, whole.
///
/// Through a file beside it and a rename, so that a machine that goes down
/// mid-write is left with the old list rather than half of the new one. A
/// rename within one folder is the one filesystem operation that either
/// happened or did not.
pub fn save(known: &[Known]) -> Result<()> {
    let file = file().context("there is no home folder to keep the list in")?;
    let folder = file.parent().context("the list has nowhere to live")?;
    std::fs::create_dir_all(folder)
        .with_context(|| format!("could not make {}", folder.display()))?;

    let text: String = known.iter().map(Known::render).collect();
    let beside = file.with_extension("writing");
    std::fs::write(&beside, text).with_context(|| format!("could not write {}", beside.display()))?;
    std::fs::rename(&beside, &file).with_context(|| format!("could not put {} in place", file.display()))?;
    Ok(())
}

/// Says a repository was just opened, and writes that down.
///
/// The path is made absolute first. A repository opened as `.` from two
/// different folders is two different lines otherwise, and both of them are
/// useless the moment you are somewhere else.
pub fn seen(path: &Path, when: i64) -> Result<()> {
    let path = std::fs::canonicalize(path)
        .with_context(|| format!("there is nothing at {}", path.display()))?;

    let mut known = all();
    match known.iter_mut().find(|known| known.path == path) {
        Some(known) => known.seen = when,
        None => known.push(Known { path, seen: when, starred: false }),
    }
    save(&known)
}

/// Turns the star on a repository on or off.
pub fn star(path: &Path, starred: bool) -> Result<()> {
    let mut known = all();
    match known.iter_mut().find(|known| known.path == path) {
        Some(known) => known.starred = starred,
        // Starring something the list has never heard of would quietly do
        // nothing, which is worse than saying so.
        None => anyhow::bail!("{} is not on the list", path.display()),
    }
    save(&known)
}

/// Takes a repository off the list.
///
/// The folder is not touched. This is the list forgetting, not a delete, and
/// anybody who wants the files gone has a file manager for that.
pub fn forget(path: &Path) -> Result<()> {
    let known: Vec<Known> = all().into_iter().filter(|known| known.path != path).collect();
    save(&known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_survives_the_trip_to_the_file_and_back() {
        let known = Known {
            path: PathBuf::from("/home/someone/Projects/picstar"),
            seen: 1_762_300_000,
            starred: true,
        };
        assert_eq!(Known::parse(known.render().trim_end()), Some(known));
    }

    #[test]
    fn a_path_with_spaces_in_it_comes_back_whole() {
        let known = Known {
            path: PathBuf::from("/home/someone/My Projects/the site"),
            seen: 7,
            starred: false,
        };
        let read = Known::parse(known.render().trim_end()).expect("it reads back");
        assert_eq!(read.path, known.path);
    }

    /// Half a line is what a machine losing power mid-write leaves behind,
    /// and the list has to open anyway.
    #[test]
    fn a_line_that_makes_no_sense_is_dropped_rather_than_refused() {
        for line in ["", "   ", "nonsense", "1762300000", "1762300000 *", "notanumber * /tmp/x"] {
            assert_eq!(Known::parse(line), None, "{line:?}");
        }
    }

    #[test]
    fn a_repository_is_named_after_its_folder() {
        let known = Known {
            path: PathBuf::from("/home/someone/Projects/picstar"),
            seen: 0,
            starred: false,
        };
        assert_eq!(known.name(), "picstar");
        assert_eq!(known.parent(), "/home/someone/Projects");
    }
}
