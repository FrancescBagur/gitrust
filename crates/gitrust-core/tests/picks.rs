//! Getting out of a cherry-pick or an undo that did not come out clean.
//!
//! Both are one commit being written, so unlike a rebase they really do end
//! when you write it. What they share with a rebase is the thing worth
//! testing: the two columns are not "this branch and the other branch", and
//! the words for them have to be right or somebody takes the wrong side and
//! finds out much later.

mod common;

use common::TempRepo;
use gitrust_core::{Contents, Half, Oid, Side};

fn commit(temp: &TempRepo, file: &str, body: &str, message: &str) -> Oid {
    temp.write(file, body);
    let repo = temp.open();
    repo.stage(file).unwrap();
    repo.commit(message).unwrap();
    temp.open().head_oid().expect("there is a commit")
}

fn read(temp: &TempRepo, file: &str) -> String {
    std::fs::read_to_string(temp.path().join(file)).unwrap_or_default()
}

/// A change on another branch that cannot be taken along cleanly, left
/// standing half-taken.
fn taking_along(name: &str) -> (TempRepo, String, Oid, Oid) {
    let temp = TempRepo::new(name);
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("side").unwrap();
    let theirs = commit(&temp, "a.txt", "what they say\n", "feat: a, their way");

    temp.open().checkout_branch(&trunk).unwrap();
    let before = commit(&temp, "a.txt", "what I say\n", "feat: a, my way");

    temp.open().cherry_pick(theirs).unwrap();
    (temp, trunk, theirs, before)
}

/// A commit that cannot be undone cleanly, because of what came after it.
fn undoing(name: &str) -> (TempRepo, Oid, Oid) {
    let temp = TempRepo::new(name);
    commit(&temp, "a.txt", "one\n", "feat: one");
    let second = commit(&temp, "a.txt", "two\n", "feat: two");
    let before = commit(&temp, "a.txt", "three\n", "feat: three");

    temp.open().revert(second).unwrap();
    (temp, second, before)
}

#[test]
fn a_change_that_clashes_is_left_standing_instead_of_refused() {
    let (temp, _, theirs, before) = taking_along("pick-stands");

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::CherryPick);
    assert_eq!(repo.head_oid(), Some(before), "nothing is written yet");

    let clashes = repo.clashes().unwrap();
    assert_eq!(clashes.len(), 1);
    assert_eq!(clashes[0].path, "a.txt");

    // And it knows which commit it is in the middle of taking along.
    assert!(read(&temp, "a.txt").contains("<<<<<<<"), "the markers are in the file");
    assert_ne!(theirs, before);
}

#[test]
fn the_sides_of_a_cherry_pick_are_this_branch_and_the_one_coming_along() {
    let (temp, _, _, _) = taking_along("pick-sides");

    let clashes = temp.open().clashes().unwrap();
    assert_eq!(clashes[0].ours, Contents::Text("what I say\n".to_string()));
    assert_eq!(clashes[0].theirs, Contents::Text("what they say\n".to_string()));

    assert_eq!(
        Half::CherryPick.sides(),
        ("this branch", "the commit being taken along")
    );
    assert_eq!(
        Side::Theirs.says(Half::CherryPick),
        "what the commit being taken along says"
    );
}

#[test]
fn deciding_it_and_writing_puts_the_change_on_this_branch() {
    let (temp, trunk, theirs, before) = taking_along("pick-written");

    temp.open().take_side("a.txt", Side::Theirs).unwrap();
    temp.open().see_it_through().unwrap();

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None);
    assert_eq!(repo.head_name(), trunk);
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(read(&temp, "a.txt"), "what they say\n");

    // A new commit here, and the one it came from is exactly where it was.
    assert_ne!(repo.head_oid(), Some(before));
    assert_ne!(repo.head_oid(), Some(theirs));
    let (subject, _) = repo.head_message().unwrap();
    assert_eq!(subject, "feat: a, their way", "the message came along with it");
}

