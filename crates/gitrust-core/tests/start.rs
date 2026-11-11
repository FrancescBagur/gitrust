//! Making a repository and bringing one down.
//!
//! What these care about most is what they refuse. The window asking for a
//! path has one box in it, and the most likely thing to go in that box by
//! mistake is a folder already full of something else.

use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gitrust-start-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    path
}

#[test]
fn a_repository_started_in_a_folder_that_is_not_there_makes_the_folder() {
    let path = scratch("fresh");
    assert!(!path.exists());

    gitrust_core::start::init(&path, None).unwrap();

    assert!(path.join(".git").exists());
    gitrust_core::Repo::open(path.to_str().unwrap()).expect("it opens");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_repository_started_in_an_empty_folder_that_is_there_is_fine() {
    let path = scratch("empty");
    std::fs::create_dir_all(&path).unwrap();

    gitrust_core::start::init(&path, None).unwrap();

    assert!(path.join(".git").exists());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_folder_with_anything_in_it_is_refused_and_left_alone() {
    let path = scratch("occupied");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("notes.txt"), "mine\n").unwrap();

    let err = gitrust_core::start::init(&path, None).unwrap_err().to_string();
    assert!(err.contains("already has something"), "{err}");

    // Nothing of theirs was touched, and no repository was left half-made.
    assert_eq!(std::fs::read_to_string(path.join("notes.txt")).unwrap(), "mine\n");
    assert!(!path.join(".git").exists());
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn a_clone_with_no_address_says_so_before_touching_the_disk() {
    let path = scratch("no-address");

    let err = gitrust_core::start::clone("   ", &path, None).unwrap_err().to_string();
    assert!(err.contains("no address"), "{err}");
    assert!(!path.exists(), "nothing was made");
}

/// A local path is a perfectly good thing to clone from, and it is the only
/// one a test can use without a network.
#[test]
fn a_repository_cloned_from_a_folder_arrives_with_its_history() {
    let source = scratch("clone-source");
    std::fs::create_dir_all(&source).unwrap();
    gitrust_core::start::init(&source, None).unwrap();

    // Who the commit below is by. Without this the test borrows whatever
    // identity the machine happens to have in its global config, which means
    // it passes on the desk of anybody who has ever used Git and fails on a
    // fresh clone, on a build machine, and for the first person who tries
    // this repository out. The other tests get theirs from `common`; this
    // file makes its repositories by hand, because making one is what it is
    // testing.
    let made = git2::Repository::open(&source).unwrap();
    let mut config = made.config().unwrap();
    config.set_str("user.name", "gitrust test").unwrap();
    config.set_str("user.email", "test@example.invalid").unwrap();

    let repo = gitrust_core::Repo::open(source.to_str().unwrap()).unwrap();
    std::fs::write(source.join("a.txt"), "one\n").unwrap();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: one").unwrap();

    let into = scratch("clone-into");
    gitrust_core::start::clone(source.to_str().unwrap(), &into, None).unwrap();

    let there = gitrust_core::Repo::open(into.to_str().unwrap()).expect("it opens");
    assert_eq!(there.commits(50).unwrap().len(), 1);
    assert_eq!(std::fs::read_to_string(into.join("a.txt")).unwrap(), "one\n");

    let _ = std::fs::remove_dir_all(&source);
    let _ = std::fs::remove_dir_all(&into);
}
