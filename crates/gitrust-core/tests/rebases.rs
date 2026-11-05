//! Getting out of a rebase that did not come out clean.
//!
//! A merge is one clash with one answer. A rebase is one clash per commit,
//! and almost everything that is easy to get wrong about it comes from that:
//! deciding is not finishing, the two sides are the other way round from a
//! merge, and throwing it away has to put back commits that were already
//! replayed as well as files that were already written.
//!
//! So these tests stop a rebase on purpose, over and over, and check what is
//! true in between.

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

/// Two commits on a branch, the first of which clashes with the trunk and the
/// second of which does not. Left standing in the middle of the rebase.
fn one_clash(name: &str) -> (TempRepo, String, Oid) {
    let temp = TempRepo::new(name);
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("side").unwrap();
    commit(&temp, "a.txt", "side one\n", "feat: a, my way");
    let tip = commit(&temp, "b.txt", "side two\n", "feat: b");

    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "a.txt", "trunk one\n", "feat: a, their way");

    temp.open().checkout_branch("side").unwrap();
    temp.open().rebase_onto(&trunk).unwrap();
    (temp, trunk, tip)
}

/// Two commits on a branch, both of which end up clashing: the second only
/// because of how the first was decided. The shape a rebase has and a merge
/// does not.
fn two_clashes(name: &str) -> (TempRepo, String) {
    let temp = TempRepo::new(name);
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("side").unwrap();
    commit(&temp, "a.txt", "side one\n", "feat: a, my way");
    commit(&temp, "a.txt", "side two\n", "feat: a again");

    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "a.txt", "trunk one\n", "feat: a, their way");

    temp.open().checkout_branch("side").unwrap();
    temp.open().rebase_onto(&trunk).unwrap();
    (temp, trunk)
}

#[test]
fn a_rebase_that_clashes_stops_there_instead_of_undoing_itself() {
    let (temp, _, _) = one_clash("rebase-stops");

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::Rebase);

    let clashes = repo.clashes().unwrap();
    assert_eq!(clashes.len(), 1);
    assert_eq!(clashes[0].path, "a.txt");
    assert_eq!(clashes[0].shape(), "both sides changed it");
}

/// The message is the only thing that says this is one commit of several, and
/// that is the difference between a rebase and a merge in one sentence.
#[test]
fn the_rebase_says_which_commit_of_how_many_it_stopped_at() {
    let temp = TempRepo::new("rebase-says-where");
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("side").unwrap();
    commit(&temp, "a.txt", "side one\n", "feat: a, my way");
    commit(&temp, "b.txt", "side two\n", "feat: b");

    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "a.txt", "trunk one\n", "feat: a, their way");

    temp.open().checkout_branch("side").unwrap();
    let note = temp.open().rebase_onto(&trunk).unwrap();

    assert!(note.contains("a.txt"), "it says what clashes: {note}");
    assert!(note.contains("at commit 1 of 2"), "{note}");
    assert!(note.contains(&trunk), "it names what it is going onto: {note}");
}

/// The one that catches everybody out. In a merge "ours" is the branch you
/// are standing on; in a rebase your own work is what is being replayed, so
/// it arrives as "theirs" and the branch underneath is "ours".
#[test]
fn the_two_sides_of_a_rebase_are_the_other_way_round_from_a_merge() {
    let (temp, _, _) = one_clash("rebase-sides");

    let clashes = temp.open().clashes().unwrap();
    let clash = &clashes[0];

    // Ours: the branch being rebased onto, which is not the branch we were on.
    assert_eq!(clash.ours, Contents::Text("trunk one\n".to_string()));
    // Theirs: our own commit, the one being replayed.
    assert_eq!(clash.theirs, Contents::Text("side one\n".to_string()));

    // And nothing on the screen ever says "ours" or "theirs" about it.
    assert_eq!(
        Half::Rebase.sides(),
        ("the branch underneath", "the commit being replayed")
    );
    assert_eq!(Half::Merge.sides(), ("this branch", "the other branch"));
    assert_eq!(
        Side::Theirs.says(Half::Rebase),
        "what the commit being replayed says"
    );
}

#[test]
fn deciding_every_file_and_carrying_on_finishes_a_rebase_that_only_stopped_once() {
    let (temp, trunk, _) = one_clash("rebase-carry-on");

    temp.open().take_side("a.txt", Side::Theirs).unwrap();
    let note = temp.open().carry_on_rebase().unwrap();

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None, "nothing is half-done any more");
    assert_eq!(repo.head_name(), "side", "and we are back on the branch");
    assert!(repo.status().unwrap().is_clean());

    // One straight line: the base, the trunk's commit, then both of ours.
    assert_eq!(repo.commits(50).unwrap().len(), 4);
    assert!(note.contains("replayed"), "{note}");

    // Both decisions survived: the file we settled and the one that replayed
    // cleanly afterwards.
    assert_eq!(read(&temp, "a.txt"), "side one\n");
    assert_eq!(read(&temp, "b.txt"), "side two\n");

    // And the branch really is on top of the trunk now, not beside it.
    assert!(repo.has_room(&trunk, "side").unwrap());
}

