//! The state of the working tree: what has changed, and whether it is staged.

use git2::Status;

/// What has happened to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    /// A short letter for the list, in the style of `git status --short`.
    pub fn tag(self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::TypeChange => "T",
            ChangeKind::Untracked => "?",
            ChangeKind::Conflicted => "!",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    pub staged: bool,
}

/// The two sides of the index: what would go in the next commit, and what would not.
#[derive(Debug, Default, Clone)]
pub struct WorkingState {
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
}

impl WorkingState {
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty()
    }
}

/// Two numbers that stay the same for as long as a repository does.
///
/// Nothing here is told when a file changes, so the window finds out by
/// looking, and what it compares is this: a number for what Git keeps and
/// a number for the working tree. They mean nothing in themselves and are
/// not kept from one run to the next -- two of them are only ever asked
/// whether they are equal.
///
/// Two, because they are answered differently. A branch that has moved is
/// the history read again. A file that was saved is the list of changes
/// read again and nothing else, and somebody halfway through a commit
/// message should not lose their place for having saved a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pulse {
    /// HEAD, the references, the stashes and what is half-done.
    pub git: u64,
    /// What has changed in the working tree and the index, and when.
    pub tree: u64,
}

/// A change recorded in the index against HEAD.
pub(crate) fn staged_kind(status: Status) -> Option<ChangeKind> {
    if status.contains(Status::INDEX_NEW) {
        Some(ChangeKind::Added)
    } else if status.contains(Status::INDEX_MODIFIED) {
        Some(ChangeKind::Modified)
    } else if status.contains(Status::INDEX_DELETED) {
        Some(ChangeKind::Deleted)
    } else if status.contains(Status::INDEX_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if status.contains(Status::INDEX_TYPECHANGE) {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

/// A change in the working tree against the index.
pub(crate) fn unstaged_kind(status: Status) -> Option<ChangeKind> {
    if status.contains(Status::CONFLICTED) {
        Some(ChangeKind::Conflicted)
    } else if status.contains(Status::WT_NEW) {
        Some(ChangeKind::Untracked)
    } else if status.contains(Status::WT_MODIFIED) {
        Some(ChangeKind::Modified)
    } else if status.contains(Status::WT_DELETED) {
        Some(ChangeKind::Deleted)
    } else if status.contains(Status::WT_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if status.contains(Status::WT_TYPECHANGE) {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}
