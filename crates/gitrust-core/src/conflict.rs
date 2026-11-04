//! Getting out of a merge that did not come out clean.
//!
//! Everything else in this crate is built on never getting into one. A merge,
//! a cherry-pick, a revert are all worked out in memory first and refused if
//! they would clash, so that a repository is never left half-done. That was
//! not caution for its own sake: a half-merged repository is a place you have
//! to know how to get out of, and until this module there was nothing here to
//! get anybody out of it.
//!
//! So this is the module that lets the rest stop refusing. Only merge, for
//! now -- see the note at the bottom about why a rebase is a different and
//! harder thing.
//!
//! # What a conflicted state is
//!
//! Three things at once, and all three have to be put right together:
//!
//! - **The index** holds up to three versions of each clashing file, at
//!   stages 1, 2 and 3: what it was before either side touched it, what it is
//!   on your side, what it is on theirs. A file is resolved when those three
//!   are replaced by one ordinary entry.
//! - **The working tree** holds the file with `<<<<<<<` markers in it, which
//!   is Git's way of putting both answers in front of you.
//! - **`MERGE_HEAD`** holds the commit being merged in, which is what makes
//!   the next commit a merge rather than an ordinary one.
//!
//! Taking a side writes all three. Abandoning throws all three away.
//!
//! # What is deliberately not here
//!
//! **No three-way merge editor.** You take one side whole, or you edit the
//! file with the markers in it and say you are done. A proper editor with
//! three panes and line-by-line picking is a real thing to build and this is
//! not it; what is here is what gets you out, which is what was missing.
//!
//! **No rebase.** A conflicted rebase is not one clash, it is a sequence of
//! them: resolve, carry on, clash again, and a notion of "continue" that
//! carries state between steps. Merge is one clash with one answer, and that
//! is the one worth having first. Rebase still refuses and still says so.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, IndexEntry, Oid, ResetType};

use crate::file::Contents;
use crate::repo::Repo;

/// Which side of a clash to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// What the branch you are on says.
    Ours,
    /// What the branch coming in says.
    Theirs,
}

impl Side {
    pub fn says(self) -> &'static str {
        match self {
            Side::Ours => "what this branch says",
            Side::Theirs => "what the other branch says",
        }
    }
}

/// What is half-done in this repository.
///
/// Only [`Half::Merge`] can be finished here. The others are listed because a
/// repository can be in them -- somebody used the command line -- and saying
/// "a rebase is half-done, and this cannot finish it" is a great deal more
/// use than drawing an empty screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    /// Nothing is half-done.
    None,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    /// Something else libgit2 knows about and this does not handle.
    Other,
}

impl Half {
    /// What it is called, for somebody reading a screen.
    pub fn says(self) -> &'static str {
        match self {
            Half::None => "nothing half-done",
            Half::Merge => "a merge",
            Half::Rebase => "a rebase",
            Half::CherryPick => "a commit being taken along",
            Half::Revert => "a commit being undone",
            Half::Other => "something",
        }
    }

    /// Whether this module can see it through.
    pub fn ours_to_finish(self) -> bool {
        self == Half::Merge
    }
}

/// One file both sides changed, in all the versions there are of it.
///
/// Each of the three is [`Contents::Absent`] when that side has no version:
/// a file added on both sides has no ancestor, and a file deleted on one side
/// and changed on the other is missing from one of the two. Those are real
/// clashes and the commonest ones to get wrong, so they are said rather than
/// left as empty text.
#[derive(Debug, Clone)]
pub struct Clash {
    pub path: String,
    /// Before either side touched it.
    pub base: Contents,
    /// On the branch you are on.
    pub ours: Contents,
    /// On the branch coming in.
    pub theirs: Contents,
}

impl Clash {
    /// What kind of clash this is, in the words somebody deciding would want.
    ///
    /// The three shapes worth telling apart, because what you would do about
    /// them is different: both changed it, one deleted it, both added it.
    pub fn shape(&self) -> &'static str {
        match (&self.base, &self.ours, &self.theirs) {
            (_, Contents::Absent, _) => "deleted here, changed there",
            (_, _, Contents::Absent) => "changed here, deleted there",
            (Contents::Absent, _, _) => "added on both sides, differently",
            _ => "both sides changed it",
        }
    }
}

impl Repo {
    /// What this repository is in the middle of.
    pub fn half_done(&self) -> Half {
        use git2::RepositoryState::*;
        match self.inner.state() {
            Clean => Half::None,
            Merge => Half::Merge,
            Rebase | RebaseInteractive | RebaseMerge => Half::Rebase,
            CherryPick | CherryPickSequence => Half::CherryPick,
            Revert | RevertSequence => Half::Revert,
            _ => Half::Other,
        }
    }

