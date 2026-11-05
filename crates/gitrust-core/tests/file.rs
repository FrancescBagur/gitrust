//! One file, looked at four ways.
//!
//! What these care about is the cases a diff never shows you: a file before
//! it existed, a line nobody has committed yet, a merge that touched nothing,
//! and a hunk undone in the working tree while the history stays put.

mod common;

use common::TempRepo;
use gitrust_core::{Contents, Oid};

fn commit(temp: &TempRepo, file: &str, body: &str, message: &str) -> Oid {
    temp.write(file, body);
    let repo = temp.open();
    repo.stage(file).unwrap();
    repo.commit(message).unwrap();
    temp.open().head_oid().expect("there is a commit")
}

#[test]
fn a_file_comes_back_as_it_stood_at_that_commit() {
    let temp = TempRepo::new("file-at");
    let first = commit(&temp, "a.txt", "one\n", "feat: one");
    let second = commit(&temp, "a.txt", "one\ntwo\n", "feat: two");

    assert_eq!(temp.open().file_at(first, "a.txt").unwrap(), Contents::Text("one\n".to_string()));
    assert_eq!(
        temp.open().file_at(second, "a.txt").unwrap(),
        Contents::Text("one\ntwo\n".to_string())
    );
}

/// Before a file's first commit there is nothing, which is not the same as
/// an empty file and should not read like one.
#[test]
fn a_file_that_was_not_there_yet_is_absent_rather_than_empty() {
    let temp = TempRepo::new("file-absent");
    let first = commit(&temp, "a.txt", "one\n", "feat: one");
    commit(&temp, "b.txt", "two\n", "feat: two");

    assert_eq!(temp.open().file_at(first, "b.txt").unwrap(), Contents::Absent);
}

#[test]
fn something_that_is_not_text_says_so_instead_of_coming_back_as_mush() {
    let temp = TempRepo::new("file-binary");
    commit(&temp, "a.txt", "one\n", "feat: one");

    std::fs::write(temp.path().join("logo.bin"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
    let repo = temp.open();
    repo.stage("logo.bin").unwrap();
    repo.commit("feat: a picture").unwrap();
    let at = temp.open().head_oid().unwrap();

    assert!(matches!(
        temp.open().file_at(at, "logo.bin").unwrap(),
        Contents::Binary(_)
    ));
}

#[test]
fn blame_says_which_commit_each_line_came_from() {
    let temp = TempRepo::new("blame");
    let first = commit(&temp, "a.txt", "one\n", "feat: one");
    let second = commit(&temp, "a.txt", "one\ntwo\n", "feat: two");

    let lines = temp.open().blame("a.txt", None).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text, "one");
    assert_eq!(lines[0].commit, first);
    assert_eq!(lines[1].commit, second);
    assert_eq!(lines[1].summary, "feat: two");
    assert!(!lines[0].author.is_empty());
}

/// Blame reads as blocks. Which line starts one is worked out while walking
/// them, because the view would otherwise have to compare each line with the
/// one above it to recover something already known.
#[test]
fn the_first_line_of_each_run_is_the_one_marked() {
    let temp = TempRepo::new("blame-runs");
    commit(&temp, "a.txt", "one\ntwo\n", "feat: both");
    commit(&temp, "a.txt", "one\ntwo\nthree\n", "feat: a third");

    let lines = temp.open().blame("a.txt", None).unwrap();
    assert!(lines[0].first, "the top of the file always starts a run");
    assert!(!lines[1].first, "same commit as the line above");
    assert!(lines[2].first, "a different commit");
}

/// A line written and not committed belongs to no commit, and is given the
/// null Oid the way the working tree's row in the graph is.
#[test]
fn a_line_nobody_has_committed_belongs_to_no_commit() {
    let temp = TempRepo::new("blame-uncommitted");
    commit(&temp, "a.txt", "one\n", "feat: one");
    temp.write("a.txt", "one\nscribbled\n");

    let lines = temp.open().blame("a.txt", None).unwrap();
    assert_eq!(lines.len(), 2);
    assert!(lines[1].commit.is_zero(), "{:?}", lines[1].commit);
    assert!(lines[1].short.is_empty());
}

#[test]
fn a_files_history_holds_only_what_touched_it() {
    let temp = TempRepo::new("file-history");
    let first = commit(&temp, "a.txt", "one\n", "feat: one");
    commit(&temp, "b.txt", "elsewhere\n", "feat: somewhere else");
    let third = commit(&temp, "a.txt", "one\ntwo\n", "feat: two");

    let story = temp.open().file_history("a.txt", 50).unwrap();
    let ids: Vec<Oid> = story.commits.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![third, first], "newest first, and nothing in between");
    assert!(!story.cut_short);
}

