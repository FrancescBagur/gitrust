//! Mutating Git operations for a checkout reached through SSH.

use anyhow::{Context, Result};
use git2::Oid;

use crate::{PullMode, RemoteRepo};

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
