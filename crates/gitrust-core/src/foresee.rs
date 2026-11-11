//! Conflicts that have not happened yet.
//!
//! A branch is going somewhere: into `main`, usually, by way of a pull
//! request. Whether it will go in cleanly is a thing Git can say today, with
//! nothing touched -- the merge is done in memory and thrown away -- and it
//! is a thing worth knowing today, while the change that caused it is still
//! fresh, rather than on the afternoon the branch is meant to land.
//!
//! Nothing here writes. Not to the working tree, not to the index, not to a
//! reference: the answer is a list of file names and that is all of it.

use anyhow::{Context, Result};
use git2::Oid;

use crate::{RefEntry, RefKind, RemoteRepo, Repo};

/// What merging into the target would run into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Foreseen {
    /// The branch it was checked against: `origin/main`.
    pub target: String,
    /// The files both sides have changed in ways that do not fit together.
    /// Empty is the good answer.
    pub files: Vec<String>,
}

impl Foreseen {
    /// The sentence for the bar, or nothing when there is nothing to say.
    pub fn says(&self) -> Option<String> {
        match self.files.len() {
            0 => None,
            1 => Some(format!("1 file would conflict with {}", self.target)),
            n => Some(format!("{n} files would conflict with {}", self.target)),
        }
    }
}

/// The branch this one is most likely on its way into.
///
/// The main branch of the remote the current branch follows, or of `origin`,
/// and failing a remote the main branch here. "Main" is `main` and then
/// `master`: there is no setting that says which branch a repository lands
/// its work on, and those two names are what nearly all of them use.
pub fn likely_target<'a>(refs: &'a [RefEntry], follows: Option<&str>) -> Option<&'a RefEntry> {
    // `origin/feat/login` follows the remote called `origin`.
    let remote = follows
        .and_then(|upstream| upstream.split_once('/'))
        .map(|(remote, _)| remote)
        .unwrap_or("origin");

    let find = |kind: RefKind, name: &str| {
        refs.iter().find(|entry| entry.kind == kind && entry.name == name)
    };

    ["main", "master"]
        .iter()
        .find_map(|branch| find(RefKind::RemoteBranch, &format!("{remote}/{branch}")))
        .or_else(|| ["main", "master"].iter().find_map(|branch| find(RefKind::LocalBranch, branch)))
}

impl Repo {
    /// The branch [`foresee`] would check against, and the commit it is at.
    ///
    /// What the remote itself calls its main branch wins over the guess,
    /// when the remote has said: a clone records it as `origin/HEAD`.
    ///
    /// [`foresee`]: Repo::foresee
    pub fn target(&self) -> Option<(String, Oid)> {
        // Each name is looked up where it would be, rather than the list of
        // every reference being read and searched: this is asked on every
        // reload, and a repository can have thousands of them.
        let remote = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.name().map(str::to_string))
            .and_then(|branch| self.inner.branch_upstream_remote(&branch).ok())
            .and_then(|remote| remote.as_str().map(str::to_string))
            .unwrap_or_else(|| "origin".to_string());

        let at = |name: &str| {
            let found = self.inner.find_reference(name).ok()?.resolve().ok()?;
            Some((found.shorthand()?.to_string(), found.peel_to_commit().ok()?.id()))
        };

