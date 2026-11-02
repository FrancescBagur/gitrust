//! Taking a commit back, taking one along, and moving a branch.
//!
//! Every one of these can lose work, so what the tests care about most is
//! what happens when they refuse: that a clash leaves the repository exactly
//! as it was, with nothing half-applied and nothing checked out.

mod common;

use common::TempRepo;
use gitrust_core::{Move, Oid, Repo};

fn commit(temp: &TempRepo, file: &str, body: &str, message: &str) -> Oid {
    temp.write(file, body);
    let repo = temp.open();
    repo.stage(file).unwrap();
    repo.commit(message).unwrap();
    tip(&temp.open())
}

fn tip(repo: &Repo) -> Oid {
    repo.head_oid().expect("there is a commit")
}

fn start(temp: &TempRepo) -> String {
    commit(temp, "a.txt", "one\n", "feat: one");
    temp.open().head_name()
}

fn read(temp: &TempRepo, file: &str) -> String {
    std::fs::read_to_string(temp.path().join(file)).unwrap_or_default()
}

#[test]
fn a_change_taken_along_is_written_again_where_you_are() {
    let temp = TempRepo::new("cherry-pick");
    let trunk = start(&temp);

    temp.open().branch_here("side").unwrap();
    let side = commit(&temp, "b.txt", "two\n", "feat: two");
    temp.open().checkout_branch(&trunk).unwrap();
    // The trunk has to have moved. Writing the same change onto the same
    // parent with the same author gives back the same commit, byte for byte,
    // and then there is nothing to tell apart.
    commit(&temp, "c.txt", "three\n", "feat: three");

    assert_eq!(read(&temp, "b.txt"), "");
    temp.open().cherry_pick(side).unwrap();

    assert_eq!(read(&temp, "b.txt"), "two\n");
    // The commit it came from is still exactly where it was.
    assert_ne!(tip(&temp.open()), side);
    assert!(temp.open().commits(50).unwrap().iter().any(|c| c.id == side));
}

#[test]
fn taking_along_something_you_already_have_is_refused_rather_than_repeated() {
    let temp = TempRepo::new("cherry-pick-have-it");
    start(&temp);
    let first = commit(&temp, "b.txt", "two\n", "feat: two");

    let err = temp.open().cherry_pick(first).unwrap_err().to_string();
    assert!(err.contains("already has"), "{err}");
}

/// The repository is left exactly as it was. That is the whole point: there
/// is no conflict editor to get anybody out of a half-applied pick.
#[test]
fn a_change_that_clashes_is_not_half_taken_along() {
    let temp = TempRepo::new("cherry-pick-clash");
    let trunk = start(&temp);

    temp.open().branch_here("side").unwrap();
    let side = commit(&temp, "a.txt", "theirs\n", "feat: theirs");
    temp.open().checkout_branch(&trunk).unwrap();
    let before = commit(&temp, "a.txt", "ours\n", "feat: ours");

    let err = temp.open().cherry_pick(side).unwrap_err().to_string();
    assert!(err.contains("clashes"), "{err}");
    assert_eq!(tip(&temp.open()), before);
    assert_eq!(read(&temp, "a.txt"), "ours\n");
    assert!(temp.open().status().unwrap().is_clean());
}

#[test]
fn undoing_a_commit_leaves_it_where_it_is_and_writes_another() {
    let temp = TempRepo::new("revert");
    start(&temp);
    let added = commit(&temp, "b.txt", "two\n", "feat: two");
    assert_eq!(read(&temp, "b.txt"), "two\n");

    temp.open().revert(added).unwrap();

    assert_eq!(read(&temp, "b.txt"), "");
    // Nothing was rewritten: the commit being undone is still there, and the
    // branch has one more commit than before rather than one fewer.
    let walk = temp.open().commits(50).unwrap();
    assert!(walk.iter().any(|c| c.id == added));
    assert_eq!(walk.len(), 3);
}

#[test]
fn a_merge_is_refused_by_both_of_them_rather_than_guessed_at() {
    let temp = TempRepo::new("history-merge");
    let trunk = start(&temp);

    temp.open().branch_here("side").unwrap();
    commit(&temp, "b.txt", "two\n", "feat: two");
    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "c.txt", "three\n", "feat: three");
    temp.open().merge("side").unwrap();
    let merge = tip(&temp.open());

    for err in [
        temp.open().cherry_pick(merge).unwrap_err().to_string(),
        temp.open().revert(merge).unwrap_err().to_string(),
    ] {
        assert!(err.contains("merge"), "{err}");
    }
}

#[test]
fn moving_a_branch_back_keeping_everything_leaves_the_files_alone() {
    let temp = TempRepo::new("move-keep");
    start(&temp);
    let first = tip(&temp.open());
    commit(&temp, "b.txt", "two\n", "feat: two");

    temp.open().move_branch(first, Move::Keep).unwrap();

    assert_eq!(tip(&temp.open()), first);
    // The file the commit held is still on disk, and still staged.
    assert_eq!(read(&temp, "b.txt"), "two\n");
    assert!(!temp.open().status().unwrap().staged.is_empty());
}