/// Carrying on is a step, not an ending. This is the whole reason the button
/// cannot say "write the merge".
#[test]
fn carrying_on_can_stop_again_at_the_very_next_commit() {
    let (temp, _) = two_clashes("rebase-stops-twice");

    // The first clash: decide it the way the branch underneath already had
    // it, which leaves that commit with nothing in it at all.
    temp.open().take_side("a.txt", Side::Ours).unwrap();
    let note = temp.open().carry_on_rebase().unwrap();

    // Not finished: the next commit's patch no longer fits what we just said.
    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::Rebase, "still half-done: {note}");
    assert_eq!(repo.clashes().unwrap().len(), 1);
    assert!(note.contains("at commit 2 of 2"), "{note}");

    // Decide the second one and it really does end.
    temp.open().take_side("a.txt", Side::Theirs).unwrap();
    temp.open().carry_on_rebase().unwrap();

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None);
    assert_eq!(repo.head_name(), "side");
    assert_eq!(read(&temp, "a.txt"), "side two\n");

    // The first commit fell away: deciding it the way it already was left it
    // with nothing to say. Base, the trunk's commit, and the one that was left.
    assert_eq!(repo.commits(50).unwrap().len(), 3);
}

#[test]
fn carrying_on_with_a_file_still_undecided_is_refused_and_says_which() {
    let (temp, _, _) = one_clash("rebase-not-yet");

    let err = temp.open().carry_on_rebase().unwrap_err().to_string();
    assert!(err.contains("still to decide"), "{err}");
    assert!(err.contains("a.txt"), "{err}");

    // And it changed nothing by refusing.
    assert_eq!(temp.open().half_done(), Half::Rebase);
    assert_eq!(temp.open().clashes().unwrap().len(), 1);
}

#[test]
fn carrying_on_when_no_rebase_is_half_done_says_so() {
    let temp = TempRepo::new("rebase-nothing-to-carry");
    commit(&temp, "a.txt", "one\n", "feat: one");

    let err = temp.open().carry_on_rebase().unwrap_err().to_string();
    assert!(err.contains("no half-done rebase"), "{err}");
}

/// Throwing a rebase away has to put back more than the files: every commit
/// it had already replayed goes back where it was, and HEAD goes back onto
/// the branch it came off.
#[test]
fn throwing_a_half_done_rebase_away_puts_the_branch_back_where_it_was() {
    let (temp, _, tip) = one_clash("rebase-thrown-away");

    let note = temp.open().abandon().unwrap();
    assert!(note.contains("side"), "it says where you are: {note}");

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None);
    assert_eq!(repo.head_name(), "side", "not left off a branch");
    assert_eq!(repo.head_oid(), Some(tip), "the branch is back on its own tip");
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(read(&temp, "a.txt"), "side one\n");
}

#[test]
fn nothing_new_may_start_while_a_rebase_is_half_done() {
    let (temp, trunk, _) = one_clash("rebase-one-at-a-time");

    let err = temp.open().merge(&trunk).unwrap_err().to_string();
    assert!(err.contains("a rebase is half-done"), "{err}");

    let err = temp.open().rebase_onto(&trunk).unwrap_err().to_string();
    assert!(err.contains("a rebase is half-done"), "{err}");

    assert_eq!(temp.open().half_done(), Half::Rebase);
}

/// Dropping one branch on another with a rebase that clashes leaves HEAD off
/// every branch, and the sentence must not claim otherwise.
#[test]
fn a_dropped_rebase_that_clashes_does_not_claim_you_are_on_a_branch() {
    let temp = TempRepo::new("rebase-dropped");
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("side").unwrap();
    commit(&temp, "a.txt", "side one\n", "feat: a, my way");
    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "a.txt", "trunk one\n", "feat: a, their way");

    // Standing on the trunk, drop the trunk on "side": "side" is replayed.
    let note = temp
        .open()
        .join(&trunk, "side", gitrust_core::Join::Rebase)
        .unwrap();

    assert!(
        !note.contains("you are now on"),
        "a stopped rebase stands on no branch: {note}"
    );
    assert_eq!(temp.open().half_done(), Half::Rebase);
}
