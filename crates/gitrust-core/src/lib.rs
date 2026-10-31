//! gitrust's core: reading the repository and working out the commit graph.
//!
//! This crate knows nothing about the user interface. Everything it exposes is
//! plain data, so the UI could be replaced without touching the Git logic.

pub mod board;
pub mod branch;
pub mod detail;
pub mod diff;
pub mod forge;
pub mod graph;
pub mod merge;
pub mod message;
pub mod model;
pub mod remote;
pub mod repo;
pub mod stash;
pub mod status;
pub mod tag;

pub use board::{Board, Card, Lane};
pub use detail::{CommitDetail, CommitFile, RefEntry};
pub use git2::Oid;
pub use diff::{DiffLine, FileDiff, Hunk, HunkId};
pub use forge::{PullRequest, PullRequestState, PullRequests};
pub use graph::{layout, layout_with_working};
pub use message::{blocks, compose, Block, SUBJECT_LIMIT};
pub use model::{Commit, Graph, Link, RefKind, RefLabel, Row};
pub use remote::{PullMode, Tracking};
pub use repo::Repo;
pub use stash::StashEntry;
pub use status::{ChangeKind, FileChange, WorkingState};
