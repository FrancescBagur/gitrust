//! What somebody wrote about a branch.
//!
//! A branch name is the only written intention a repository holds, and it is
//! a poor one: `fix-the-footer-rides-over-the-content` is what is left of a
//! sentence after it has been filed down to fit a reference name. Worse, it
//! was the backlog that filed it: you wrote the sentence, starting the item
//! turned it into that, and the sentence was gone.
//!
//! A note gives the branch the sentence back, and room for a paragraph under
//! it saying what there is to do. Nothing else here is stored -- the columns,
//! the parentage, the dates are all read off the graph -- but this cannot be
//! read off anything. Nobody's intention is in a commit.
//!
//! # Where it is kept
//!
//! In the same reference as the backlog, [`crate::backlog::REF`], under
//! `notes/` beside its `items/`. A reference of its own would have been
//! tidier to look at and worse to use: starting a backlog item has to take
//! the item out *and* write the note, and across two references that is two
//! commits with a half-done state between them. In one tree it is one commit,
//! which is the rule everywhere else here -- work it out first, write once.
//!
//! One file per branch, named after the branch with its slashes turned into
//! `~`. Git forbids `~` in a reference name, so the two cannot be confused
//! and the name reads as itself in `git log`. Two people writing notes on two
//! branches write two files and the trees merge without anybody noticing; two
//! people writing the same branch's note clash, which is a real disagreement
//! and worth stopping for.
//!
//! # What it does not do
//!
//! A note is not removed when its branch goes. A branch can come back, a
//! deletion can be a mistake, and throwing away the only sentence anybody
//! wrote is worse than leaving a file nobody reads. So notes for branches
//! that no longer exist sit there, counted by nothing and shown by nothing.
//! Over years that is clutter, and there is no tidying for it yet.

use std::collections::HashMap;

use anyhow::Result;
use git2::Commit;

use crate::repo::Repo;

/// What was written about one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub branch: String,
    /// The title in the words it was written in, which a branch name cannot
    /// hold.
    pub title: String,
    /// What there is to do, in as many lines as it takes. Empty is ordinary:
    /// most work needs a sentence, not a brief.
    pub body: String,
    pub added: i64,
    pub by: String,
}

impl Note {
    /// What this note is called in the tree.
    pub(crate) fn file(&self) -> String {
        format!("{}.txt", file_name(&self.branch))
    }

    pub(crate) fn render(&self) -> String {
        // The body goes after a blank line rather than on a key of its own,
        // because it is the one field that is allowed to be several lines and
        // a key cannot be.
        let mut text = format!("branch: {}\n", self.branch);
        text.push_str(&format!("title: {}\n", self.title));
        text.push_str(&format!("added: {}\n", self.added));
        text.push_str(&format!("by: {}\n", self.by));
        if !self.body.is_empty() {
            text.push('\n');
            text.push_str(&self.body);
            if !self.body.ends_with('\n') {
                text.push('\n');
            }
        }
        text
    }
}

/// A branch name turned into a file name, and back.
///
/// Git will not have `~` in a reference name, so turning `/` into one is
/// reversible and cannot collide: there is exactly one branch name that gives
/// any file name, and exactly one file name for any branch.
fn file_name(branch: &str) -> String {
    branch.replace('/', "~")
}

fn branch_name(file: &str) -> String {
    file.replace('~', "/")
}

/// Reads a note back from its file.
///
/// The branch is read from inside rather than taken from the file name: the
/// file says what it is about, and a tree entry that has been renamed by hand
/// is then wrong about its name rather than wrong about its contents.
///
/// Keys it does not know are passed over, so a newer gitrust writing a field
/// this one never heard of does not make somebody's notes unreadable.
fn parse(text: &str) -> Option<Note> {
    let mut branch = None;
    let mut title = None;
    let mut added = 0;
    let mut by = String::new();

    let mut lines = text.lines();
    for line in lines.by_ref() {
        // The first blank line ends the keys and starts the body.
        if line.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        match key {
            "branch" => branch = Some(value.to_string()),
            "title" => title = Some(value.to_string()),
            "added" => added = value.parse().unwrap_or(0),
            "by" => by = value.to_string(),
            _ => {}
        }
    }

    let body = lines.collect::<Vec<&str>>().join("\n");
    Some(Note {
        branch: branch?,
        title: title?,
        body: body.trim_end().to_string(),
        added,
        by,
    })
}

impl Repo {
    /// Every note there is, by branch.
    ///
    /// Notes for branches that are gone come back too. It is the caller that
    /// knows which branches exist, and a note nobody asks for costs nothing.
    pub fn notes(&self) -> Result<HashMap<String, Note>> {
        let Some(commit) = self.board_tip() else {
            return Ok(HashMap::new());
        };
        Ok(self
            .notes_of(&commit)?
            .into_iter()
            .map(|note| (note.branch.clone(), note))
            .collect())
    }

