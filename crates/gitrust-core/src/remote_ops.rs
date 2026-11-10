//! Mutating Git operations for a checkout reached through SSH.

use anyhow::{Context, Result};
use git2::Oid;

use crate::{PullMode, RemoteRepo, StashEntry};

impl RemoteRepo {
    pub fn branch_at(&self, name: &str, oid: Oid) -> Result<String> {
        self.git(&["branch", name, &oid.to_string()])?;
        Ok(format!("{name} created"))
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<String> {
        self.git(&["branch", "-m", old, new])?;
        Ok(format!("{old} is now {new}"))
    }

    pub fn delete_branch(&self, name: &str) -> Result<()> {
        self.git(&["branch", "-d", name]).map(|_| ())
    }

    pub fn checkout_branch(&self, name: &str) -> Result<String> {
        self.git(&["switch", name])?;
        Ok(format!("now on {name}"))
    }

    pub fn checkout_commit(&self, oid: Oid) -> Result<String> {
        self.git(&["switch", "--detach", &oid.to_string()])?;
        Ok(format!("now at {}", &oid.to_string()[..7]))
    }

    pub fn checkout_remote(&self, name: &str) -> Result<String> {
        let (_, branch) = name.split_once('/').context("a remote branch is named remote/branch")?;
        self.git(&["switch", "--track", "-c", branch, name])?;
        Ok(format!("now on {branch}, following {name}"))
    }

    pub fn fetch_all(&self) -> Result<String> {
        self.git(&["fetch", "--all", "--prune"])?;
        Ok("fetched every remote".to_string())
    }

    pub fn pull(&self, mode: PullMode) -> Result<String> {
        let mut args = vec!["pull"];
        match mode { PullMode::Merge => {}, PullMode::FastForward => args.push("--ff-only"), PullMode::Rebase => args.push("--rebase") }
        self.git(&args)?;
        Ok("pulled the current branch".to_string())
    }

    pub fn push(&self) -> Result<String> {
        self.git(&["push"])?;
        Ok("pushed the current branch".to_string())
    }

    pub fn push_branch(&self, branch: &str) -> Result<String> {
        self.git(&["push", "-u", "origin", branch])?;
        Ok(format!("pushed {branch}"))
    }

    pub fn set_upstream(&self, branch: &str, upstream: Option<&str>) -> Result<String> {
        match upstream {
            Some(upstream) => self.git(&["branch", "--set-upstream-to", upstream, branch])?,
            None => self.git(&["branch", "--unset-upstream", branch])?,
        };
        Ok(format!("tracking updated for {branch}"))
    }

    pub fn tag(&self, name: &str, target: Oid, message: &str) -> Result<()> {
        anyhow::ensure!(!name.trim().is_empty(), "the tag needs a name");
        if message.trim().is_empty() { self.git(&["tag", name, &target.to_string()])?; }
        else { self.git(&["tag", "-a", name, &target.to_string(), "-m", message.trim()])?; }
        Ok(())
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> { self.git(&["tag", "-d", name]).map(|_| ()) }

    pub fn stashes(&self) -> Result<Vec<StashEntry>> {
        let text = self.git(&["stash", "list", "--format=%gd%x1f%H%x1f%ct%x1f%gs"])?;
        Ok(text.lines().filter_map(|line| {
            let fields: Vec<_> = line.split('\x1f').collect();
            let index = fields.first()?.trim_start_matches("stash@{").trim_end_matches('}').parse().ok()?;
            Some(StashEntry { index, id: Oid::from_str(fields.get(1)?).ok()?, time: fields.get(2)?.parse().ok()?, message: fields.get(3)?.to_string() })
        }).collect())
    }

    pub fn stash_save(&self, message: &str) -> Result<String> { self.git(&["stash", "push", "-m", message])?; Ok("changes stashed".to_string()) }
    pub fn stash_pop(&self, index: usize) -> Result<String> { self.git(&["stash", "pop", &format!("stash@{{{index}}}")])?; Ok("changes restored".to_string()) }
    pub fn stash_apply(&self, index: usize) -> Result<String> { self.git(&["stash", "apply", &format!("stash@{{{index}}}")])?; Ok("changes applied, and still stashed".to_string()) }
    pub fn stash_drop(&self, index: usize) -> Result<String> { self.git(&["stash", "drop", &format!("stash@{{{index}}}")])?; Ok("stash dropped".to_string()) }

    pub fn merge(&self, name: &str) -> Result<String> {
        self.git(&["merge", name])?;
        Ok(format!("merged {name}"))
    }

    pub fn rebase_onto(&self, name: &str) -> Result<String> {
        self.git(&["rebase", name])?;
        Ok(format!("rebased onto {name}"))
    }

    pub fn cherry_pick(&self, oid: Oid) -> Result<String> {
        self.git(&["cherry-pick", &oid.to_string()])?;
        Ok(format!("took {} along", &oid.to_string()[..7]))
    }

    pub fn revert(&self, oid: Oid) -> Result<String> {
        self.git(&["revert", "--no-edit", &oid.to_string()])?;
        Ok(format!("undid {}", &oid.to_string()[..7]))
    }
}