/// A list that might be missing its oldest entries has to say so, or it is a
/// different answer wearing the same clothes.
#[test]
fn a_history_that_ran_out_of_room_admits_it() {
    let temp = TempRepo::new("file-history-cut");
    for n in 0..6 {
        commit(&temp, "a.txt", &format!("line {n}\n"), &format!("feat: {n}"));
    }

    let story = temp.open().file_history("a.txt", 3).unwrap();
    assert!(story.cut_short);
    assert!(story.commits.len() <= 3);
}

/// A merge mentions every path either side touched. Listing it would put a
/// commit in the history of a file it did nothing to.
#[test]
fn a_merge_that_did_nothing_to_the_file_stays_out_of_its_history() {
    let temp = TempRepo::new("file-history-merge");
    commit(&temp, "a.txt", "one\n", "feat: one");
    let trunk = temp.open().head_name();

    temp.open().branch_here("side").unwrap();
    commit(&temp, "side.txt", "theirs\n", "feat: theirs");
    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "ours.txt", "ours\n", "feat: ours");
    temp.open().merge("side").unwrap();

    let story = temp.open().file_history("a.txt", 50).unwrap();
    assert_eq!(story.commits.len(), 1, "only the commit that wrote it");
}

#[test]
fn undoing_one_hunk_changes_the_files_and_leaves_the_history_alone() {
    let temp = TempRepo::new("revert-hunk");
    commit(&temp, "a.txt", "one\ntwo\nthree\n", "feat: three lines");
    let changed = commit(&temp, "a.txt", "one\nCHANGED\nthree\n", "feat: the middle");

    let hunks = temp.open().commit_file_diff(changed, "a.txt").unwrap();
    let hunk = hunks.hunks.first().expect("there is a hunk").id;
    temp.open().revert_hunk(changed, "a.txt", hunk).unwrap();

    // The files went back; the commit did not move.
    let text = std::fs::read_to_string(temp.path().join("a.txt")).unwrap();
    assert_eq!(text, "one\ntwo\nthree\n");
    assert_eq!(temp.open().head_oid().unwrap(), changed);
    assert!(!temp.open().status().unwrap().is_clean(), "it is sitting there uncommitted");
}

/// Applying a patch backwards over something somebody is in the middle of is
/// how you get a file that is neither what they wrote nor what they had.
#[test]
fn a_file_with_uncommitted_changes_is_refused_and_left_exactly_as_it_was() {
    let temp = TempRepo::new("revert-hunk-busy");
    commit(&temp, "a.txt", "one\ntwo\nthree\n", "feat: three lines");
    let changed = commit(&temp, "a.txt", "one\nCHANGED\nthree\n", "feat: the middle");

    temp.write("a.txt", "one\nCHANGED\nthree\nmine\n");
    let hunks = temp.open().commit_file_diff(changed, "a.txt").unwrap();
    let hunk = hunks.hunks.first().expect("there is a hunk").id;

    let err = temp.open().revert_hunk(changed, "a.txt", hunk).unwrap_err().to_string();
    assert!(err.contains("not committed"), "{err}");
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt")).unwrap(),
        "one\nCHANGED\nthree\nmine\n"
    );
}
