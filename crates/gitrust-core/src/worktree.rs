//! The same repository checked out in more than one place at once.
//!
//! A branch is a unit of work, and until now working on two of them meant
//! switching between them: one working tree, one branch at a time, and
//! whatever you had half-done had to be stashed or carried along. A worktree
//! is Git's own answer to that. The history, the objects and the references
//! are shared -- there is one repository -- but each checkout has its own
//! folder and its own HEAD.
//!
//! The rule that makes it safe is also the one that surprises people: **a
//! branch can only be checked out in one place**. Git refuses the second,
//! because two folders writing to one branch would be two different ideas of
//! what it contains. So the interesting question stops being "can I switch to
//! this branch" and becomes "where is this branch open", which is what
//! [`Repo::held_by`] answers.
//!
//! Where the folders go is a decision, not a law: Git does not care. They go
//! in a folder beside the repository, named after it, because the one place
//! they must not go is inside the repository, where every tool that walks the
//! tree would find a second copy of everything. That means writing outside the
//! folder you opened, which is worth saying out loud before doing it, so
//! [`Repo::worktree_path_for`] works the path out without creating anything.

use std::path::PathBuf;

use anyhow::{Context, Result};
use git2::{BranchType, Repository, WorktreeAddOptions, WorktreeLockStatus, WorktreePruneOptions};

use crate::repo::Repo;

/// One checkout of the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// What Git calls it: the folder's name under `.git/worktrees`.
    pub name: String,
    pub path: PathBuf,
    /// The branch open there. `None` with a detached HEAD, or when the folder
    /// is no longer there to ask.
    pub branch: Option<String>,
    /// The checkout the repository was opened from. It is in the list because
    /// it is one of them, and it is marked because it is the one that cannot
    /// be removed from in here.
    pub main: bool,
    /// Git still has the bookkeeping but the folder is gone, which is what
    /// happens when somebody deletes it by hand.
    pub missing: bool,
    /// Why it was locked, when it was. A locked worktree is somebody saying
    /// "this is on a disk that comes and goes, do not tidy it away".
    pub locked: Option<String>,
}

impl Repo {
    /// Every checkout of this repository, the one you are in first.
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let mut out = vec![self.main_worktree()];

        for name in self.inner.worktrees()?.iter().flatten() {
            let Ok(tree) = self.inner.find_worktree(name) else {
                continue;
            };
            let path = tree.path().to_path_buf();
            let missing = !path.exists();

            out.push(Worktree {
                name: name.to_string(),
                branch: match missing {
                    true => None,
                    false => head_of(&tree),
                },
                path,
                main: false,
                missing,
                locked: match tree.is_locked() {
                    Ok(WorktreeLockStatus::Locked(why)) => {
                        Some(why.unwrap_or_else(|| "no reason given".to_string()))
                    }
                    _ => None,
                },
            });
        }

