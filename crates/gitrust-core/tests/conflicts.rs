//! Getting out of a merge that did not come out clean.
//!
//! The three ways out -- take a side, write it yourself, throw it all away --
//! and the three shapes of clash that are easiest to get wrong: both changed
//! it, one deleted it, both added it.

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

/// Both sides change the same line: the commonest clash there is.
fn clashing(name: &str) -> (TempRepo, String) {
    let temp = TempRepo::new(name);
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("theirs").unwrap();
    commit(&temp, "a.txt", "what they say\n", "feat: theirs");
    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "a.txt", "what I say\n", "feat: ours");

    temp.open().merge("theirs").unwrap();
    (temp, trunk)
}

#[test]
fn a_clash_is_readable_in_all_three_of_its_versions() {
    let (temp, _) = clashing("clash-read");

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::Merge);

    let clashes = repo.clashes().unwrap();
    assert_eq!(clashes.len(), 1);
    let clash = &clashes[0];
    assert_eq!(clash.path, "a.txt");
    assert_eq!(clash.base, Contents::Text("one\n".to_string()));
    assert_eq!(clash.ours, Contents::Text("what I say\n".to_string()));
    assert_eq!(clash.theirs, Contents::Text("what they say\n".to_string()));
    assert_eq!(clash.shape(), "both sides changed it");
}

#[test]
fn taking_a_side_settles_the_file_and_leaves_the_merge_ready() {
    let (temp, _) = clashing("clash-take-ours");

    temp.open().take_side("a.txt", Side::Ours).unwrap();

    let repo = temp.open();
    assert!(repo.clashes().unwrap().is_empty(), "nothing left to decide");
    assert_eq!(read(&temp, "a.txt"), "what I say\n");
    // Still half-done: deciding is not committing.
    assert_eq!(repo.half_done(), Half::Merge);
}

#[test]
fn taking_the_other_side_takes_the_other_side() {
    let (temp, _) = clashing("clash-take-theirs");

    temp.open().take_side("a.txt", Side::Theirs).unwrap();

    assert_eq!(read(&temp, "a.txt"), "what they say\n");
    assert!(temp.open().clashes().unwrap().is_empty());
}

#[test]
fn a_decided_merge_is_written_with_both_sides_as_its_parents() {
    let (temp, trunk) = clashing("clash-finish");
    let theirs = temp.open().merging_in().expect("something is coming in");

    temp.open().take_side("a.txt", Side::Ours).unwrap();
    temp.open().finish_merge().unwrap();

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None, "nothing is half-done any more");
    assert_eq!(repo.head_name(), trunk);

    let tip = repo.commits(20).unwrap();
    let tip = tip.first().expect("there is a commit");
    assert_eq!(tip.parents.len(), 2, "it is a merge");
    assert!(tip.parents.contains(&theirs));
    assert!(repo.status().unwrap().is_clean());
}

/// Committing a merge with a file still undecided would write the markers
/// into the history, which is the one outcome worth refusing outright.
#[test]
fn a_merge_with_anything_left_to_decide_is_refused_and_says_what() {
    let (temp, _) = clashing("clash-unfinished");

    let err = temp.open().finish_merge().unwrap_err().to_string();
    assert!(err.contains("a.txt"), "{err}");
    assert_eq!(temp.open().half_done(), Half::Merge, "still half-done");
}

/// Leaving `<<<<<<<` in a file and calling it resolved commits, pushes, and
/// breaks when somebody runs it. Nothing afterwards complains, so this does.
#[test]
fn text_with_the_markers_still_in_it_is_refused() {
    let (temp, _) = clashing("clash-markers");

    let err = temp
        .open()
        .take_text("a.txt", "<<<<<<< HEAD\nwhat I say\n=======\nwhat they say\n>>>>>>> theirs\n")
        .unwrap_err()
        .to_string();

    assert!(err.contains("markers"), "{err}");
    assert_eq!(temp.open().clashes().unwrap().len(), 1, "still undecided");
}