    /// Every file still waiting to be decided.
    ///
    /// Empty while nothing is clashing, which is the ordinary answer and not
    /// an error: a merge can be half-done with every file already resolved,
    /// waiting only to be committed.
    pub fn clashes(&self) -> Result<Vec<Clash>> {
        let index = self.inner.index()?;
        if !index.has_conflicts() {
            return Ok(Vec::new());
        }

        let mut clashes = Vec::new();
        for clash in index.conflicts()? {
            let clash = clash?;
            // The path comes from whichever side has one. All three carry it
            // and at least one is always there, but which one varies: that is
            // the whole of what "deleted on one side" means here.
            let Some(path) = [&clash.ancestor, &clash.our, &clash.their]
                .into_iter()
                .flatten()
                .find_map(|entry| String::from_utf8(entry.path.clone()).ok())
            else {
                continue;
            };

            clashes.push(Clash {
                path,
                base: self.version(clash.ancestor.as_ref())?,
                ours: self.version(clash.our.as_ref())?,
                theirs: self.version(clash.their.as_ref())?,
            });
        }

        clashes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(clashes)
    }

    /// One version of a clashing file, read out of the index.
    fn version(&self, entry: Option<&IndexEntry>) -> Result<Contents> {
        let Some(entry) = entry else {
            return Ok(Contents::Absent);
        };
        let blob = self.inner.find_blob(entry.id)?;
        if blob.is_binary() {
            return Ok(Contents::Binary(blob.size()));
        }
        Ok(Contents::Text(String::from_utf8_lossy(blob.content()).to_string()))
    }

    /// The file as it sits on disk, markers and all.
    ///
    /// What somebody editing by hand is editing. It is read from the working
    /// tree rather than put together from the three stages, because that is
    /// the file they have been looking at and possibly already changed.
    pub fn clashing_text(&self, path: &str) -> Result<String> {
        let whole = self.inner.workdir().context("there is no working tree here")?;
        std::fs::read_to_string(whole.join(path))
            .with_context(|| format!("could not read {path}"))
    }

    /// Settles a file by taking one side of it whole.
    pub fn take_side(&self, path: &str, side: Side) -> Result<String> {
        let index = self.inner.index()?;
        let clash = index
            .conflict_get(std::path::Path::new(path))
            .with_context(|| format!("{path} is not one of the files clashing"))?;

        let wanted = match side {
            Side::Ours => clash.our,
            Side::Theirs => clash.their,
        };

        match wanted {
            // That side has no version: taking it means the file goes.
            None => {
                let whole = self.inner.workdir().context("there is no working tree here")?;
                let _ = std::fs::remove_file(whole.join(path));
                self.settle(path, None)?;
                Ok(format!("{path} is gone, which is {}", side.says()))
            }
            Some(entry) => {
                let blob = self.inner.find_blob(entry.id)?;
                self.settle(path, Some(blob.content()))?;
                Ok(format!("{path} is now {}", side.says()))
            }
        }
    }

    /// Settles a file with text somebody wrote themselves.
    ///
    /// Refused while the markers are still in it. Leaving `<<<<<<<` in a file
    /// and calling it resolved is the single commonest way a merge goes wrong,
    /// and it is wrong in a way nothing afterwards complains about: it
    /// commits, it pushes, and it breaks when somebody runs it.
    pub fn take_text(&self, path: &str, text: &str) -> Result<String> {
        anyhow::ensure!(
            !text.lines().any(is_marker),
            "{path} still has conflict markers in it"
        );
        self.settle(path, Some(text.as_bytes()))?;
        Ok(format!("{path} is settled"))
    }

    /// Writes a file's decided contents and takes it out of the clash.
    ///
    /// The three staged versions go and one ordinary entry takes their place,
    /// which is the whole of what "resolved" means to Git.
    fn settle(&self, path: &str, contents: Option<&[u8]>) -> Result<()> {
        if let Some(contents) = contents {
            let whole = self.inner.workdir().context("there is no working tree here")?;
            let at = whole.join(path);
            if let Some(folder) = at.parent() {
                std::fs::create_dir_all(folder)?;
            }
            std::fs::write(&at, contents)
                .with_context(|| format!("could not write {path}"))?;
        }

        let mut index = self.inner.index()?;
        let path = std::path::Path::new(path);
        index.conflict_remove(path)?;
        match contents {
            Some(_) => index.add_path(path)?,
            None => {
                // Already removed by conflict_remove; remove_path would fail
                // on something that is not there, and has nothing to do.
            }
        }
        index.write()?;
        Ok(())
    }

