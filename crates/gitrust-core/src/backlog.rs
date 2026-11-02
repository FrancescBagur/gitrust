//! The backlog: the work that has not been started.
//!
//! Everything else on the board is worked out from the graph, which is why it
//! cannot fall out of date. This cannot be: a thing nobody has begun leaves no
//! trace in a repository, so if it is to be on the board at all it has to be
//! written down somewhere.
//!
//! What keeps that from rotting is a rule rather than a mechanism: the backlog
//! is a waiting room, not a list of tasks. An item lives here only until it
//! becomes a branch, and the main thing you do with one is start it. From that
//! moment it is a card like any other, read off the graph. So what is stored
//! stays small, and stays recent.
//!
//! # Where it is kept
//!
//! In [`REF`], a reference outside the branch namespace. It is the same trick
//! `git notes` plays with `refs/notes/*`: a commit of its own, hanging from no
//! branch. This matters because the history here walks `refs/heads/*`,
//! `refs/remotes/*` and `refs/tags/*` and nothing else, so the backlog is
//! invisible to the graph, to the labels on the commits and to the sidebar
//! without any of them having to know it exists. It is not a secret, mind:
//! `git log --all` walks every reference there is and will show these commits
//! like any other. It is out of the way, which is all it needs to be.
//!
//! One file per item, not one file with every item. That is not tidiness: it
//! is the answer to the conflict. Two people writing down two different things
//! are two different files, and Git's own merge of trees settles it without
//! anybody noticing. Only two people touching the same item clash, and that is
//! a real disagreement and worth stopping for.
//!
//! # How it is shared
//!
//! By being pushed and fetched by name, which is why it reaches everyone who
//! has the repository rather than only this machine. Writing is local and
//! instant: noting down an idea should not be a network call. What has not
//! gone out yet is counted, so the column can say so.

use anyhow::{Context, Result};
use git2::{Commit, Oid, Signature};

use crate::note::Note;
use crate::repo::Repo;

/// Where what is written down about work lives: the backlog, and the notes
/// on branches that have already started.
///
/// The name is narrower than what it holds, and it is kept anyway. It is
/// already pushed to people's remotes, and a rename would be a migration for
/// everybody to gain a better word. See [`crate::note`] for what shares it
/// and why it shares it rather than taking a reference of its own.
pub const REF: &str = "refs/gitrust/backlog";

/// Where the remote's copy is put when it comes down, so the two can be
/// compared and joined instead of one quietly replacing the other.
fn remote_ref(remote: &str) -> String {
    format!("refs/gitrust/remotes/{remote}/backlog")
}

/// The longest a branch name made from a title may be.
const LEAF: usize = 40;

/// Something to do that has no branch yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The first twelve characters of its own blob's hash, which is also what
    /// it is called in the tree. Two people writing the identical thing write
    /// the identical file, and it merges into one.
    pub id: String,
    pub title: String,
    /// The folder its branch will go in, the same one the cards use.
    pub project: Option<String>,
    pub added: i64,
    pub by: String,
}

impl Item {
    /// The branch this would become, which is shown on the card so that the
    /// name is never a surprise.
    pub fn branch(&self) -> String {
        let leaf = slug(&self.title);
        match &self.project {
            Some(project) => format!("{project}/{leaf}"),
            None => leaf,
        }
    }

    fn render(&self) -> String {
        let mut text = format!("title: {}\n", self.title);
        if let Some(project) = &self.project {
            text.push_str(&format!("project: {project}\n"));
        }
        text.push_str(&format!("added: {}\n", self.added));
        text.push_str(&format!("by: {}\n", self.by));
        text
    }
}

/// The whole backlog, and how much of it the remote has not seen.
#[derive(Debug, Clone, Default)]
pub struct Backlog {
    /// Newest first, the way the columns are read.
    pub items: Vec<Item>,
    /// Items written here that have not gone out yet. Zero with no remote to
    /// send them to, because there is nothing to say then.
    pub unshared: usize,
}

/// A title turned into something a branch can be called.
///
/// Anything that is not a plain letter or digit becomes a hyphen, accented
/// letters included, and a run of them becomes one. A branch name is read
/// aloud, not decoded, so losing the accents costs nothing and keeps the name
/// typeable from any keyboard.
fn slug(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }

    let out = out.trim_end_matches('-');
    match out.len() > LEAF {
        true => out[..LEAF].trim_end_matches('-').to_string(),
        false => out.to_string(),
    }
}