#[test]
fn text_somebody_wrote_themselves_settles_it() {
    let (temp, _) = clashing("clash-by-hand");

    temp.open().take_text("a.txt", "what we both say\n").unwrap();

    assert_eq!(read(&temp, "a.txt"), "what we both say\n");
    assert!(temp.open().clashes().unwrap().is_empty());
    temp.open().finish_merge().unwrap();
    assert_eq!(temp.open().half_done(), Half::None);
}

/// A row of equals signs is most Markdown ever written, and refusing it
/// would make a heading impossible to resolve.
#[test]
fn a_markdown_heading_underline_is_not_a_conflict_marker() {
    let (temp, _) = clashing("clash-markdown");

    temp.open().take_text("a.txt", "A heading\n=========\n\nand text\n").unwrap();
    assert!(temp.open().clashes().unwrap().is_empty());
}

#[test]
fn throwing_the_whole_thing_away_puts_the_files_back() {
    let (temp, trunk) = clashing("clash-abandon");
    let before = temp.open().head_oid().unwrap();

    temp.open().abandon().unwrap();

    let repo = temp.open();
    assert_eq!(repo.half_done(), Half::None);
    assert_eq!(repo.head_oid().unwrap(), before, "no commit was written");
    assert_eq!(repo.head_name(), trunk);
    assert_eq!(read(&temp, "a.txt"), "what I say\n", "the markers are gone");
    assert!(repo.status().unwrap().is_clean());
}

/// One side deletes a file, the other changes it. Git cannot guess and
/// neither should this: the shape is said so somebody can decide.
#[test]
fn a_file_deleted_on_one_side_is_a_clash_that_says_so() {
    let temp = TempRepo::new("clash-deleted");
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("theirs").unwrap();
    commit(&temp, "a.txt", "they changed it\n", "feat: theirs");
    temp.open().checkout_branch(&trunk).unwrap();

    std::fs::remove_file(temp.path().join("a.txt")).unwrap();
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: gone").unwrap();

    temp.open().merge("theirs").unwrap();

    let clashes = temp.open().clashes().unwrap();
    assert_eq!(clashes.len(), 1);
    assert_eq!(clashes[0].ours, Contents::Absent);
    assert_eq!(clashes[0].shape(), "deleted here, changed there");

    // Taking our side means it stays gone.
    temp.open().take_side("a.txt", Side::Ours).unwrap();
    assert!(!temp.path().join("a.txt").exists());
    temp.open().finish_merge().unwrap();
    assert_eq!(temp.open().half_done(), Half::None);
}

/// Both sides add the same path with different contents: there is no
/// ancestor version at all, which is not an empty file.
#[test]
fn a_file_added_on_both_sides_has_no_version_underneath_it() {
    let temp = TempRepo::new("clash-added");
    commit(&temp, "seed.txt", "seed\n", "feat: seed");
    let trunk = temp.open().head_name();

    temp.open().branch_here("theirs").unwrap();
    commit(&temp, "new.txt", "theirs\n", "feat: theirs");
    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "new.txt", "ours\n", "feat: ours");

    temp.open().merge("theirs").unwrap();

    let clashes = temp.open().clashes().unwrap();
    assert_eq!(clashes.len(), 1);
    assert_eq!(clashes[0].base, Contents::Absent);
    assert_eq!(clashes[0].shape(), "added on both sides, differently");
}

/// Two sets of markers in one working tree and no way to tell which decision
/// belongs to which.
#[test]
fn nothing_new_may_start_while_something_is_half_done() {
    let (temp, _) = clashing("clash-one-at-a-time");
    temp.open().branch_at("other", temp.open().head_oid().unwrap()).unwrap();

    let err = temp.open().merge("other").unwrap_err().to_string();
    assert!(err.contains("half-done"), "{err}");
}

#[test]
fn abandoning_when_nothing_is_half_done_says_so_rather_than_resetting() {
    let temp = TempRepo::new("clash-nothing");
    commit(&temp, "a.txt", "one\n", "feat: one");
    temp.write("a.txt", "something I am in the middle of\n");

    let err = temp.open().abandon().unwrap_err().to_string();
    assert!(err.contains("nothing half-done"), "{err}");
    // And it really did not reset: that work is still there.
    assert_eq!(read(&temp, "a.txt"), "something I am in the middle of\n");
}