/// Deciding every file the way this branch already had it leaves nothing to
/// write. An empty commit would be a lie, and refusing would throw away a
/// real answer, so it says what happened.
#[test]
fn deciding_it_the_way_it_already_was_writes_nothing_and_says_so() {
    let (temp, _, _, before) = taking_along("pick-nothing-left");

    temp.open().take_side("a.txt", Side::Ours).unwrap();
    let note = temp.open().see_it_through().unwrap();

    assert!(note.contains("nothing was written"), "{note}");
    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None, "and it is not left half-done");
    assert_eq!(repo.head_oid(), Some(before));
    assert_eq!(read(&temp, "a.txt"), "what I say\n");
}

#[test]
fn an_undo_that_clashes_is_left_standing_instead_of_refused() {
    let (temp, _, before) = undoing("undo-stands");

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::Revert);
    assert_eq!(repo.head_oid(), Some(before));
    assert_eq!(repo.clashes().unwrap().len(), 1);

    assert_eq!(
        Half::Revert.sides(),
        ("this branch", "the commit being undone")
    );
}

#[test]
fn deciding_an_undo_and_writing_it_writes_a_commit_that_says_what_it_undid() {
    let (temp, second, before) = undoing("undo-written");

    temp.open().take_text("a.txt", "one\n").unwrap();
    temp.open().see_it_through().unwrap();

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None);
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(read(&temp, "a.txt"), "one\n");

    let (subject, body) = repo.head_message().unwrap();
    assert_eq!(subject, "revert: feat: two", "it says what it undid");
    assert!(body.contains(&second.to_string()), "and names it exactly: {body}");

    // Nothing was rewritten: what it undid is still there, with one more on top.
    let walk = repo.commits(50).unwrap();
    assert!(walk.iter().any(|c| c.id == second));
    assert!(walk.iter().any(|c| c.id == before));
    assert_eq!(walk.len(), 4);
}

#[test]
fn writing_with_a_file_still_undecided_is_refused_and_says_which() {
    let (temp, _, _, _) = taking_along("pick-not-yet");

    let err = temp.open().see_it_through().unwrap_err().to_string();
    assert!(err.contains("still to decide"), "{err}");
    assert!(err.contains("a.txt"), "{err}");
    assert_eq!(temp.open().half_done(), Half::CherryPick);
}

#[test]
fn throwing_a_half_taken_change_away_puts_the_branch_back() {
    let (temp, _, _, before) = taking_along("pick-thrown-away");

    temp.open().abandon().unwrap();

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None);
    assert_eq!(repo.head_oid(), Some(before));
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(read(&temp, "a.txt"), "what I say\n");
}

#[test]
fn nothing_new_may_start_while_a_change_is_half_taken_along() {
    let (temp, _, theirs, _) = taking_along("pick-one-at-a-time");

    for err in [
        temp.open().merge("side").unwrap_err().to_string(),
        temp.open().cherry_pick(theirs).unwrap_err().to_string(),
        temp.open().revert(theirs).unwrap_err().to_string(),
    ] {
        assert!(err.contains("half-done"), "{err}");
    }
    assert_eq!(temp.open().half_done(), Half::CherryPick);
}

/// Both of them finish by checking out over your files, so neither may start
/// with work sitting in them. It always wrote over them; now it says so.
#[test]
fn neither_one_starts_on_top_of_uncommitted_work() {
    let temp = TempRepo::new("pick-dirty");
    commit(&temp, "a.txt", "one\n", "feat: one");
    let second = commit(&temp, "b.txt", "two\n", "feat: two");

    temp.write("a.txt", "something I was in the middle of\n");

    let err = temp.open().cherry_pick(second).unwrap_err().to_string();
    assert!(err.contains("commit or stash"), "{err}");
    let err = temp.open().revert(second).unwrap_err().to_string();
    assert!(err.contains("commit or stash"), "{err}");

    assert_eq!(
        read(&temp, "a.txt"),
        "something I was in the middle of\n",
        "and it is still there"
    );
}

#[test]
fn seeing_it_through_when_nothing_is_half_done_says_so() {
    let temp = TempRepo::new("through-nothing");
    commit(&temp, "a.txt", "one\n", "feat: one");

    let err = temp.open().see_it_through().unwrap_err().to_string();
    assert!(err.contains("nothing half-done"), "{err}");
}