/// Reads one item back from its file.
///
/// Keys it does not know are passed over rather than refused: a newer version
/// of gitrust writing a field this one has never heard of should not make
/// somebody's backlog unreadable.
fn parse(id: &str, text: &str) -> Option<Item> {
    let mut title = None;
    let mut project = None;
    let mut added = 0;
    let mut by = String::new();

    for line in text.lines() {
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        match key {
            "title" => title = Some(value.to_string()),
            "project" => project = Some(value.to_string()),
            "added" => added = value.parse().unwrap_or(0),
            "by" => by = value.to_string(),
            _ => {}
        }
    }

    Some(Item {
        id: id.to_string(),
        title: title?,
        project,
        added,
        by,
    })
}

/// The first twelve characters of a hash, which is plenty to tell apart as
/// many items as anybody is going to write down.
fn short(oid: Oid) -> String {
    oid.to_string()[..12].to_string()
}

impl Repo {
    /// Everything waiting to be started.
    ///
    /// An empty backlog and a repository that has never had one are the same
    /// thing here, because there is nothing to tell the two apart and nothing
    /// anyone would do differently.
    pub fn backlog(&self) -> Result<Backlog> {
        let items = match self.board_tip() {
            Some(commit) => self.items_of(&commit)?,
            None => Vec::new(),
        };

        Ok(Backlog {
            unshared: self.unshared(),
            items,
        })
    }

    /// Writes an item down. It goes out with the next share, not now.
    pub fn backlog_add(&self, title: &str, project: Option<&str>) -> Result<String> {
        let title = title.trim();
        anyhow::ensure!(!title.is_empty(), "the item needs a title");

        let signature = self.who()?;
        let item = Item {
            id: String::new(),
            title: title.to_string(),
            project: project.map(str::to_string),
            added: signature.when().seconds(),
            by: format!(
                "{} <{}>",
                signature.name().unwrap_or(""),
                signature.email().unwrap_or("")
            ),
        };
        anyhow::ensure!(
            !slug(title).is_empty(),
            "'{title}' leaves nothing a branch could be called"
        );

        let mut items = self.backlog()?.items;
        items.push(item);
        self.write_board(&items, &self.notes_kept()?, &signature, &format!("add: {title}"))?;
        Ok(format!("'{title}' is in the backlog"))
    }

    /// Takes an item out without starting it.
    ///
    /// Nothing is really lost: the reference keeps its own history, so
    /// `git log refs/gitrust/backlog` still has the sentence in it.
    pub fn backlog_remove(&self, id: &str) -> Result<String> {
        let items = self.backlog()?.items;
        let gone = items
            .iter()
            .find(|item| item.id == id)
            .context("that item is no longer in the backlog")?
            .title
            .clone();

        let kept: Vec<Item> = items.into_iter().filter(|item| item.id != id).collect();
        let signature = self.who()?;
        self.write_board(&kept, &self.notes_kept()?, &signature, &format!("drop: {gone}"))?;
        Ok(format!("'{gone}' dropped from the backlog"))
    }

    /// Makes the item's branch and takes it out of the waiting room.
    ///
    /// The branch goes first. If the name is already taken it stops there, and
    /// the item is still where it was: better to say the name clashes than to
    /// throw away the note and leave nothing behind.
    pub fn backlog_start(&self, id: &str) -> Result<String> {
        let items = self.backlog()?.items;
        let item = items
            .iter()
            .find(|item| item.id == id)
            .context("that item is no longer in the backlog")?
            .clone();

        let branch = item.branch();
        self.branch_here(&branch)?;

        // The sentence that was written is what the branch is for, and the
        // branch name is only what survives of it after being filed down to
        // fit a reference. It moves across in the same commit that takes the
        // item out, so there is no moment where those words belong to
        // nothing -- which is exactly what used to happen to them.
        let note = Note {
            branch: branch.clone(),
            title: item.title.clone(),
            body: String::new(),
            added: item.added,
            by: item.by.clone(),
        };
        let notes = [self.notes_without(&branch)?, vec![note]].concat();

        let kept: Vec<Item> = items.into_iter().filter(|item| item.id != id).collect();
        let signature = self.who()?;
        self.write_board(&kept, &notes, &signature, &format!("start: {}", item.title))?;
        Ok(format!("you are on {branch} now, started from the backlog"))
    }