        Ok(out)
    }

    /// Which checkout has this branch open, if any other one does.
    ///
    /// This is the question worth asking before offering to switch. Git's own
    /// refusal is correct and unreadable; knowing the folder means being able
    /// to say "it is open over there" and offer to go rather than to fail.
    pub fn held_by(&self, branch: &str) -> Option<Worktree> {
        self.worktrees()
            .ok()?
            .into_iter()
            .find(|tree| !tree.main && tree.branch.as_deref() == Some(branch))
    }

    /// Where the checkouts live: a folder beside the repository, named after it.
    ///
    /// Beside and not inside. Inside, every build, every search and every
    /// `git status` would walk a second copy of the whole tree, and the
    /// repository would be listing itself as untracked.
    pub fn worktree_root(&self) -> PathBuf {
        let main = self.main_workdir();
        let name = main
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "repository".to_string());

        match main.parent() {
            Some(parent) => parent.join(format!("{name}.worktrees")),
            None => main.join(".worktrees"),
        }
    }

    /// Where a branch's checkout would go, without creating anything.
    ///
    /// Said before it is done, because it is a folder outside the one that was
    /// opened and nobody should find out afterwards.
    pub fn worktree_path_for(&self, branch: &str) -> PathBuf {
        self.worktree_root().join(folder(branch))
    }

    /// Opens a branch in a checkout of its own.
    ///
    /// Everything that can be refused is refused before anything is written:
    /// the branch has to exist, it must not already be open somewhere, and the
    /// folder must be free. What is left after that is Git's to do.
    pub fn add_worktree(&self, branch: &str) -> Result<String> {
        let reference = self
            .inner
            .find_branch(branch, BranchType::Local)
            .with_context(|| format!("no branch called '{branch}'"))?
            .into_reference();

        if let Some(held) = self.held_by(branch) {
            anyhow::bail!("{branch} is already open in {}", held.path.display());
        }
        anyhow::ensure!(
            self.head_branch() != Some(branch.to_string()),
            "{branch} is the branch you are on, so it is already open here"
        );

        let name = self.free_name(branch);
        let path = self.worktree_root().join(&name);
        anyhow::ensure!(
            !path.exists(),
            "{} is already there; move it or empty it first",
            path.display()
        );

        std::fs::create_dir_all(self.worktree_root())
            .with_context(|| format!("could not make {}", self.worktree_root().display()))?;

        let mut options = WorktreeAddOptions::new();
        options.reference(Some(&reference));
        self.inner
            .worktree(&name, &path, Some(&options))
            .with_context(|| format!("could not open {branch} in {}", path.display()))?;

        Ok(format!("{branch} is open in {}", path.display()))
    }

    /// What is uncommitted in a checkout, which is what removing it throws away.
    pub fn worktree_changes(&self, name: &str) -> Result<usize> {
        let tree = self
            .inner
            .find_worktree(name)
            .with_context(|| format!("no checkout called '{name}'"))?;
        let Ok(repo) = Repository::open_from_worktree(&tree) else {
            return Ok(0);
        };

        let mut options = git2::StatusOptions::new();
        options.include_untracked(true).include_ignored(false);
        let count = repo.statuses(Some(&mut options))?.len();
        Ok(count)
    }

    /// Takes a checkout away, folder and all.
    ///
    /// It does not ask: [`Repo::worktree_changes`] is there so that whoever
    /// calls this can ask first, in the same breath as saying what is lost.
    /// The branch is left alone -- it goes back to being a branch nobody has
    /// open, which is what it was before.
    pub fn remove_worktree(&self, name: &str) -> Result<String> {
        let tree = self
            .inner
            .find_worktree(name)
            .with_context(|| format!("no checkout called '{name}'"))?;

        if let Ok(WorktreeLockStatus::Locked(why)) = tree.is_locked() {
            let why = why.unwrap_or_else(|| "no reason given".to_string());
            anyhow::bail!("{name} is locked: {why}");
        }

        let path = tree.path().to_path_buf();
        let mut options = WorktreePruneOptions::new();
        options.valid(true).working_tree(true);
        tree.prune(Some(&mut options))
            .with_context(|| format!("could not remove {}", path.display()))?;

        // libgit2 takes away what it put there, which does not include
        // anything that was written since. An empty folder left behind is
        // tidied; one with somebody's files in it is left exactly where it is.
        let _ = std::fs::remove_dir(&path);
        Ok(format!("{name} removed"))
    }

    /// Forgets the checkouts whose folders are no longer there.
    ///
    /// This is housekeeping, not deletion: everything it touches is already
    /// gone from the disk, and all that is left is Git's note that it existed.
    pub fn prune_worktrees(&self) -> Result<String> {
        let mut gone = Vec::new();

        for name in self.inner.worktrees()?.iter().flatten() {
            let Ok(tree) = self.inner.find_worktree(name) else {
                continue;
            };
            if tree.path().exists() {
                continue;
            }
            let mut options = WorktreePruneOptions::new();
            options.valid(true);
            if tree.prune(Some(&mut options)).is_ok() {
                gone.push(name.to_string());
            }
        }

        Ok(match gone.len() {
            0 => "every checkout is where it should be".to_string(),
            1 => format!("{} was gone and has been forgotten", gone[0]),
            n => format!("{n} checkouts were gone and have been forgotten"),
        })
    }

    /// The checkout this `Repo` was opened from.
    fn main_worktree(&self) -> Worktree {
        let path = self.main_workdir();
        Worktree {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "main".to_string()),
            branch: self.head_branch(),
            path,
            main: true,
            missing: false,
            locked: None,
        }
    }

    /// The folder of the checkout the repository was opened from.
    ///
    /// `commondir` is the main repository's `.git` whichever checkout you came
    /// in through, so this is the one folder every worktree agrees on.
    fn main_workdir(&self) -> PathBuf {
        let common = self.inner.commondir();
        common.parent().unwrap_or(common).to_path_buf()
    }

    /// The branch HEAD is on, or `None` when it is detached.
    fn head_branch(&self) -> Option<String> {
        let head = self.inner.head().ok()?;
        head.is_branch().then(|| head.shorthand().map(str::to_string))?
    }

    /// A name no other checkout has.
    ///
    /// Branch names have slashes in them and a checkout's name is one folder,
    /// so `fix/footer` and `fix-footer` arrive here as the same word. Counting
    /// up is not pretty, and it beats opening somebody else's checkout.
    fn free_name(&self, branch: &str) -> String {
        let wanted = folder(branch);
        let taken: Vec<String> = self
            .inner
            .worktrees()
            .map(|names| names.iter().flatten().map(str::to_string).collect())
            .unwrap_or_default();

        if !taken.contains(&wanted) {
            return wanted;
        }
        (2..)
            .map(|n| format!("{wanted}-{n}"))
            .find(|name| !taken.contains(name))
            .unwrap_or(wanted)
    }
}

/// The branch HEAD is on inside a worktree, asked of the worktree itself.
fn head_of(tree: &git2::Worktree) -> Option<String> {
    let repo = Repository::open_from_worktree(tree).ok()?;
    let head = repo.head().ok()?;
    head.is_branch().then(|| head.shorthand().map(str::to_string))?
}

/// A branch name turned into one folder name.
fn folder(branch: &str) -> String {
    let mut out = String::new();
    for ch in branch.chars() {
        match ch {
            ch if ch.is_alphanumeric() || ch == '_' || ch == '.' => out.push(ch),
            _ if !out.is_empty() && !out.ends_with('-') => out.push('-'),
            _ => {}
        }
    }

    let out = out.trim_matches(|ch| ch == '-' || ch == '.');
    match out.is_empty() {
        true => "checkout".to_string(),
        false => out.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_name_becomes_one_folder() {
        assert_eq!(folder("fix/footer"), "fix-footer");
        assert_eq!(folder("main"), "main");
        assert_eq!(folder("feature/ACME-12/retry"), "feature-ACME-12-retry");
    }

    #[test]
    fn a_folder_name_never_starts_or_ends_with_a_dot() {
        // ".." and ".git" in a folder name are the two that would be read as
        // something other than a name.
        assert_eq!(folder(".."), "checkout");
        assert_eq!(folder("../escape"), "escape");
        assert_eq!(folder(".git"), "git");
    }

    #[test]
    fn a_name_with_nothing_usable_in_it_still_gets_one() {
        assert_eq!(folder("///"), "checkout");
        assert_eq!(folder(""), "checkout");
    }
}
