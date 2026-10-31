//! How a file's diff is represented, already cut into hunks.

/// A line of the diff. `origin` is libgit2's mark: `+`, `-` or a space.
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub origin: char,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

/// Identifies a hunk by the ranges in its `@@ -a,b +c,d @@` header.
///
/// It is needed because staging a single hunk means recognising it inside a
/// diff that libgit2 works out again: counting positions does not do, since
/// the set of hunks changes as soon as one of them is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HunkId {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

impl HunkId {
    /// The same ranges seen from an inverted diff.
    ///
    /// Taking a hunk out of the index applies the diff backwards, and in that
    /// diff the old and new sides are swapped.
    pub fn reversed(self) -> Self {
        Self {
            old_start: self.new_start,
            old_lines: self.new_lines,
            new_start: self.old_start,
            new_lines: self.old_lines,
        }
    }
}

/// A contiguous block of changes, with its `@@ ... @@` header.
#[derive(Debug, Clone)]
pub struct Hunk {
    pub header: String,
    pub id: HunkId,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// How many lines this hunk adds and how many it takes away.
    pub fn counts(&self) -> (usize, usize) {
        let added = self.lines.iter().filter(|line| line.origin == '+').count();
        let removed = self.lines.iter().filter(|line| line.origin == '-').count();
        (added, removed)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
}

impl FileDiff {
    /// How many lines are added and taken away in the whole file.
    pub fn counts(&self) -> (usize, usize) {
        self.hunks.iter().fold((0, 0), |(added, removed), hunk| {
            let (a, r) = hunk.counts();
            (added + a, removed + r)
        })
    }
}