    /// Sends the backlog to the remote so that everyone with the repository
    /// has it.
    ///
    /// Whatever is up there is brought down and joined first, so sharing never
    /// walks over something somebody else wrote while you were not looking.
    pub fn backlog_share(&self) -> Result<String> {
        let remote_name = self.backlog_remote().context("this repository has no remotes")?;
        self.fetch_from(&remote_name)?;

        let tip = self
            .board_tip()
            .context("there is nothing in the backlog to share")?
            .id();

        let mut remote = self
            .inner
            .find_remote(&remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;
        self.send(&mut remote, &[format!("{REF}:{REF}")])?;

        // The remote has it now, so the copy that stands for "what they have"
        // is moved up to match. Without this the column would go on saying
        // things are unshared until the next fetch.
        self.inner.reference(
            &remote_ref(&remote_name),
            tip,
            true,
            "backlog: shared with the remote",
        )?;

        Ok(format!("the backlog is on {remote_name} now"))
    }

    /// Joins the copy that came down from the remote with the one here.
    ///
    /// Called after every fetch. The three ordinary cases cost nothing: one
    /// side has nothing, or one is already ahead of the other. Only when both
    /// have written since they last met is there a merge, and one file per
    /// item is what makes that merge silent.
    pub(crate) fn absorb_backlog(&self, remote: &str) -> Result<()> {
        let Ok(theirs) = self
            .inner
            .find_reference(&remote_ref(remote))
            .and_then(|reference| reference.peel_to_commit())
        else {
            return Ok(());
        };

        let Some(ours) = self.board_tip() else {
            self.inner
                .reference(REF, theirs.id(), true, "backlog: first copy from the remote")?;
            return Ok(());
        };

        if ours.id() == theirs.id() || self.inner.graph_descendant_of(ours.id(), theirs.id())? {
            return Ok(());
        }
        if self.inner.graph_descendant_of(theirs.id(), ours.id())? {
            self.inner
                .reference(REF, theirs.id(), true, "backlog: moved up to the remote")?;
            return Ok(());
        }

        let mut index = self.inner.merge_commits(&ours, &theirs, None)?;
        anyhow::ensure!(
            !index.has_conflicts(),
            "the backlog here and the one on {remote} have both changed the same item"
        );

        let tree = self.inner.find_tree(index.write_tree_to(&self.inner)?)?;
        let signature = self.who()?;
        self.inner.commit(
            Some(REF),
            &signature,
            &signature,
            &format!("merge the backlog from {remote}"),
            &tree,
            &[&ours, &theirs],
        )?;
        Ok(())
    }

    /// The refspec that brings the backlog down, to go with a remote's own.
    ///
    /// A pattern rather than the name outright: a remote that has never had a
    /// backlog matches nothing and the fetch carries on, where asking for a
    /// reference by name that is not there would make every fetch fail.
    pub(crate) fn backlog_refspec(&self, remote: &str) -> String {
        format!("+refs/gitrust/backlog*:refs/gitrust/remotes/{remote}/backlog*")
    }

    /// The commit the written-down work is at, if anything was ever written.
    pub(crate) fn board_tip(&self) -> Option<Commit<'_>> {
        self.inner.find_reference(REF).ok()?.peel_to_commit().ok()
    }

    fn items_of(&self, commit: &Commit<'_>) -> Result<Vec<Item>> {
        let tree = commit.tree()?;
        let Some(folder) = tree.get_name("items") else {
            return Ok(Vec::new());
        };
        let folder = folder.to_object(&self.inner)?;
        let Some(folder) = folder.as_tree() else {
            return Ok(Vec::new());
        };

        let mut items = Vec::new();
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
            if let Some(item) = parse(name, &text) {
                items.push(item);
            }
        }

