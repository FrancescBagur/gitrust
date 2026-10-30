//! Estado del árbol de trabajo: qué ha cambiado y si está preparado o no.

use git2::Status;

/// Qué le ha pasado a un fichero.
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
    /// Letra corta para la lista, al estilo de `git status --short`.
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

/// Los dos lados del índice: lo que iría al próximo commit y lo que no.
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

/// Cambio registrado en el índice respecto a HEAD.
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

/// Cambio del árbol de trabajo respecto al índice.
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