#[test]
fn moving_a_branch_back_and_unstaging_leaves_the_files_but_not_the_index() {
    let temp = TempRepo::new("move-unstage");
    start(&temp);
    let first = tip(&temp.open());
    commit(&temp, "b.txt", "two\n", "feat: two");

    temp.open().move_branch(first, Move::Unstage).unwrap();

    assert_eq!(tip(&temp.open()), first);
    assert_eq!(read(&temp, "b.txt"), "two\n");
    assert!(temp.open().status().unwrap().staged.is_empty());
}

/// The one operation in gitrust that destroys work nothing can give back.
#[test]
fn moving_a_branch_back_and_discarding_takes_the_files_with_it() {
    let temp = TempRepo::new("move-discard");
    start(&temp);
    let first = tip(&temp.open());
    commit(&temp, "b.txt", "two\n", "feat: two");

    temp.open().move_branch(first, Move::Discard).unwrap();

    assert_eq!(tip(&temp.open()), first);
    assert_eq!(read(&temp, "b.txt"), "");
    assert!(temp.open().status().unwrap().is_clean());
}

#[test]
fn moving_a_branch_to_where_it_already_is_says_so_and_does_nothing() {
    let temp = TempRepo::new("move-nowhere");
    start(&temp);
    let here = tip(&temp.open());

    let said = temp.open().move_branch(here, Move::Discard).unwrap();
    assert!(said.contains("already"), "{said}");
    assert_eq!(tip(&temp.open()), here);
}

#[test]
fn a_branch_moved_while_you_are_on_no_branch_is_refused() {
    let temp = TempRepo::new("move-detached");
    start(&temp);
    let first = tip(&temp.open());
    commit(&temp, "b.txt", "two\n", "feat: two");

    temp.open().checkout_commit(first).unwrap();
    let err = temp.open().move_branch(first, Move::Keep).unwrap_err().to_string();
    assert!(err.contains("not on a branch"), "{err}");
}

#[test]
fn going_to_a_commit_puts_you_on_no_branch_with_its_files() {
    let temp = TempRepo::new("checkout-commit");
    start(&temp);
    let first = tip(&temp.open());
    commit(&temp, "b.txt", "two\n", "feat: two");

    temp.open().checkout_commit(first).unwrap();

    assert_eq!(read(&temp, "b.txt"), "");
    assert_eq!(temp.open().head_name(), "detached HEAD");
}

#[test]
fn a_branch_made_at_a_commit_does_not_move_the_one_you_are_on() {
    let temp = TempRepo::new("branch-at");
    let trunk = start(&temp);
    let first = tip(&temp.open());
    let second = commit(&temp, "b.txt", "two\n", "feat: two");

    temp.open().branch_at("older", first).unwrap();

    assert_eq!(temp.open().head_name(), trunk);
    assert_eq!(tip(&temp.open()), second);
    assert_eq!(read(&temp, "b.txt"), "two\n");
}

#[test]
fn a_branch_renamed_while_you_are_on_it_takes_you_with_it() {
    let temp = TempRepo::new("rename-current");
    let trunk = start(&temp);

    temp.open().rename_branch(&trunk, "renamed").unwrap();

    assert_eq!(temp.open().head_name(), "renamed");
}

#[test]
fn a_name_that_is_taken_is_refused_and_nothing_is_renamed() {
    let temp = TempRepo::new("rename-taken");
    let trunk = start(&temp);
    temp.open().branch_at("other", tip(&temp.open())).unwrap();

    let err = temp.open().rename_branch(&trunk, "other").unwrap_err().to_string();
    assert!(err.contains("already"), "{err}");
    assert_eq!(temp.open().head_name(), trunk);
}

#[test]
fn moving_up_is_refused_when_there_would_be_a_merge_to_write() {
    let temp = TempRepo::new("ff-refused");
    let trunk = start(&temp);

    temp.open().branch_here("side").unwrap();
    commit(&temp, "b.txt", "two\n", "feat: two");
    temp.open().checkout_branch(&trunk).unwrap();
    let ours = commit(&temp, "c.txt", "three\n", "feat: three");

    let err = temp.open().fast_forward_to("side").unwrap_err().to_string();
    assert!(err.contains("merge"), "{err}");
    assert_eq!(tip(&temp.open()), ours);
}

#[test]
fn moving_up_when_there_is_room_moves_without_writing_a_commit() {
    let temp = TempRepo::new("ff-done");
    let trunk = start(&temp);

    temp.open().branch_here("side").unwrap();
    let ahead = commit(&temp, "b.txt", "two\n", "feat: two");
    temp.open().checkout_branch(&trunk).unwrap();

    temp.open().fast_forward_to("side").unwrap();

    assert_eq!(tip(&temp.open()), ahead);
    assert_eq!(temp.open().commits(50).unwrap().len(), 2);
}
