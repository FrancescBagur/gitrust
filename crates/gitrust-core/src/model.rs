//! The data types shared between the repository reader and the UI.

use git2::Oid;

/// What kind of reference points at a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// A label shown on a commit: a branch, a tag or HEAD.
#[derive(Debug, Clone)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

/// A commit, flattened to the fields the UI needs in order to paint it.
#[derive(Debug, Clone)]
pub struct Commit {
    pub id: Oid,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub time: i64,
    pub parents: Vec<Oid>,
    pub refs: Vec<RefLabel>,
}

impl Commit {
    /// The commit that does not exist yet: the working tree's changes.
    ///
    /// Git gives the working tree no identifier, so this row carries the null
    /// Oid. No real commit can have it, so it is what tells this row from the
    /// rest without needing a field of its own, and the graph's layout treats
    /// it like any other: it hangs off HEAD by its parent.
    pub fn working(head: Oid) -> Self {
        Self {
            id: Oid::zero(),
            short_id: String::new(),
            summary: String::new(),
            author: String::new(),
            time: 0,
            parents: vec![head],
            refs: Vec::new(),
        }
    }

    /// Whether this row is the uncommitted changes instead of a commit.
    pub fn is_working(&self) -> bool {
        self.id.is_zero()
    }

    /// Whether this commit matches what is being looked for.
    ///
    /// It looks in what one remembers of a commit when looking for it: what it
    /// was about, who made it, or the start of its hash if it came copied from
    /// somewhere. The needle has to arrive lowercased already; this runs
    /// thousands of times a frame, and lowercasing it each time would be waste.
    pub fn matches(&self, needle: &str) -> bool {
        // The uncommitted-changes row has nothing to look in, and its null Oid
        // would match any needle that starts with a zero.
        if self.is_working() {
            return false;
        }

        if self.summary.to_lowercase().contains(needle)
            || self.author.to_lowercase().contains(needle)
            || self.short_id.starts_with(needle)
        {
            return true;
        }

        // A hash longer than the short one can only have been pasted, so it is
        // compared whole, and only when the needle looks like one.
        needle.len() > self.short_id.len()
            && needle.chars().all(|c| c.is_ascii_hexdigit())
            && self.id.to_string().starts_with(needle)
    }

    /// The references that point here, from the one that matters most to the
    /// one that matters least, each saying whether it is where you are.
    ///
    /// The branch you are on goes first: it is the answer to the question one
    /// asks when looking at the history. Behind it the other local ones, then
    /// the tags, and last the remotes, which nearly always repeat a local one
    /// and are the first to go when it does not all fit.
    ///
    /// HEAD's own mark does not come out on its own if there is a local branch
    /// with the same name: that branch is the one that is checked out, and
    /// saying it twice says no more. Only a detached HEAD stands alone.
    pub fn ordered_refs(&self) -> Vec<(&RefLabel, bool)> {
        let head = self
            .refs
            .iter()
            .find(|label| label.kind == RefKind::Head)
            .map(|label| label.name.as_str());

        let doubled = head.is_some_and(|name| {
            self.refs
                .iter()
                .any(|label| label.kind == RefKind::LocalBranch && label.name == name)
        });

        let mut out: Vec<(&RefLabel, bool)> = self
            .refs
            .iter()
            .filter(|label| !(doubled && label.kind == RefKind::Head))
            .map(|label| {
                let current = match label.kind {
                    RefKind::Head => true,
                    RefKind::LocalBranch => Some(label.name.as_str()) == head,
                    _ => false,
                };
                (label, current)
            })
            .collect();

        // Stable: within one rank, the order they came in is kept.
        out.sort_by_key(|(label, current)| rank(label.kind, *current));
        out
    }
}

/// How much a reference matters. Zero is the one that cannot be missing.
fn rank(kind: RefKind, current: bool) -> u8 {
    if current {
        return 0;
    }
    match kind {
        RefKind::Head => 0,
        RefKind::LocalBranch => 1,
        RefKind::Tag => 2,
        RefKind::RemoteBranch => 3,
    }
}

/// A line drawn in the band that runs from one row to the next.
///
/// `from` is the lane in the current row and `to` the lane in the row below.
/// When the two match the line is vertical; if not, it describes a curve.
#[derive(Debug, Clone, Copy)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    pub color: usize,
}

