//! Núcleo de gitrust: lectura del repositorio y cálculo del grafo de commits.
//!
//! Este crate no sabe nada de interfaz gráfica. Todo lo que expone son datos
//! planos, de modo que la UI se pueda sustituir sin tocar la lógica Git.

pub mod detail;
pub mod diff;
pub mod graph;
pub mod message;
pub mod model;
pub mod repo;
pub mod status;

pub use detail::{CommitDetail, CommitFile, RefEntry};
pub use git2::Oid;
pub use diff::{DiffLine, FileDiff, Hunk, HunkId};
pub use graph::layout;
pub use message::{blocks, compose, Block, SUBJECT_LIMIT};
pub use model::{Commit, Graph, Link, RefKind, RefLabel, Row};
pub use repo::Repo;
pub use status::{ChangeKind, FileChange, WorkingState};
