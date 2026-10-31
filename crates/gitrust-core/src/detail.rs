//! A commit's detail, and the repository's references.

use git2::Oid;

use crate::model::RefKind;
use crate::status::ChangeKind;

/// A file a commit touched.
#[derive(Debug, Clone)]
pub struct CommitFile {
    pub path: String,
    pub kind: ChangeKind,
}

/// Everything the detail panel needs from a commit.
#[derive(Debug, Clone)]
pub struct CommitDetail {
    pub id: Oid,
    pub short_id: String,
    pub summary: String,
    /// The body of the message, without the subject or the blank line.
    pub body: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub parents: Vec<Oid>,
    pub files: Vec<CommitFile>,
}

impl CommitDetail {
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// How many files are added, changed and deleted.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut added = 0;
        let mut modified = 0;
        let mut deleted = 0;
        for file in &self.files {
            match file.kind {
                ChangeKind::Added => added += 1,
                ChangeKind::Deleted => deleted += 1,
                _ => modified += 1,
            }
        }
        (added, modified, deleted)
    }
}

/// A reference in the repository, for the sidebar.
#[derive(Debug, Clone)]
pub struct RefEntry {
    /// The short name: `main`, `origin/main`, `v1.0`.
    pub name: String,
    pub kind: RefKind,
    pub target: Oid,
    /// Whether this is the branch HEAD is on.
    pub is_head: bool,
}

impl RefEntry {
    /// The folder in the name, to group the way the branch tree does:
    /// `feat/login` lives under `feat`, and `main` at the root.
    pub fn folder(&self) -> Option<&str> {
        let name = match self.kind {
            // On a remote the first segment is the remote, not a folder.
            RefKind::RemoteBranch => self.name.split_once('/').map(|(_, rest)| rest)?,
            _ => self.name.as_str(),
        };
        name.rsplit_once('/').map(|(folder, _)| folder)
    }

    /// The name without the folder.
    pub fn leaf(&self) -> &str {
        self.name.rsplit_once('/').map_or(self.name.as_str(), |(_, leaf)| leaf)
    }
}