/// A row of the graph: the commit, the lane its dot goes in, and the lines
/// that leave it towards the next row.
#[derive(Debug, Clone)]
pub struct Row {
    pub commit: Commit,
    pub lane: usize,
    pub color: usize,
    pub links: Vec<Link>,
}

/// The whole graph, ready to paint.
#[derive(Debug, Default)]
pub struct Graph {
    pub rows: Vec<Row>,
    pub lane_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(refs: &[(&str, RefKind)]) -> Commit {
        Commit {
            id: Oid::zero(),
            short_id: String::new(),
            summary: String::new(),
            author: String::new(),
            time: 0,
            parents: Vec::new(),
            refs: refs
                .iter()
                .map(|(name, kind)| RefLabel { name: name.to_string(), kind: *kind })
                .collect(),
        }
    }

    fn names(commit: &Commit) -> Vec<(&str, bool)> {
        commit
            .ordered_refs()
            .into_iter()
            .map(|(label, current)| (label.name.as_str(), current))
            .collect()
    }

    fn searchable(summary: &str, author: &str, id: Oid) -> Commit {
        Commit {
            id,
            short_id: id.to_string().chars().take(7).collect(),
            summary: summary.to_string(),
            author: author.to_string(),
            time: 0,
            parents: Vec::new(),
            refs: Vec::new(),
        }
    }

    #[test]
    fn the_search_looks_at_message_author_and_hash() {
        let id = Oid::from_str("3f2a1b9c4d5e6f708192a3b4c5d6e7f809a1b2c3").unwrap();
        let commit = searchable("feat: la Paleta de carriles", "Francesc", id);

        // Case blind, and on any of the three sides.
        assert!(commit.matches("paleta"));
        assert!(commit.matches("carriles"));
        assert!(commit.matches("franc"));
        assert!(commit.matches("3f2a1b9"));
        assert!(!commit.matches("rebase"));
    }

    #[test]
    fn a_hash_longer_than_the_short_one_is_compared_whole() {
        let id = Oid::from_str("3f2a1b9c4d5e6f708192a3b4c5d6e7f809a1b2c3").unwrap();
        let commit = searchable("feat: algo", "Quien sea", id);

        assert!(commit.matches("3f2a1b9c4d5e"));
        assert!(!commit.matches("3f2a1b9c4d5f"));
        // And what is not a hash is not compared as one, however long it is.
        assert!(!commit.matches("zzzzzzzzzzzz"));
    }

    #[test]
    fn the_uncommitted_changes_row_matches_nothing() {
        let commit = Commit::working(Oid::zero());
        assert!(!commit.matches("a"));
        // Not even with a needle its null Oid would satisfy.
        assert!(!commit.matches("0000"));
    }

    #[test]
    fn the_checked_out_branch_absorbs_heads_mark() {
        let commit = commit(&[
            ("main", RefKind::Head),
            ("main", RefKind::LocalBranch),
            ("origin/main", RefKind::RemoteBranch),
        ]);
        assert_eq!(names(&commit), [("main", true), ("origin/main", false)]);
    }

    #[test]
    fn among_several_locals_only_one_is_checked_out() {
        let commit = commit(&[
            ("arreglo", RefKind::Head),
            ("main", RefKind::LocalBranch),
            ("arreglo", RefKind::LocalBranch),
        ]);
        assert_eq!(names(&commit), [("arreglo", true), ("main", false)]);
    }

    #[test]
    fn with_a_detached_head_the_mark_stands_alone() {
        let commit = commit(&[("HEAD", RefKind::Head), ("v1.0", RefKind::Tag)]);
        assert_eq!(names(&commit), [("HEAD", true), ("v1.0", false)]);
    }

    #[test]
    fn remotes_come_last_and_tags_come_before_them() {
        let commit = commit(&[
            ("origin/main", RefKind::RemoteBranch),
            ("v2", RefKind::Tag),
            ("main", RefKind::LocalBranch),
        ]);
        assert_eq!(
            names(&commit),
            [("main", false), ("v2", false), ("origin/main", false)]
        );
    }

    #[test]
    fn a_commit_with_no_references_gives_none() {
        assert!(commit(&[]).ordered_refs().is_empty());
    }
}
