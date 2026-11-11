//! Opening the repository, reading commits, and working on the index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{
    build::CheckoutBuilder, ApplyLocation, ApplyOptions, Delta, Diff, DiffFormat, DiffHunk,
    DiffOptions, ObjectType, Oid, Repository, Sort, StatusOptions,
};

use crate::detail::{CommitDetail, CommitFile, RefEntry};
use crate::diff::{DiffLine, FileDiff, Hunk, HunkId};
use crate::model::{Commit, RefKind, RefLabel};
use crate::status::{staged_kind, unstaged_kind, ChangeKind, FileChange, WorkingState};

pub struct Repo {
    pub(crate) inner: Repository,
    /// Who the window is working as, when somebody has said so. It is lent
    /// to the repository on the way in and never read out of it: a profile
    /// is about this machine, and a repository knows nothing about which of
    /// its owner's several lives it belongs to.
    pub(crate) profile: Option<crate::Profile>,
    /// Whether a fetch is asked to take away the remote branches that have
    /// gone from the remote. When it is not, nothing is asked either way and
    /// Git's own `fetch.prune` decides, as it did before there was a choice.
    pub(crate) prune: bool,
}

impl Repo {
    /// Opens the repository that holds `path`, climbing the parent directories
    /// the way `git` does from a subfolder.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let inner = Repository::discover(path)
            .with_context(|| format!("no Git repository at {}", path.display()))?;
        Ok(Self { inner, profile: None, prune: false })
    }

    /// Works under a profile: its name on the commits it writes, its key on
    /// the remotes it reaches, its accounts at the forges it asks.
    ///
    /// Said once, on the way in, rather than handed to each call that needs
    /// it. A commit and the push a second later have to be the same person,
    /// and an argument repeated on every call is an argument that one day
    /// one of them is missing.
    pub fn using(mut self, profile: Option<crate::Profile>) -> Self {
        self.profile = profile;
        self
    }

    /// Fetches the way the preferences say: pruning, or leaving that to Git.
    pub fn pruning(mut self, prune: bool) -> Self {
        self.prune = prune;
        self
    }

    /// Who the commits are written as: the profile, or Git's own settings.
    ///
    /// The profile wins when it has both halves of a name, and otherwise
    /// nothing changes -- which is what lets a profile exist for its key
    /// alone and leave the name on the commits where it was.
    pub(crate) fn who(&self) -> Result<git2::Signature<'static>> {
        if let Some((name, email)) = self.profile.as_ref().and_then(crate::Profile::who) {
            return git2::Signature::now(name, email)
                .with_context(|| format!("'{name} <{email}>' is not a name Git will write"));
        }
        self.inner
            .signature()
            .context("set user.name and user.email in Git first, or work under a profile")
    }

    /// The path of the working tree, or of the repository itself if it is bare.
    pub fn workdir(&self) -> PathBuf {
        self.inner
            .workdir()
            .unwrap_or_else(|| self.inner.path())
            .to_path_buf()
    }

    /// The current branch's name, or the short hash if HEAD is detached.
    /// The commit HEAD is on, whether or not that is a branch.
    pub fn head_oid(&self) -> Option<crate::Oid> {
        self.inner.head().ok()?.peel_to_commit().ok().map(|commit| commit.id())
    }

    pub fn head_name(&self) -> String {
        // Off a branch, libgit2 gives "HEAD" as the shorthand, which reads
        // like a branch called HEAD. Asking outright is the only way to tell.
        if self.inner.head_detached().unwrap_or(false) {
            return "detached HEAD".to_string();
        }
        match self.inner.head() {
            Ok(head) => head
                .shorthand()
                .map(str::to_string)
                .unwrap_or_else(|| "detached HEAD".to_string()),
            Err(_) => "no commits".to_string(),
        }
    }

    /// The commit HEAD points at, if the repository has any.
    pub fn head_id(&self) -> Option<Oid> {
        self.inner.head().ok()?.peel(ObjectType::Commit).ok().map(|object| object.id())
    }

    // ----------------------------------------------------------------- history

    /// Walks the history of every reference and gives back up to `limit`
    /// commits in topological and date order, newest first.
    pub fn commits(&self, limit: usize) -> Result<Vec<Commit>> {
        let labels = self.ref_labels()?;

        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;

        // A glob fails if no reference matches, which is normal in a freshly
        // made repository or one with no remotes. For us that is no error.
        let _ = walk.push_glob("refs/heads/*");
        let _ = walk.push_glob("refs/remotes/*");
        let _ = walk.push_glob("refs/tags/*");
        if let Ok(head) = self.inner.head() {
            if let Some(oid) = head.target() {
                let _ = walk.push(oid);
            }
        }

        let mut out = Vec::new();
        for oid in walk.take(limit) {
            let oid = oid?;
            let commit = self.inner.find_commit(oid)?;
            let author = commit.author();

            out.push(Commit {
                id: oid,
                short_id: oid.to_string().chars().take(7).collect(),
                summary: commit.summary().unwrap_or("(no message)").to_string(),
                author: author.name().unwrap_or("(unknown)").to_string(),
                time: commit.time().seconds(),
                parents: commit.parent_ids().collect(),
                refs: labels.get(&oid).cloned().unwrap_or_default(),
            });
        }

        Ok(out)
    }

    /// A map from commit to the references that point at it.
    pub(crate) fn ref_labels(&self) -> Result<HashMap<Oid, Vec<RefLabel>>> {
        let mut map: HashMap<Oid, Vec<RefLabel>> = HashMap::new();

        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(name) = reference.name() else {
                continue;
            };
            // An annotated tag points at a tag object, not the commit: peel it.
            let Ok(object) = reference.peel(ObjectType::Commit) else {
                continue;
            };

            let label = if let Some(branch) = name.strip_prefix("refs/heads/") {
                RefLabel { name: branch.to_string(), kind: RefKind::LocalBranch }
            } else if let Some(branch) = name.strip_prefix("refs/remotes/") {
                RefLabel { name: branch.to_string(), kind: RefKind::RemoteBranch }
            } else if let Some(tag) = name.strip_prefix("refs/tags/") {
                RefLabel { name: tag.to_string(), kind: RefKind::Tag }
            } else {
                continue;
            };

            map.entry(object.id()).or_default().push(label);
        }

        // HEAD's mark carries the name of the branch it is on, not the word
        // "HEAD". That is how you tell which of the branches pointing at this
        // commit is the one you have checked out, which with two or three on
        // the same commit could not be guessed. Detached, `shorthand` already
        // gives back "HEAD" and the mark stands alone, which is right.
        if let Ok(head) = self.inner.head() {
            if let Ok(object) = head.peel(ObjectType::Commit) {
                let name = head.shorthand().unwrap_or("HEAD").to_string();
                map.entry(object.id())
                    .or_default()
                    .insert(0, RefLabel { name, kind: RefKind::Head });
            }
        }

        Ok(map)
    }

    // ------------------------------------------------------------ working tree

    /// The changed files, split between index and working tree. One file can
    /// show up on both sides if it is half staged.
    pub fn status(&self) -> Result<WorkingState> {
        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);

        let statuses = self.inner.statuses(Some(&mut options))?;
        let mut state = WorkingState::default();

        for entry in statuses.iter() {
            let Some(path) = entry.path() else {
                continue;
            };
            let status = entry.status();

            if let Some(kind) = staged_kind(status) {
                state.staged.push(FileChange { path: path.to_string(), kind, staged: true });
            }
            if let Some(kind) = unstaged_kind(status) {
                state.unstaged.push(FileChange { path: path.to_string(), kind, staged: false });
            }
        }

        state.staged.sort_by(|a, b| a.path.cmp(&b.path));
        state.unstaged.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(state)
    }

    /// A file's diff. With `staged` it compares HEAD against the index; without
    /// it, the index against the working tree.
    pub fn file_diff(&self, path: &str, staged: bool) -> Result<FileDiff> {
        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3).include_untracked(true).recurse_untracked_dirs(true);

        let diff = if staged {
            let tree = match self.inner.head() {
                Ok(head) => Some(head.peel_to_tree()?),
                Err(_) => None,
            };
            self.inner.diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?
        } else {
            self.inner.diff_index_to_workdir(None, Some(&mut options))?
        };

        collect_file_diff(&diff, path)
    }

    // ---------------------------------------------------------------- a commit

    /// The message, the authorship and the files a commit touched, against its
    /// first parent. A root commit is compared with the empty tree.
    pub fn commit_detail(&self, oid: Oid) -> Result<CommitDetail> {
        let commit = self.inner.find_commit(oid)?;
        let author = commit.author();

        let tree = commit.tree()?;
        let parent_tree = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };
        let diff = self
            .inner
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

        let mut files: Vec<CommitFile> = diff
            .deltas()
            .filter_map(|delta| {
                let path = delta.new_file().path().or_else(|| delta.old_file().path())?;
                Some(CommitFile {
                    path: path.display().to_string(),
                    kind: delta_kind(delta.status()),
                })
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));

        // The message is "subject\n\nbody"; we keep the two parts apart.
        let message = commit.message().unwrap_or("");
        let body = message
            .split_once("\n\n")
            .map(|(_, body)| body.trim_end().to_string())
            .unwrap_or_default();

        Ok(CommitDetail {
            id: oid,
            short_id: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("(no message)").to_string(),
            body,
            author: author.name().unwrap_or("(unknown)").to_string(),
            email: author.email().unwrap_or_default().to_string(),
            time: commit.time().seconds(),
            parents: commit.parent_ids().collect(),
            files,
        })
    }

    /// A file's diff inside a commit, against its first parent.
    pub fn commit_file_diff(&self, oid: Oid, path: &str) -> Result<FileDiff> {
        let commit = self.inner.find_commit(oid)?;
        let tree = commit.tree()?;
        let parent_tree = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };

        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3);

        let diff = self.inner.diff_tree_to_tree(
            parent_tree.as_ref(),
            Some(&tree),
            Some(&mut options),
        )?;
        collect_file_diff(&diff, path)
    }

    /// Every reference, for the sidebar. Each remote's `HEAD` is left out: it
    /// is an alias, not one more branch.
    pub fn references(&self) -> Result<Vec<RefEntry>> {
        let head = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string));

        let mut out = Vec::new();
        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(name) = reference.name() else {
                continue;
            };
            if name.ends_with("/HEAD") {
                continue;
            }
            let Ok(object) = reference.peel(ObjectType::Commit) else {
                continue;
            };

            let (short, kind) = if let Some(branch) = name.strip_prefix("refs/heads/") {
                (branch.to_string(), RefKind::LocalBranch)
            } else if let Some(branch) = name.strip_prefix("refs/remotes/") {
                (branch.to_string(), RefKind::RemoteBranch)
            } else if let Some(tag) = name.strip_prefix("refs/tags/") {
                (tag.to_string(), RefKind::Tag)
            } else {
                continue;
            };

            let is_head = kind == RefKind::LocalBranch && head.as_deref() == Some(short.as_str());
            out.push(RefEntry { name: short, kind, target: object.id(), is_head });
        }

        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Adds a file to the index. If it is gone from disk it is marked deleted.
    pub fn stage(&self, path: &str) -> Result<()> {
        let mut index = self.inner.index()?;
        let exists = self.inner.workdir().map(|dir| dir.join(path).exists()).unwrap_or(false);

        if exists {
            index.add_path(Path::new(path))?;
        } else {
            index.remove_path(Path::new(path))?;
        }
        index.write()?;
        Ok(())
    }

    /// Puts a file in the index back to the state it has in HEAD.
    pub fn unstage(&self, path: &str) -> Result<()> {
        match self.inner.head() {
            Ok(head) => {
                let object = head.peel(ObjectType::Commit)?;
                self.inner.reset_default(Some(&object), [path])?;
            }
            // With no HEAD there is nothing to go back to: taking it out is enough.
            Err(_) => {
                let mut index = self.inner.index()?;
                index.remove_path(Path::new(path))?;
                index.write()?;
            }
        }
        Ok(())
    }

    /// Stages a single hunk, leaving the rest of the file unstaged.
    ///
    /// It applies to the index the working tree's diff filtered down to that
    /// hunk, which is what `git add -p` does underneath.
    pub fn stage_hunk(&self, path: &str, hunk: HunkId) -> Result<()> {
        let mut options = DiffOptions::new();
        options
            .pathspec(path)
            .context_lines(3)
            .include_untracked(true)
            .recurse_untracked_dirs(true);

        let diff = self.inner.diff_index_to_workdir(None, Some(&mut options))?;
        self.apply_hunk(&diff, hunk, ApplyLocation::Index)
    }

    /// Puts a single hunk back from the index to the working tree.
    ///
    /// The diff is worked out inverted, so that applying it to the index undoes
    /// the change. In an inverted diff the sides are swapped, so the hunk is
    /// looked for with its ranges inverted too.
    pub fn unstage_hunk(&self, path: &str, hunk: HunkId) -> Result<()> {
        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3).reverse(true);

        let tree = match self.inner.head() {
            Ok(head) => Some(head.peel_to_tree()?),
            Err(_) => None,
        };
        let diff = self
            .inner
            .diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?;

        self.apply_hunk(&diff, hunk.reversed(), ApplyLocation::Index)
    }

    pub(crate) fn apply_hunk(&self, diff: &Diff<'_>, wanted: HunkId, to: ApplyLocation) -> Result<()> {
        let mut applied = false;
        {
            let mut options = ApplyOptions::new();
            options.hunk_callback(|hunk| match hunk {
                Some(hunk) => {
                    let matches = hunk_id(&hunk) == wanted;
                    applied |= matches;
                    matches
                }
                // With no hunk it is the file header: it has to go through.
                None => true,
            });

            self.inner.apply(diff, to, Some(&mut options))?;
        }

        anyhow::ensure!(applied, "that hunk is gone; reload the diff");
        Ok(())
    }

    pub fn stage_all(&self, paths: &[String]) -> Result<()> {
        for path in paths {
            self.stage(path)?;
        }
        Ok(())
    }

    pub fn unstage_all(&self, paths: &[String]) -> Result<()> {
        for path in paths {
            self.unstage(path)?;
        }
        Ok(())
    }

    // -------------------------------------------------------------- discarding

    /// Throws away a file's changes in the working tree.
    ///
    /// It goes back to what the index says, not to what HEAD says: if the file
    /// had a part staged, that part is kept. It is what `git restore <file>`
    /// does, and it is what belongs to a list that keeps the two sides apart.
    ///
    /// An untracked file has no index to go back to: there the change is the
    /// whole file, so it is deleted. That leaves no copy anywhere, and it is
    /// the one operation in the whole app there is no coming back from.
    pub fn discard(&self, path: &str) -> Result<()> {
        if !self.in_index(path)? {
            let file = self.workdir().join(path);
            if file.exists() {
                std::fs::remove_file(&file)
                    .with_context(|| format!("could not delete {path}"))?;
            }
            return Ok(());
        }

        let mut options = CheckoutBuilder::new();
        options.force().path(path);

        self.inner
            .checkout_index(None, Some(&mut options))
            .with_context(|| format!("could not discard the changes to {path}"))?;
        Ok(())
    }

    /// Throws away a single hunk from the working tree and leaves the rest.
    ///
    /// The diff is worked out inverted, as it is for unstaging, but applied to
    /// the tree: that way it undoes that piece where it is written.
    pub fn discard_hunk(&self, path: &str, hunk: HunkId) -> Result<()> {
        anyhow::ensure!(
            self.in_index(path)?,
            "{path} is not in the index yet: discard it whole"
        );

        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3).reverse(true);

        let diff = self.inner.diff_index_to_workdir(None, Some(&mut options))?;
        self.apply_hunk(&diff, hunk.reversed(), ApplyLocation::WorkDir)
    }

    pub fn discard_all(&self, paths: &[String]) -> Result<()> {
        for path in paths {
            self.discard(path)?;
        }
        Ok(())
    }

    /// Whether Git knows anything about that file, or it is brand new.
    fn in_index(&self, path: &str) -> Result<bool> {
        Ok(self.inner.index()?.get_path(Path::new(path), 0).is_some())
    }

    /// Writes a commit with whatever is in the index.
    pub fn commit(&self, message: &str) -> Result<Oid> {
        let message = message.trim();
        anyhow::ensure!(!message.is_empty(), "the commit message is empty");

        let signature = self.who()?;

        let mut index = self.inner.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.inner.find_tree(tree_id)?;

        let parent = match self.inner.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();

        let oid = self
            .inner
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)?;
        Ok(oid)
    }

    /// Redoes the last commit with whatever is in the index now.
    ///
    /// It edits nothing: it writes a new commit with the same parent and moves
    /// the branch onto it. The old one is left with nobody pointing at it. So
    /// if it had already gone out to a remote, theirs and yours stop being the
    /// same history and the next push will have to be forced.
    ///
    /// The author and their date are kept: the work is still by whoever did it
    /// and from when they did it. The only thing that changes is who closes it,
    /// which is what `git commit --amend` does.
    pub fn amend(&self, message: &str) -> Result<Oid> {
        let message = message.trim();
        anyhow::ensure!(!message.is_empty(), "the commit message is empty");

        let head = self
            .inner
            .head()
            .and_then(|head| head.peel_to_commit())
            .context("there is no commit to amend")?;

        let committer = self.who()?;

        let mut index = self.inner.index()?;
        let tree = self.inner.find_tree(index.write_tree()?)?;

        let oid = head.amend(
            Some("HEAD"),
            None,
            Some(&committer),
            None,
            Some(message),
            Some(&tree),
        )?;
        Ok(oid)
    }

    /// The subject and the body of the last commit, split the way the commit
    /// box wants them.
    ///
    /// It is needed for amending: going in to change a message without seeing
    /// what it said is how you lose half of it without noticing.
    pub fn head_message(&self) -> Option<(String, String)> {
        let commit = self.inner.head().ok()?.peel_to_commit().ok()?;
        let message = commit.message()?;

        let subject = commit.summary().unwrap_or_default().to_string();
        let body = message
            .split_once("\n\n")
            .map(|(_, body)| body.trim_end().to_string())
            .unwrap_or_default();

        Some((subject, body))
    }
}

