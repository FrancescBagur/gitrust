//! Núcleo de gitrust: lectura del repositorio y cálculo del grafo de commits.
//!
//! Este crate no sabe nada de interfaz gráfica. Todo lo que expone son datos
//! planos, de modo que la UI se pueda sustituir sin tocar la lógica Git.

pub mod branch;
pub mod detail;
pub mod diff;
pub mod graph;
pub mod merge;
pub mod message;
pub mod model;
pub mod remote;
pub mod repo;
pub mod stash;
pub mod status;
pub mod tag;

pub use detail::{CommitDetail, CommitFile, RefEntry};
pub use git2::Oid;
pub use diff::{DiffLine, FileDiff, Hunk, HunkId};
pub use graph::{layout, layout_with_working};
pub use message::{blocks, compose, Block, SUBJECT_LIMIT};
pub use model::{Commit, Graph, Link, RefKind, RefLabel, Row};
pub use remote::Tracking;
pub use repo::Repo;
pub use stash::StashEntry;
pub use status::{ChangeKind, FileChange, WorkingState};