        // The last thing written, first, as on the rest of the board.
        items.sort_by(|a, b| b.added.cmp(&a.added).then_with(|| a.title.cmp(&b.title)));
        Ok(items)
    }

    /// Writes the whole list again as one commit on [`REF`].
    ///
    /// Rewriting all of it rather than editing a file in place sounds wasteful
    /// and is not: Git stores each item's blob once however many times it is
    /// written, and what matters for the merge is the tree that comes out, not
    /// how it was built.
    pub(crate) fn write_board(
        &self,
        items: &[Item],
        notes: &[Note],
        signature: &Signature<'_>,
        message: &str,
    ) -> Result<()> {
        let mut root = self.inner.treebuilder(None)?;

        if !items.is_empty() {
            let mut folder = self.inner.treebuilder(None)?;
            for item in items {
                let blob = self.inner.blob(item.render().as_bytes())?;
                folder.insert(format!("{}.txt", short(blob)), blob, 0o100_644)?;
            }
            let folder = folder.write()?;
            root.insert("items", folder, 0o040_000)?;
        }

        if !notes.is_empty() {
            let mut folder = self.inner.treebuilder(None)?;
            for note in notes {
                let blob = self.inner.blob(note.render().as_bytes())?;
                folder.insert(&note.file(), blob, 0o100_644)?;
            }
            let folder = folder.write()?;
            root.insert("notes", folder, 0o040_000)?;
        }

        let tree = self.inner.find_tree(root.write()?)?;
        let parent = self.board_tip();
        let parents: Vec<&Commit<'_>> = parent.iter().collect();

        self.inner
            .commit(Some(REF), signature, signature, message, &tree, &parents)?;
        Ok(())
    }

    /// How many of the items the remote has not been told about.
    ///
    /// It is the difference between the two trees, so taking something out
    /// counts as much as putting something in: both are things the remote does
    /// not know yet.
    fn unshared(&self) -> usize {
        let Some(remote) = self.backlog_remote() else {
            return 0;
        };

        // Never having written one is the only case with nothing to say. An
        // empty backlog is not that case: emptying it is itself a change the
        // remote has not been told about.
        let ours = self.board_tip().and_then(|commit| commit.tree().ok());
        let Some(ours) = ours else {
            return 0;
        };
        let theirs = self
            .inner
            .find_reference(&remote_ref(&remote))
            .ok()
            .and_then(|reference| reference.peel_to_commit().ok())
            .and_then(|commit| commit.tree().ok());

        self.inner
            .diff_tree_to_tree(theirs.as_ref(), Some(&ours), None)
            .map(|diff| diff.deltas().len())
            .unwrap_or(0)
    }

    /// The remote the backlog travels to: "origin" when it is there, which is
    /// nearly always, and otherwise the first one configured.
    fn backlog_remote(&self) -> Option<String> {
        let remotes = self.inner.remotes().ok()?;
        let names: Vec<&str> = remotes.iter().flatten().collect();
        if names.iter().any(|name| *name == "origin") {
            return Some("origin".to_string());
        }
        names.first().map(|name| name.to_string())
    }

    pub(crate) fn who(&self) -> Result<Signature<'_>> {
        self.inner
            .signature()
            .context("set user.name and user.email in Git first")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, project: Option<&str>) -> Item {
        Item {
            id: "abc123".to_string(),
            title: title.to_string(),
            project: project.map(str::to_string),
            added: 1_700_000_000,
            by: "Someone <someone@example.com>".to_string(),
        }
    }

    #[test]
    fn a_title_becomes_a_name_a_branch_can_have() {
        assert_eq!(slug("The footer breaks at 320px"), "the-footer-breaks-at-320px");
        assert_eq!(slug("  spaces   everywhere  "), "spaces-everywhere");
        assert_eq!(slug("café roto"), "caf-roto");
    }

    #[test]
    fn a_title_longer_than_a_branch_name_is_cut_without_a_trailing_hyphen() {
        let long = slug("one two three four five six seven eight nine ten eleven");
        assert!(long.len() <= LEAF);
        assert!(!long.ends_with('-'));
    }

    #[test]
    fn a_title_with_nothing_typeable_in_it_leaves_no_name() {
        assert_eq!(slug("¿¡—…!?"), "");
    }

    #[test]
    fn the_project_is_the_folder_the_branch_goes_in() {
        assert_eq!(item("Dark mode", Some("theme")).branch(), "theme/dark-mode");
        assert_eq!(item("Dark mode", None).branch(), "dark-mode");
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let original = item("The footer breaks at 320px", Some("fix"));
        let read = parse("abc123", &original.render()).expect("it should read back");
        assert_eq!(read, original);
    }

    #[test]
    fn an_item_with_no_project_says_nothing_about_one() {
        let original = item("Dark mode", None);
        let text = original.render();
        assert!(!text.contains("project:"));
        assert_eq!(parse("abc123", &text).expect("it should read back"), original);
    }

    #[test]
    fn a_field_nobody_here_knows_is_passed_over_not_refused() {
        let text = "title: Dark mode\nmood: hopeful\nadded: 1700000000\nby: Someone <someone@example.com>\n";
        let read = parse("abc123", text).expect("it should read back");
        assert_eq!(read.title, "Dark mode");
    }

    #[test]
    fn a_file_with_no_title_is_not_an_item() {
        assert!(parse("abc123", "added: 1700000000\n").is_none());
    }
}