        at(&format!("refs/remotes/{remote}/HEAD"))
            .or_else(|| at(&format!("refs/remotes/{remote}/main")))
            .or_else(|| at(&format!("refs/remotes/{remote}/master")))
            .or_else(|| at("refs/heads/main"))
            .or_else(|| at("refs/heads/master"))
    }

    /// Merges the branch into its target in memory and says what clashed.
    ///
    /// `None` when there is nothing to check: no target, no commit to check
    /// from, or a branch that already has everything the target has -- which
    /// cannot conflict with it, and is most branches most of the time.
    pub fn foresee(&self) -> Result<Option<Foreseen>> {
        let Some((target, theirs)) = self.target() else {
            return Ok(None);
        };
        let Some(ours) = self.head_id() else {
            return Ok(None);
        };
        if ours == theirs || self.inner.graph_descendant_of(ours, theirs).unwrap_or(false) {
            return Ok(None);
        }

        let ours = self.inner.find_commit(ours)?;
        let theirs = self.inner.find_commit(theirs)?;
        let index = self
            .inner
            .merge_commits(&ours, &theirs, None)
            .with_context(|| format!("could not try the merge with {target}"))?;

        let mut files = Vec::new();
        if index.has_conflicts() {
            for clash in index.conflicts()? {
                let clash = clash?;
                // Any of the three that is there: a file deleted on one side
                // has no entry on that side and is still the same file.
                let entry = clash.our.or(clash.their).or(clash.ancestor);
                if let Some(entry) = entry {
                    files.push(String::from_utf8_lossy(&entry.path).into_owned());
                }
            }
        }
        files.sort();
        files.dedup();
        Ok(Some(Foreseen { target, files }))
    }
}

impl RemoteRepo {
    /// The same question, asked of the Git on the server.
    ///
    /// `git merge-tree --write-tree` is that Git's own merge in memory. It
    /// arrived in Git 2.38; a server with an older one cannot answer, and
    /// that is an error here for the caller to keep quiet about.
    pub fn foresee(&self, target: &str) -> Result<Option<Foreseen>> {
        let (status, out, err) = self.git_status(&[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "-z",
            "HEAD",
            target,
        ])?;
        // Nought is a clean merge and one is a merge with conflicts. Past
        // that it did not merge at all.
        anyhow::ensure!(
            status == 0 || status == 1,
            "{}",
            if err.trim().is_empty() { "the server's Git could not try the merge" } else { err.trim() }
        );
        Ok(Some(Foreseen { target: target.to_string(), files: merge_tree_names(&out) }))
    }
}

/// The file names in what `merge-tree --name-only -z` prints: the tree it
/// wrote, and then each file that clashed, with a NUL after every one.
fn merge_tree_names(out: &str) -> Vec<String> {
    let mut files: Vec<String> = out
        .split('\0')
        .skip(1)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: RefKind) -> RefEntry {
        RefEntry { name: name.to_string(), kind, target: Oid::zero(), is_head: false }
    }

    #[test]
    fn the_target_is_the_main_branch_of_the_remote_being_followed() {
        let refs = [
            entry("main", RefKind::LocalBranch),
            entry("origin/main", RefKind::RemoteBranch),
            entry("upstream/master", RefKind::RemoteBranch),
            entry("feat/login", RefKind::LocalBranch),
        ];
        let named = |follows| likely_target(&refs, follows).map(|entry| entry.name.as_str());
        assert_eq!(named(Some("origin/feat/login")), Some("origin/main"));
        assert_eq!(named(Some("upstream/feat/login")), Some("upstream/master"));
        // Following nothing, it is origin's.
        assert_eq!(named(None), Some("origin/main"));
    }

    #[test]
    fn with_no_remote_the_target_is_the_main_branch_here() {
        let refs = [entry("master", RefKind::LocalBranch), entry("feat/login", RefKind::LocalBranch)];
        assert_eq!(likely_target(&refs, None).map(|entry| entry.name.as_str()), Some("master"));
        // A tag called main is not a branch to merge into.
        let refs = [entry("main", RefKind::Tag)];
        assert!(likely_target(&refs, None).is_none());
    }

    #[test]
    fn what_merge_tree_prints_is_a_tree_and_then_the_files() {
        assert_eq!(merge_tree_names("abc123\0b.txt\0a.txt\0"), vec!["a.txt", "b.txt"]);
        // A clean merge prints the tree and nothing after it.
        assert!(merge_tree_names("abc123\0").is_empty());
        assert!(merge_tree_names("").is_empty());
    }

    #[test]
    fn nothing_to_say_is_said_as_nothing() {
        let clean = Foreseen { target: "origin/main".to_string(), files: Vec::new() };
        assert_eq!(clean.says(), None);
        let one = Foreseen { target: "origin/main".to_string(), files: vec!["a".to_string()] };
        assert_eq!(one.says().unwrap(), "1 file would conflict with origin/main");
    }
}
