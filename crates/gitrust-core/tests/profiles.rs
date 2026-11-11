//! Working under a profile, where it shows: on the commits.
//!
//! The key and the token cannot be tested here. Both are only ever handed to
//! libgit2 as it talks to a server, and a test that proved anything about
//! them would need a server that asks for them. What *can* be pinned down is
//! the part that writes into the repository, and that is this: whose name
//! ends up on a commit, and that a profile which says nothing about a name
//! changes nothing about it.

mod common;

use common::TempRepo;
use gitrust_core::{Profile, Repo};

/// The repository's own configuration says this, as every test repository
/// here does. A profile either overrides it or it does not.
const CONFIGURED: (&str, &str) = ("gitrust test", "test@example.invalid");

fn acme() -> Profile {
    Profile {
        author: "Someone At Acme".to_string(),
        email: "someone@acme.example".to_string(),
        ..Profile::new("Acme Ltd")
    }
}

fn commit(repo: &Repo, name: &str, body: &str) -> (String, String) {
    std::fs::write(repo.workdir().join(name), body).unwrap();
    repo.stage(name).unwrap();
    let oid = repo.commit(&format!("feat: {name}")).unwrap();

    let detail = repo.commit_detail(oid).unwrap();
    (detail.author, detail.email)
}

#[test]
fn with_no_profile_the_commits_are_signed_as_git_is_configured() {
    let temp = TempRepo::new("profile-none");
    let repo = temp.open();

    let (author, email) = commit(&repo, "a.txt", "one\n");
    assert_eq!(author, CONFIGURED.0);
    assert_eq!(email, CONFIGURED.1);
}

#[test]
fn a_profile_puts_its_own_name_on_the_commits() {
    let temp = TempRepo::new("profile-name");
    let repo = temp.open().using(Some(acme()));

    let (author, email) = commit(&repo, "a.txt", "one\n");
    assert_eq!(author, "Someone At Acme");
    assert_eq!(email, "someone@acme.example");
}

/// The point of this one: a profile made for a key alone, or for a token
/// alone, must not quietly start rewriting the name on the history.
#[test]
fn a_profile_with_no_name_in_it_leaves_the_commits_as_they_were() {
    let temp = TempRepo::new("profile-keyonly");
    let keyed = Profile {
        key: "~/.ssh/acme_ed25519".to_string(),
        ..Profile::new("Acme Ltd")
    };
    let repo = temp.open().using(Some(keyed));

    let (author, email) = commit(&repo, "a.txt", "one\n");
    assert_eq!(author, CONFIGURED.0);
    assert_eq!(email, CONFIGURED.1);
}

/// Half an identity is no identity: Git itself will not write a commit with
/// a name and no address, and the right moment to fall back is before the
/// commit, not at it.
#[test]
fn half_a_name_is_not_enough_to_take_over_a_commit() {
    let temp = TempRepo::new("profile-half");
    let half = Profile {
        author: "Someone At Acme".to_string(),
        ..Profile::new("Acme Ltd")
    };
    let repo = temp.open().using(Some(half));

    let (author, email) = commit(&repo, "a.txt", "one\n");
    assert_eq!(author, CONFIGURED.0);
    assert_eq!(email, CONFIGURED.1);
}

/// Amending keeps the author and their date and changes only the committer,
/// which is `git commit --amend`'s own behaviour. Under a profile the
/// committer is the profile; the original author stays where it was.
#[test]
fn amending_under_a_profile_leaves_the_original_author_alone() {
    let temp = TempRepo::new("profile-amend");
    let repo = temp.open();
    commit(&repo, "a.txt", "one\n");

    let under = temp.open().using(Some(acme()));
    std::fs::write(temp.path().join("a.txt"), "two\n").unwrap();
    under.stage("a.txt").unwrap();
    let oid = under.amend("feat: one, said better").unwrap();

    let detail = under.commit_detail(oid).unwrap();
    assert_eq!(detail.author, CONFIGURED.0, "the author is who wrote it");
    assert_eq!(detail.email, CONFIGURED.1);
}

/// The work the board does under a profile -- the backlog lives in a
/// reference of its own and is written with a signature like anything else.
#[test]
fn the_backlog_is_written_under_the_profile_too() {
    let temp = TempRepo::new("profile-backlog");
    let repo = temp.open();
    commit(&repo, "a.txt", "one\n");

    let under = temp.open().using(Some(acme()));
    under.backlog_add("the footer rides over the content", None).unwrap();

    // Read with libgit2 outright: what is being checked is the signature on
    // a commit the crate does not hand back in any shape of its own.
    let raw = git2::Repository::open(temp.path()).unwrap();
    let head = raw
        .find_reference("refs/gitrust/backlog")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    assert_eq!(head.author().name().unwrap(), "Someone At Acme");
}