/// Walks a diff already worked out and cuts it into hunks and lines.
fn collect_file_diff(diff: &Diff<'_>, path: &str) -> Result<FileDiff> {
    let mut out = FileDiff { path: path.to_string(), ..Default::default() };

    diff.print(DiffFormat::Patch, |_delta, hunk, line| {
        match line.origin() {
            // The file header: we already know which file this is.
            'F' => {}
            'B' => out.binary = true,
            'H' => {
                let header = String::from_utf8_lossy(line.content()).trim_end().to_string();
                let id = hunk.as_ref().map(hunk_id).unwrap_or_default();
                out.hunks.push(Hunk { header, id, lines: Vec::new() });
            }
            origin => {
                // An untracked file brings no hunk header.
                if out.hunks.is_empty() {
                    let header = hunk
                        .as_ref()
                        .map(|h| String::from_utf8_lossy(h.header()).trim_end().to_string())
                        .unwrap_or_default();
                    let id = hunk.as_ref().map(hunk_id).unwrap_or_default();
                    out.hunks.push(Hunk { header, id, lines: Vec::new() });
                }
                if let Some(current) = out.hunks.last_mut() {
                    current.lines.push(DiffLine {
                        origin,
                        content: String::from_utf8_lossy(line.content())
                            .trim_end_matches(['\n', '\r'])
                            .to_string(),
                        old_lineno: line.old_lineno(),
                        new_lineno: line.new_lineno(),
                    });
                }
            }
        }
        true
    })?;

    Ok(out)
}

/// Turns libgit2's delta status into ours.
fn delta_kind(status: Delta) -> ChangeKind {
    match status {
        Delta::Added | Delta::Copied | Delta::Untracked => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Typechange => ChangeKind::TypeChange,
        Delta::Conflicted => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    }
}

/// The four ranges in a hunk's header, which identify it inside its own
/// diff.
fn hunk_id(hunk: &DiffHunk<'_>) -> HunkId {
    HunkId {
        old_start: hunk.old_start(),
        old_lines: hunk.old_lines(),
        new_start: hunk.new_start(),
        new_lines: hunk.new_lines(),
    }
}
