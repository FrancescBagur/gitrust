//! What a branch says it is for.
//!
//! A note is the only thing on the board that is written rather than read off
//! the graph, apart from the backlog it shares a reference with. These tests
//! care about two things: that what was written comes back, and that it
//! shares that reference without the two standing on each other.

mod common;

use common::TempRepo;

fn start(temp: &TempRepo) -> String {
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: one").unwrap();
    repo.head_name()
}

#[test]
fn a_branch_with_nothing_written_about_it_has_no_note() {
    let temp = TempRepo::new("note-none");
    let trunk = start(&temp);

    let repo = temp.open();
    assert_eq!(repo.note(&trunk).unwrap(), None);
    assert!(repo.notes().unwrap().is_empty());
}

#[test]
fn what_was_written_about_a_branch_comes_back() {
    let temp = TempRepo::new("note-round-trip");
    let trunk = start(&temp);

    let repo = temp.open();
    repo.note_set(
        &trunk,
        "the footer rides over the content on a narrow phone",
        "Only below 420px.\nThe sticky bar is the one at fault.",
    )
    .unwrap();

    let note = temp.open().note(&trunk).unwrap().expect("there is a note");
    assert_eq!(note.branch, trunk);
    assert_eq!(note.title, "the footer rides over the content on a narrow phone");
    assert_eq!(note.body, "Only below 420px.\nThe sticky bar is the one at fault.");
    assert!(!note.by.is_empty(), "it says who wrote it");
}

#[test]
fn writing_it_again_replaces_it_rather_than_leaving_two() {
    let temp = TempRepo::new("note-replace");
    let trunk = start(&temp);

    temp.open().note_set(&trunk, "first thought", "").unwrap();
    temp.open().note_set(&trunk, "second thought", "and why").unwrap();

    let notes = temp.open().notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[&trunk].title, "second thought");
    assert_eq!(notes[&trunk].body, "and why");
}

/// Clearing the box is the only way anybody would try to say there is nothing
/// written here after all, so an empty title takes the note away.
#[test]
fn a_note_with_its_title_rubbed_out_is_no_note_at_all() {
    let temp = TempRepo::new("note-emptied");
    let trunk = start(&temp);

    temp.open().note_set(&trunk, "something", "body").unwrap();
    temp.open().note_set(&trunk, "   ", "body still here").unwrap();

    assert_eq!(temp.open().note(&trunk).unwrap(), None);
}

#[test]
fn taking_a_note_away_leaves_the_others_where_they_were() {
    let temp = TempRepo::new("note-clear-one");
    let trunk = start(&temp);
    temp.open().branch_here("fix/footer").unwrap();

    temp.open().note_set(&trunk, "the trunk", "").unwrap();
    temp.open().note_set("fix/footer", "the footer", "").unwrap();
    temp.open().note_clear(&trunk).unwrap();

    let notes = temp.open().notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes["fix/footer"].title, "the footer");
}

/// A branch name with slashes in it is the ordinary case, and the file it is
/// kept in has to come back as the same name.
#[test]
fn a_branch_with_slashes_in_its_name_is_noted_like_any_other() {
    let temp = TempRepo::new("note-slashes");
    start(&temp);
    temp.open().branch_here("feat/report/export").unwrap();

    temp.open()
        .note_set("feat/report/export", "export the report to PDF", "")
        .unwrap();

    let note = temp.open().note("feat/report/export").unwrap().expect("a note");
    assert_eq!(note.branch, "feat/report/export");
}

#[test]
fn starting_an_item_keeps_the_sentence_that_was_written() {
    let temp = TempRepo::new("note-from-backlog");
    start(&temp);

    let repo = temp.open();
    repo.backlog_add("the footer rides over the content", None).unwrap();
    let id = temp.open().backlog().unwrap().items[0].id.clone();
    let branch = temp.open().backlog().unwrap().items[0].branch();

    temp.open().backlog_start(&id).unwrap();

    // The item is gone from the waiting room, and the words are not.
    let repo = temp.open();
    assert!(repo.backlog().unwrap().items.is_empty());
    let note = repo.note(&branch).unwrap().expect("the sentence survived");
    assert_eq!(note.title, "the footer rides over the content");
    assert_eq!(note.body, "");
}

/// They live in one reference, so each has to hand the other back untouched
/// when it writes. This is the test that would have caught doing it wrong.
#[test]
fn the_backlog_and_the_notes_do_not_stand_on_each_other() {
    let temp = TempRepo::new("note-shares-the-ref");
    let trunk = start(&temp);

    temp.open().note_set(&trunk, "what the trunk is for", "").unwrap();
    temp.open().backlog_add("something to do", None).unwrap();

    // Writing the backlog kept the note.
    let repo = temp.open();
    assert_eq!(repo.backlog().unwrap().items.len(), 1);
    assert!(repo.note(&trunk).unwrap().is_some());

    // And writing a note keeps the backlog.
    temp.open().note_set(&trunk, "changed my mind", "").unwrap();
    let repo = temp.open();
    assert_eq!(repo.backlog().unwrap().items.len(), 1);
    assert_eq!(repo.note(&trunk).unwrap().unwrap().title, "changed my mind");

    // And taking an item out keeps it too.
    let id = repo.backlog().unwrap().items[0].id.clone();
    temp.open().backlog_remove(&id).unwrap();
    let repo = temp.open();
    assert!(repo.backlog().unwrap().items.is_empty());
    assert!(repo.note(&trunk).unwrap().is_some());
}

/// Nothing tidies these away, which is a decision and not an oversight: a
/// branch can come back, and a deletion can be a mistake.
#[test]
fn a_note_outlives_the_branch_it_was_written_about() {
    let temp = TempRepo::new("note-outlives");
    let trunk = start(&temp);
    temp.open().branch_here("fix/footer").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    temp.open().note_set("fix/footer", "the footer", "").unwrap();
    temp.open().delete_branch("fix/footer").unwrap();

    assert!(temp.open().note("fix/footer").unwrap().is_some());
}