    /// What was written about one branch, if anything was.
    pub fn note(&self, branch: &str) -> Result<Option<Note>> {
        Ok(self.notes()?.remove(branch))
    }

    /// Writes the note for a branch, replacing whatever was there.
    ///
    /// An empty title takes the note away rather than leaving an empty one:
    /// clearing the box is the only way anybody would try to say "there is
    /// nothing written here after all".
    pub fn note_set(&self, branch: &str, title: &str, body: &str) -> Result<String> {
        let title = title.trim();
        if title.is_empty() {
            return self.note_clear(branch);
        }

        let signature = self.who()?;
        let note = Note {
            branch: branch.to_string(),
            title: title.to_string(),
            body: body.trim().to_string(),
            added: signature.when().seconds(),
            by: format!(
                "{} <{}>",
                signature.name().unwrap_or(""),
                signature.email().unwrap_or("")
            ),
        };

        let kept = self.notes_without(branch)?;
        let notes = [kept, vec![note]].concat();
        let items = self.backlog()?.items;
        self.write_board(&items, &notes, &signature, &format!("note: {branch}"))?;
        Ok(format!("{branch} says what it is for now"))
    }

    /// Takes the note away.
    ///
    /// Nothing is really lost: the reference keeps its own history, so
    /// `git log refs/gitrust/backlog` still has what was written in it.
    pub fn note_clear(&self, branch: &str) -> Result<String> {
        let notes = self.notes_without(branch)?;
        let signature = self.who()?;
        let items = self.backlog()?.items;
        self.write_board(&items, &notes, &signature, &format!("unnote: {branch}"))?;
        Ok(format!("{branch} has no note now"))
    }

    /// The notes as they stand, for a write that is not about them.
    ///
    /// Every write rewrites the whole tree, so anything that changes the
    /// backlog has to hand the notes back unchanged or they would be dropped
    /// by the same commit.
    pub(crate) fn notes_kept(&self) -> Result<Vec<Note>> {
        let Some(commit) = self.board_tip() else {
            return Ok(Vec::new());
        };
        self.notes_of(&commit)
    }

    pub(crate) fn notes_without(&self, branch: &str) -> Result<Vec<Note>> {
        Ok(self
            .notes_kept()?
            .into_iter()
            .filter(|note| note.branch != branch)
            .collect())
    }

    pub(crate) fn notes_of(&self, commit: &Commit<'_>) -> Result<Vec<Note>> {
        let tree = commit.tree()?;
        let Some(folder) = tree.get_name("notes") else {
            return Ok(Vec::new());
        };
        let folder = folder.to_object(&self.inner)?;
        let Some(folder) = folder.as_tree() else {
            return Ok(Vec::new());
        };

        let mut notes = Vec::new();
        for entry in folder.iter() {
            let Some(name) = entry.name().and_then(|name| name.strip_suffix(".txt")) else {
                continue;
            };
            let Ok(object) = entry.to_object(&self.inner) else {
                continue;
            };
            let Some(blob) = object.as_blob() else {
                continue;
            };
            let text = String::from_utf8_lossy(blob.content()).to_string();
            if let Some(mut note) = parse(&text) {
                // A file with no branch line of its own is still readable:
                // its name says which branch it is about.
                if note.branch.is_empty() {
                    note.branch = branch_name(name);
                }
                notes.push(note);
            }
        }

        notes.sort_by(|a, b| a.branch.cmp(&b.branch));
        Ok(notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_name_survives_the_trip_through_a_file_name() {
        for branch in ["main", "fix/footer", "feat/a/b/c", "report-export"] {
            assert_eq!(branch_name(&file_name(branch)), branch, "{branch}");
        }
    }

    #[test]
    fn a_note_read_back_is_the_note_that_was_written() {
        let note = Note {
            branch: "fix/footer".to_string(),
            title: "the footer rides over the content on a narrow phone".to_string(),
            body: "Only below 420px.\n\nThe sticky bar is the one at fault.".to_string(),
            added: 1_700_000_000,
            by: "Francesc <f@example.com>".to_string(),
        };
        assert_eq!(parse(&note.render()), Some(note));
    }

    #[test]
    fn a_note_with_nothing_under_its_title_is_an_ordinary_note() {
        let note = Note {
            branch: "main".to_string(),
            title: "keep the lights on".to_string(),
            body: String::new(),
            added: 7,
            by: String::new(),
        };
        let read = parse(&note.render()).expect("it reads back");
        assert_eq!(read.body, "");
        assert_eq!(read.title, "keep the lights on");
    }

    #[test]
    fn a_field_from_a_newer_gitrust_is_passed_over_rather_than_refused() {
        let text = "branch: main\ntitle: a title\nmood: cheerful\nadded: 3\nby: me\n\nbody\n";
        let note = parse(text).expect("it still reads");
        assert_eq!(note.title, "a title");
        assert_eq!(note.body, "body");
    }

    #[test]
    fn a_file_with_no_title_is_not_a_note() {
        assert_eq!(parse("branch: main\nadded: 3\n"), None);
    }
}
