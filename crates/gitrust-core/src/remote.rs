//! Talking to the remote: fetching, pushing, and knowing what each side owes.
//!
//! All three operations give back the sentence to show when they are done,
//! instead of a pile of data the interface would have to write up. What counts
//! about a `pull` is not how many objects came down, but whether the branch
//! moved and where to.

use anyhow::{Context, Result};
use std::collections::HashMap;

use git2::{
    build::CheckoutBuilder, BranchType, Config, Cred, CredentialType, Direction, FetchOptions,
    ObjectType, Oid, PushOptions, Remote, RemoteCallbacks,
};

use crate::repo::Repo;

/// What to do with what comes from the remote when moving up is not enough.
///
/// They are the same three `git pull` offers, and the usual one is the first:
/// move up if you can and, if not, join the two histories with a commit that
/// says so. The other two exist because some people want neither of those
/// things, and that is a fair preference, not a detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    /// Moves up if it can; if not, writes a merge commit.
    Merge,
    /// Only moves up. With the histories diverged, it stops and says so.
    FastForward,
    /// Replays your commits on top of what comes, leaving the history straight.
    Rebase,
}

/// How far the current branch has drifted from the one it tracks on the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracking {
    /// The remote branch's short name: "origin/main".
    pub remote: String,
    /// Commits that are here and not there.
    pub ahead: usize,
    /// Commits that are there and not here.
    pub behind: usize,
}

impl Repo {
    /// The remote branch the current one tracks, and the distance to it.
    ///
    /// It is `None` when the branch tracks nothing, which is the normal state
    /// until the first `push`. It does not tell that case from a failure to
    /// read: the only thing this is for is painting two counts, and if they
    /// cannot be known they are not painted.
    pub fn tracking(&self) -> Option<Tracking> {
        let head = self.inner.head().ok()?;
        let upstream = self.inner.branch_upstream_name(head.name()?).ok()?;
        let upstream = upstream.as_str()?;

        let ours = head.peel(ObjectType::Commit).ok()?.id();
        let theirs = self
            .inner
            .find_reference(upstream)
            .ok()?
            .peel(ObjectType::Commit)
            .ok()?
            .id();

        let (ahead, behind) = self.inner.graph_ahead_behind(ours, theirs).ok()?;
        Some(Tracking {
            remote: upstream.strip_prefix("refs/remotes/").unwrap_or(upstream).to_string(),
            ahead,
            behind,
        })
    }

    /// Brings down whatever is on the remote without touching the working tree.
    /// Says which remote branch a local one follows, or that it follows none.
    ///
    /// Following is what makes the distance above the graph mean anything:
    /// without it there is no "two ahead" because there is nothing to be
    /// ahead of.
    pub fn set_upstream(&self, branch: &str, upstream: Option<&str>) -> Result<String> {
        let mut local = self
            .inner
            .find_branch(branch, git2::BranchType::Local)
            .with_context(|| format!("no branch called '{branch}'"))?;

        local
            .set_upstream(upstream)
            .with_context(|| match upstream {
                Some(name) => format!("could not make {branch} follow {name}"),
                None => format!("could not stop {branch} following anything"),
            })?;

        Ok(match upstream {
            Some(name) => format!("{branch} follows {name} now"),
            None => format!("{branch} follows nothing now"),
        })
    }

    pub fn fetch(&self) -> Result<String> {
        let (remote, _) = self.upstream()?;
        self.fetch_from(&remote)?;

        match self.tracking() {
            Some(t) if t.behind > 0 => Ok(format!("{} to pull from {}", commits(t.behind), t.remote)),
            _ => Ok(format!("{remote} has nothing new")),
        }
    }