    /// Writes the merge commit, once nothing is clashing any more.
    pub fn finish_merge(&self) -> Result<String> {
        anyhow::ensure!(
            self.half_done() == Half::Merge,
            "there is no half-done merge here"
        );

        let mut index = self.inner.index()?;
        let left: Vec<String> = self
            .clashes()?
            .into_iter()
            .map(|clash| clash.path)
            .collect();
        anyhow::ensure!(
            left.is_empty(),
            "still to decide: {}",
            named(&left)
        );

        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let current = head.shorthand().unwrap_or("HEAD").to_string();
        let ours = head.peel_to_commit()?;

        let incoming = self
            .inner
            .revparse_single("MERGE_HEAD")
            .context("the repository says a merge is half-done but not what with")?
            .id();
        let theirs = self.inner.find_commit(incoming)?;

        let tree = self.inner.find_tree(index.write_tree()?)?;
        let signature = self
            .inner
            .signature()
            .context("set user.name and user.email in Git first")?;

        // Git leaves the message it had in mind in MERGE_MSG, along with
        // whatever it wrote about the conflicts. Using it keeps the wording
        // the same whether or not there was anything to decide.
        let message = self
            .merge_message()
            .unwrap_or_else(|| format!("Merge into {current}"));

        self.inner.commit(
            Some(&refname),
            &signature,
            &signature,
            &message,
            &tree,
            &[&ours, &theirs],
        )?;

        self.inner
            .cleanup_state()
            .context("the merge was written but the half-done state is still here")?;
        self.checkout_head_forced()?;

        Ok(format!("the merge is written on {current}"))
    }

    /// Throws the whole half-done thing away.
    ///
    /// Everything goes: the markers, the staged versions, anything typed into
    /// those files since. That is what it is for, and it is the only way out
    /// that always works, so it has to be offered and it has to be asked
    /// about first.
    pub fn abandon(&self) -> Result<String> {
        let half = self.half_done();
        anyhow::ensure!(half != Half::None, "there is nothing half-done here");

        let head = self
            .inner
            .head()
            .context("the repository has no commits")?
            .peel_to_commit()?;

        self.inner
            .reset(head.as_object(), ResetType::Hard, None)
            .context("could not put the files back")?;
        self.inner.cleanup_state()?;

        Ok(format!("{} is abandoned", half.says()))
    }

    /// What Git meant the merge commit to say.
    fn merge_message(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.inner.path().join("MERGE_MSG")).ok()?;
        // Git writes the conflicting paths into it as comments while the
        // merge is half-done. They are notes to the person, not to the log.
        let message: String = text
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<&str>>()
            .join("\n");
        let message = message.trim().to_string();
        (!message.is_empty()).then_some(message)
    }

    /// Brings the working tree up to whatever HEAD now says.
    ///
    /// Forced, and safe here: it runs straight after writing a commit whose
    /// tree is the index that was just settled, so there is nothing of
    /// anybody's it can walk over.
    fn checkout_head_forced(&self) -> Result<()> {
        self.inner
            .checkout_head(Some(CheckoutBuilder::new().force()))
            .context("the merge was written but the working tree could not be updated")?;
        Ok(())
    }

    /// The commit being merged in, while one is.
    pub fn merging_in(&self) -> Option<Oid> {
        self.inner.revparse_single("MERGE_HEAD").ok().map(|object| object.id())
    }
}

/// Whether a line is one of Git's conflict markers.
///
/// The start and end markers carry the branch name after them, so they are
/// matched by their beginning. The middle one is seven equals signs and
/// nothing else, and matching that one loosely would refuse any file with a
/// row of equals signs in it -- which is most Markdown ever written.
fn is_marker(line: &str) -> bool {
    line.starts_with("<<<<<<< ")
        || line.starts_with(">>>>>>> ")
        || line == "======="
        || line.starts_with("||||||| ")
}

/// A list of paths the way you would say them aloud: three, and the rest.
fn named(paths: &[String]) -> String {
    match paths.len() {
        0 => String::new(),
        1..=3 => paths.join(", "),
        n => format!("{} and {} more", paths[..3].join(", "), n - 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_markers_are_recognised_and_a_row_of_equals_signs_is_not() {
        assert!(is_marker("<<<<<<< HEAD"));
        assert!(is_marker(">>>>>>> feat/something"));
        assert!(is_marker("======="));
        assert!(is_marker("||||||| merged common ancestors"));

        // A heading underline in Markdown, which is not a marker and must
        // not stop somebody settling a file.
        assert!(!is_marker("========"));
        assert!(!is_marker("=========="));
        assert!(!is_marker("a = b"));
        assert!(!is_marker("<<<<<<<"));
    }

    #[test]
    fn a_list_of_paths_is_said_the_way_somebody_would_say_it() {
        let paths: Vec<String> = ["a", "b", "c", "d", "e"].iter().map(|s| s.to_string()).collect();
        assert_eq!(named(&paths[..1]), "a");
        assert_eq!(named(&paths[..3]), "a, b, c");
        assert_eq!(named(&paths), "a, b, c and 2 more");
    }
}