    /// Fetches from every remote, whether the current branch tracks one or not.
    pub fn fetch_all(&self) -> Result<String> {
        let remotes: Vec<String> =
            self.inner.remotes()?.iter().flatten().map(str::to_string).collect();
        anyhow::ensure!(!remotes.is_empty(), "this repository has no remotes");

        for remote in &remotes {
            self.fetch_from(remote)?;
        }

        match self.tracking() {
            Some(t) if t.behind > 0 => Ok(format!("{} to pull from {}", commits(t.behind), t.remote)),
            _ if remotes.len() == 1 => Ok(format!("{} has nothing new", remotes[0])),
            _ => Ok(format!("fetched from {} remotes", remotes.len())),
        }
    }

    /// Fetches from the remote and joins what comes with what is here, as asked.
    ///
    /// The fetching is the same in all three modes; what changes is what is done
    /// when the two histories have parted. Moving up, when it can be done, is
    /// always done: there is nothing to join if one is ahead of the other.
    pub fn pull(&self, mode: PullMode) -> Result<String> {
        let (remote, branch) = self.upstream()?;
        self.fetch_from(&remote)?;
        let upstream = format!("{remote}/{branch}");

        match mode {
            PullMode::Merge => self.merge(&upstream),
            PullMode::FastForward => self.fast_forward(&remote, &branch),
            PullMode::Rebase => self.rebase_onto(&upstream),
        }
    }

    /// Moves the branch up, but only if it can be done without merging anything.
    fn fast_forward(&self, remote: &str, branch: &str) -> Result<String> {
        let head = self.inner.head()?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let upstream = self.inner.branch_upstream_name(&refname)?;
        let upstream = upstream.as_str().context("the remote has no name")?;

        let theirs = self.inner.find_reference(upstream)?;
        let theirs = self.inner.reference_to_annotated_commit(&theirs)?;
        let (analysis, _) = self.inner.merge_analysis(&[&theirs])?;

        if analysis.is_up_to_date() {
            return Ok(format!("{branch} was already up to date"));
        }
        anyhow::ensure!(
            analysis.is_fast_forward(),
            "{branch} and {remote}/{branch} have diverged: pull with merge or rebase instead"
        );

        let behind = self.tracking().map(|t| t.behind).unwrap_or(0);

        // The order matters: the tree first, the reference after. If the tree
        // fails -- because unsaved changes are in the way -- the branch stays
        // where it was and there is nothing to undo.
        let commit = self.inner.find_commit(theirs.id())?;
        self.inner
            .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
            .context("unsaved changes are in the way; commit them or stash them")?;
        self.inner
            .find_reference(&refname)?
            .set_target(theirs.id(), "pull: fast-forward")?;

        Ok(format!("{} from {remote}/{branch}", commits(behind)))
    }

    /// Pushes the current branch, and leaves it tracking the remote if it was not.
    pub fn push(&self) -> Result<String> {
        let head = self.inner.head()?;
        let branch = head
            .shorthand()
            .filter(|_| head.is_branch())
            .context("HEAD is detached: there is no branch to push")?
            .to_string();

        self.push_branch(&branch)
    }

    /// The same, but for the branch named outright.
    ///
    /// It is needed to push a branch you are not on, which is what dragging its
    /// card on the board asks for.
    pub fn push_branch(&self, branch: &str) -> Result<String> {
        let refname = format!("refs/heads/{branch}");
        anyhow::ensure!(
            self.inner.find_reference(&refname).is_ok(),
            "no branch called '{branch}'"
        );

        // With no remote configured it tries "origin", which is what it is
        // called in the vast majority of repositories.
        let remote_name = self.remote_of(&refname);

        // This branch's distance, not HEAD's. A card is pushed from the board
        // while you are standing wherever you were standing, so asking
        // `tracking` would count whatever branch that is -- and from a trunk
        // that is in step with its remote, every push reported no commits at
        // all while quietly pushing them.
        let ahead = self.ahead_of(&refname);
        let mut remote = self
            .inner
            .find_remote(&remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        // The server's refusal does not arrive as an error from `push`: it
        // arrives here, and without catching it a rejected push would look fine.
        let refused = std::cell::RefCell::new(None);
        {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(credentials(self.inner.config()?, self.profile.clone()));
            callbacks.push_update_reference(|_, status| {
                if let Some(reason) = status {
                    *refused.borrow_mut() = Some(reason.to_string());
                }
                Ok(())
            });

            let mut options = PushOptions::new();
            options.remote_callbacks(callbacks);
            remote
                .push(&[format!("{refname}:{refname}")], Some(&mut options))
                .with_context(|| format!("could not push {branch} to {remote_name}"))?;
        }

        if let Some(reason) = refused.into_inner() {
            anyhow::bail!("{remote_name} rejected {branch}: {reason}");
        }

        // A brand-new branch tracks nothing: after the first push there is
        // something to track, and doing it now saves the `--set-upstream`.
        if self.inner.branch_upstream_name(&refname).is_err() {
            let mut local = self.inner.find_branch(branch, git2::BranchType::Local)?;
            let _ = local.set_upstream(Some(&format!("{remote_name}/{branch}")));
        }

        Ok(match ahead {
            Some(count) => format!("{} to {remote_name}/{branch}", commits(count)),
            // Nothing was being followed, so there was no distance to measure:
            // a count here would be made up, and "0 commits" reads as a push
            // that did nothing.
            None => format!("{branch} is on {remote_name} now"),
        })
    }

    /// How far a branch is ahead of whatever it follows.
    ///
    /// `None` when it follows nothing, which is every branch's first push.
    fn ahead_of(&self, refname: &str) -> Option<usize> {
        let upstream = self.inner.branch_upstream_name(refname).ok()?;
        let theirs = self
            .inner
            .find_reference(upstream.as_str()?)
            .ok()?
            .peel(ObjectType::Commit)
            .ok()?
            .id();
        let ours = self
            .inner
            .find_reference(refname)
            .ok()?
            .peel(ObjectType::Commit)
            .ok()?
            .id();
        self.inner.graph_ahead_behind(ours, theirs).ok().map(|(ahead, _)| ahead)
    }

    /// Pushes the branch over whatever is there, but only if what is there is
    /// still what you last saw.
    ///
    /// That is the "with lease" of `git push --force-with-lease`, and it is what
    /// separates redoing your own history from wiping out someone else's work
    /// without anyone noticing. The remote is asked before anything is written:
    /// if it has changed since your last visit, nothing is forced and it says why.
    pub fn push_force(&self) -> Result<String> {
        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let branch = refname
            .strip_prefix("refs/heads/")
            .context("HEAD is detached: there is no branch to push")?
            .to_string();

        let remote_name = self.remote_of(&refname);
        let mut remote = self
            .inner
            .find_remote(&remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        // What we saw the last time we fetched.
        let seen = self
            .inner
            .find_branch(&format!("{remote_name}/{branch}"), BranchType::Remote)
            .ok()
            .and_then(|branch| branch.get().target());

        let theirs = self.heads_of(&mut remote)?.get(&refname).copied();
        anyhow::ensure!(
            theirs == seen,
            "{remote_name}/{branch} has changed since you last fetched: \
             fetch and look at what is there before overwriting it"
        );

        // The "+" in front of the refspec is what allows not moving up.
        self.send(&mut remote, &[format!("+{refname}:{refname}")])?;
        Ok(format!("{branch} forced onto {remote_name}"))
    }

    /// Sends the remote the tags it does not have.
    ///
    /// The ones already there are not sent again, and one that has moved stays
    /// here: the remote turns it down, and that is right, because a tag that
    /// moves stops being good for the one thing it is for.
    pub fn push_tags(&self) -> Result<String> {
        let remote_name = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.name().map(|name| self.remote_of(name)))
            .unwrap_or_else(|| "origin".to_string());

        let mut remote = self
            .inner
            .find_remote(&remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        let theirs = self.heads_of(&mut remote)?;
        let mut missing = Vec::new();

        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(name) = reference.name() else {
                continue;
            };
            if !name.starts_with("refs/tags/") {
                continue;
            }
            if theirs.contains_key(name) {
                continue;
            }
            missing.push(format!("{name}:{name}"));
        }

        if missing.is_empty() {
            return Ok(format!("{remote_name} already has every tag"));
        }

        let count = missing.len();
        self.send(&mut remote, &missing)?;
        Ok(format!(
            "{count} {} to {remote_name}",
            if count == 1 { "tag sent" } else { "tags sent" }
        ))
    }

    /// What references the remote has right now.
    fn heads_of(&self, remote: &mut Remote<'_>) -> Result<HashMap<String, Oid>> {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(credentials(self.inner.config()?, self.profile.clone()));

        let connection = remote
            .connect_auth(Direction::Push, Some(callbacks), None)
            .context("could not reach the remote")?;

        let heads = connection
            .list()?
            .iter()
            .map(|head| (head.name().to_string(), head.oid()))
            .collect();

        Ok(heads)
    }

    /// Sends some refspecs and finds out whether the server says no.
    pub(crate) fn send(&self, remote: &mut Remote<'_>, refspecs: &[String]) -> Result<()> {
        let refused = std::cell::RefCell::new(None);
        {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(credentials(self.inner.config()?, self.profile.clone()));
            callbacks.push_update_reference(|name, status| {
                if let Some(reason) = status {
                    *refused.borrow_mut() = Some(format!("{name}: {reason}"));
                }
                Ok(())
            });

            let mut options = PushOptions::new();
            options.remote_callbacks(callbacks);
            remote.push(refspecs, Some(&mut options))?;
        }

        match refused.into_inner() {
            Some(reason) => anyhow::bail!("the remote refused — {reason}"),
            None => Ok(()),
        }
    }

    /// A branch's remote, or "origin", which is what it is called nearly always.
    fn remote_of(&self, refname: &str) -> String {
        self.inner
            .branch_upstream_remote(refname)
            .ok()
            .and_then(|buf| buf.as_str().map(str::to_string))
            .unwrap_or_else(|| "origin".to_string())
    }

    /// Deletes a branch on the remote, and its trace here along with it.
    ///
    /// A refspec with nothing on the left is how you tell Git the reference on
    /// the other side is surplus. It is the same as `git push origin :branch`.
    ///
    /// This is seen from every machine, not just this one.
    pub fn delete_remote_branch(&self, name: &str) -> Result<String> {
        let (remote_name, branch) = name
            .split_once('/')
            .context("a remote branch is named remote/branch")?;

        let mut remote = self
            .inner
            .find_remote(remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        let refused = std::cell::RefCell::new(None);
        {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(credentials(self.inner.config()?, self.profile.clone()));
            callbacks.push_update_reference(|_, status| {
                if let Some(reason) = status {
                    *refused.borrow_mut() = Some(reason.to_string());
                }
                Ok(())
            });

            let mut options = PushOptions::new();
            options.remote_callbacks(callbacks);
            remote
                .push(&[format!(":refs/heads/{branch}")], Some(&mut options))
                .with_context(|| format!("could not delete {branch} on {remote_name}"))?;
        }

        if let Some(reason) = refused.into_inner() {
            anyhow::bail!("{remote_name} refused: {reason}");
        }

        // And the local copy of that remote branch, which otherwise is left
        // pointing at something that is no longer there.
        if let Ok(mut local) = self.inner.find_branch(name, git2::BranchType::Remote) {
            let _ = local.delete();
        }

        Ok(format!("{name} deleted on the remote"))
    }

    /// The remote the current branch looks at, and its name.
    fn upstream(&self) -> Result<(String, String)> {
        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?;
        let branch = refname
            .strip_prefix("refs/heads/")
            .context("HEAD is detached: there is no branch to track")?
            .to_string();

        let remote = self
            .inner
            .branch_upstream_remote(refname)
            .ok()
            .and_then(|buf| buf.as_str().map(str::to_string))
            .with_context(|| format!("{branch} does not track any remote branch"))?;

        Ok((remote, branch))
    }

    /// Brings down the branches, and the backlog along with them.
    ///
    /// The remote's own refspecs are asked for by name rather than left to
    /// default, because one more has to go with them: the backlog lives
    /// outside the branch namespace, so nothing configured would ever bring it
    /// down. It costs no second round trip -- it is the same fetch.
    pub(crate) fn fetch_from(&self, name: &str) -> Result<()> {
        let mut remote = self
            .inner
            .find_remote(name)
            .with_context(|| format!("no remote called '{name}'"))?;

        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(credentials(self.inner.config()?, self.profile.clone()));

        let mut options = FetchOptions::new();
        options.remote_callbacks(callbacks);

        let mut refspecs: Vec<String> =
            remote.fetch_refspecs()?.iter().flatten().map(str::to_string).collect();
        refspecs.push(self.backlog_refspec(name));

        remote
            .fetch(&refspecs, Some(&mut options), None)
            .context("could not fetch from the remote")?;

        // A backlog that will not join is not a reason for the pull to fail:
        // the branches are down and that is what was asked for. The column
        // goes on saying the two sides differ, which is where it belongs.
        let _ = self.absorb_backlog(name);
        Ok(())
    }
}

/// The credentials, in the order `git` would try them, with the profile's own
/// ahead of each: its key before the agent, its token for the host before the
/// credential helper, and then the bare username for the SSH remotes that ask
/// for one before anything else.
///
/// The profile goes first and the usual route stays behind it, so a key that
/// the server turns down -- the wrong client's key, on the right host -- falls
/// through to the agent instead of ending the push. A profile that names
/// neither key nor token changes nothing at all here.
///
/// Each route is tried once only. libgit2 calls back for as long as it is not
/// given something that works, so without keeping count this would go round
/// for ever against a server that says no.
pub(crate) fn credentials(
    config: Config,
    profile: Option<crate::Profile>,
) -> impl FnMut(&str, Option<&str>, CredentialType) -> Result<Cred, git2::Error> {
    let mut used = CredentialType::empty();
    let mut key_tried = false;
    let mut token_tried = false;

    move |url, username, allowed| {
        let user = username.unwrap_or("git");

        // The profile's key, before the agent and only once. libgit2 wants
        // the public half beside it when there is one; for the key types in
        // use today it can work it out from the private half, so a missing
        // `.pub` is not a missing key.
        if allowed.contains(CredentialType::SSH_KEY) && !key_tried {
            key_tried = true;
            if let Some(key) = profile.as_ref().and_then(crate::Profile::key_file) {
                let public = key.with_extension("pub");
                let public = public.exists().then_some(public);
                return Cred::ssh_key(user, public.as_deref(), &key, None);
            }
        }

        // The profile's token for this host, before the helper and only
        // once. An account with no token is an account whose forge tool is
        // logged in on its own; there is nothing to hand over here.
        if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) && !token_tried {
            token_tried = true;
            if let Some(account) = profile.as_ref().and_then(|profile| profile.account_for(url)) {
                if !account.token.is_empty() {
                    // With a token as the password the name beside it is not
                    // checked by either forge, but it cannot be empty.
                    let who = match account.user.trim().is_empty() {
                        true => "oauth2",
                        false => account.user.trim(),
                    };
                    return Cred::userpass_plaintext(who, &account.token);
                }
            }
        }

        let mut once = |kind: CredentialType| {
            let first = allowed.contains(kind) && !used.contains(kind);
            if first {
                used |= kind;
            }
            first
        };

        if once(CredentialType::SSH_KEY) {
            return Cred::ssh_key_from_agent(user);
        }
        if once(CredentialType::USER_PASS_PLAINTEXT) {
            return Cred::credential_helper(&config, url, username);
        }
        if once(CredentialType::USERNAME) {
            return Cred::username(user);
        }
        if once(CredentialType::DEFAULT) {
            return Cred::default();
        }

        Err(git2::Error::from_str(
            "no credentials for this remote: load your key into the SSH agent, \
             or give this profile the key and the token for it",
        ))
    }
}

/// "1 commit", "4 commits".
fn commits(count: usize) -> String {
    if count == 1 {
        "1 commit".to_string()
    } else {
        format!("{count} commits")
    }
}
