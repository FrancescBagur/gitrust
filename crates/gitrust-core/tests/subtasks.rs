//! Tests of which branch came out of which, and of the board inside a card.
//!
//! Git records nothing about where a branch came from, so all of this is read
//! off the shape of the history. These tests build the shapes by hand -- a
//! fork, two siblings, a branch off a branch off a branch -- and check that
//! the reading matches what somebody who drew it on paper would say.

mod common;

use common::TempRepo;
use gitrust_core::{Lane, Repo};

/// The first commit, and the name of the branch it landed on.
fn start(temp: &TempRepo) -> String {
    write(temp, "a.txt", "one\n", "feat: one");
    temp.open().head_name()
}

fn write(temp: &TempRepo, file: &str, body: &str, message: &str) {
    temp.write(file, body);
    let repo = temp.open();
    repo.stage(file).unwrap();
    repo.commit(message).unwrap();
}

/// Cuts a branch where we are and puts a commit on it.
fn branch_with(temp: &TempRepo, name: &str, file: &str, body: &str) {
    temp.open().branch_here(name).unwrap();
    write(temp, file, body, &format!("feat: {name}"));
}

fn parent(repo: &Repo, branch: &str) -> Option<String> {
    repo.parents().unwrap().get(branch).cloned().flatten()
}

fn on(repo: &Repo, base: &str) -> Vec<String> {
    let mut names: Vec<String> = repo
        .board_against(base, None)
        .unwrap()
        .cards
        .into_iter()
        .map(|card| card.branch)
        .collect();
    names.sort();
    names
}

#[test]
fn two_branches_that_left_the_trunk_together_are_neither_the_others_parent() {
    let temp = TempRepo::new("subtask-siblings");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    temp.open().checkout_branch(&trunk).unwrap();
    branch_with(&temp, "tweak", "t.txt", "one\n");
    temp.open().checkout_branch(&trunk).unwrap();

    let repo = temp.open();
    assert_eq!(parent(&repo, "report"), None);
    assert_eq!(parent(&repo, "tweak"), None);
    assert_eq!(on(&repo, &trunk), ["report", "tweak"]);
}

#[test]
fn a_branch_cut_from_a_branch_is_its_subtask_and_leaves_the_board_outside() {
    let temp = TempRepo::new("subtask-child");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    // Cut from report where report now is, which is what makes it a subtask.
    branch_with(&temp, "report-export", "e.txt", "one\n");
    temp.open().checkout_branch(&trunk).unwrap();

    let repo = temp.open();
    assert_eq!(parent(&repo, "report-export").as_deref(), Some("report"));

    // It is on one board only, and that board is inside its parent's card.
    assert_eq!(on(&repo, &trunk), ["report"]);
    assert_eq!(on(&repo, "report"), ["report-export"]);

    // So the card says how many are in there, or it would be nowhere anybody
    // thought to look.
    let outside = repo.board_against(&trunk, None).unwrap();
    assert_eq!(outside.cards[0].branch, "report");
    assert_eq!(outside.cards[0].children, 1);
}

#[test]
fn a_subtask_is_measured_against_its_parent_and_not_against_the_trunk() {
    let temp = TempRepo::new("subtask-measure");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    write(&temp, "r.txt", "one\ntwo\n", "feat: more of report");
    branch_with(&temp, "report-export", "e.txt", "one\n");
    temp.open().checkout_branch(&trunk).unwrap();

    // Three commits ahead of the trunk, one ahead of its parent. One of those
    // is the honest answer to "how much work is this subtask", and it is the
    // smaller one.
    let repo = temp.open();
    let inside = repo.board_against("report", None).unwrap();
    assert_eq!(inside.cards[0].ahead, 1);
    assert_eq!(inside.trunk, "report");

    let outside = repo.board_against(&trunk, None).unwrap();
    assert_eq!(outside.cards[0].ahead, 2);
}

#[test]
fn done_for_a_subtask_is_having_gone_into_its_parent() {
    let temp = TempRepo::new("subtask-done");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    branch_with(&temp, "report-export", "e.txt", "one\n");
    // The parent carried on while the subtask was being done, which is the
    // ordinary way of it and is what makes the merge a merge.
    temp.open().checkout_branch("report").unwrap();
    write(&temp, "r.txt", "one\ntwo\n", "feat: more of report");

    // Into its parent, not into the trunk.
    temp.open().land_into("report", "report-export").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    let repo = temp.open();
    let inside = repo.board_against("report", None).unwrap();
    assert_eq!(inside.cards[0].branch, "report-export");
    assert_eq!(inside.cards[0].lane, Lane::Done);

    // Finishing it must not take it off the board somebody would look for it
    // on. Being inside its parent is the one thing that says it is finished.
    assert_eq!(parent(&repo, "report-export").as_deref(), Some("report"));

    // And the parent is still work in progress, because nothing went into the
    // trunk at all.
    let outside = repo.board_against(&trunk, None).unwrap();
    assert_eq!(outside.cards[0].branch, "report");
    assert_eq!(outside.cards[0].lane, Lane::InProgress);
}

#[test]
fn a_subtask_that_went_in_by_fast_forward_cannot_be_told_from_its_parent() {
    let temp = TempRepo::new("subtask-fast-forward");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    branch_with(&temp, "report-export", "e.txt", "one\n");
    // The parent did not move, so there is nothing to merge: it just catches
    // up. Afterwards the two names point at the same commit.
    temp.open().land_into("report", "report-export").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    // This is the known wrong answer, pinned here rather than left to be
    // found. Two names on one commit say nothing about which came from which,
    // so neither is the other's parent and both sit outside. It is the same
    // blindness the columns already own up to: a fast forward leaves the
    // graph no evidence that anything happened.
    let repo = temp.open();
    assert_eq!(parent(&repo, "report-export"), None);
    assert_eq!(parent(&repo, "report"), None);
    assert_eq!(on(&repo, &trunk), ["report", "report-export"]);
}

#[test]
fn two_subtasks_of_the_same_branch_are_its_children_and_not_each_others() {
    let temp = TempRepo::new("subtask-twins");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    branch_with(&temp, "report-export", "e.txt", "one\n");
    temp.open().checkout_branch("report").unwrap();
    branch_with(&temp, "report-print", "p.txt", "one\n");
    temp.open().checkout_branch(&trunk).unwrap();

    // Both forked at the same commit, so the fork alone cannot tell them
    // apart. What does is that the parent is still standing there and the
    // sibling has walked on.
    let repo = temp.open();
    assert_eq!(parent(&repo, "report-export").as_deref(), Some("report"));
    assert_eq!(parent(&repo, "report-print").as_deref(), Some("report"));
    assert_eq!(on(&repo, "report"), ["report-export", "report-print"]);
    assert_eq!(on(&repo, &trunk), ["report"]);
}

#[test]
fn a_subtask_of_a_subtask_goes_one_board_further_in() {
    let temp = TempRepo::new("subtask-deep");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    branch_with(&temp, "report-export", "e.txt", "one\n");
    branch_with(&temp, "report-export-csv", "c.txt", "one\n");
    temp.open().checkout_branch(&trunk).unwrap();

    let repo = temp.open();
    assert_eq!(parent(&repo, "report-export").as_deref(), Some("report"));
    assert_eq!(parent(&repo, "report-export-csv").as_deref(), Some("report-export"));

    assert_eq!(on(&repo, &trunk), ["report"]);
    assert_eq!(on(&repo, "report"), ["report-export"]);
    assert_eq!(on(&repo, "report-export"), ["report-export-csv"]);

    // One in each, all the way down.
    let inside = repo.board_against("report", None).unwrap();
    assert_eq!(inside.cards[0].children, 1);
}

#[test]
fn a_rebase_moves_the_fork_and_the_subtask_comes_back_out() {
    let temp = TempRepo::new("subtask-rebase");
    let trunk = start(&temp);

    branch_with(&temp, "report", "r.txt", "one\n");
    branch_with(&temp, "report-export", "e.txt", "one\n");
    assert_eq!(parent(&temp.open(), "report-export").as_deref(), Some("report"));

    // The trunk moves on, so replaying really does rewrite the commits. On a
    // trunk that had not moved they would come out identical and the rebase
    // would change nothing at all.
    temp.open().checkout_branch(&trunk).unwrap();
    write(&temp, "a.txt", "one\ntwo\n", "feat: two");
    temp.open().checkout_branch("report-export").unwrap();

    // This is the second known wrong answer. Replaying the branch on the
    // trunk leaves nothing in its history saying it ever hung from report, so
    // it reads as cut straight from the trunk -- which, from the graph alone,
    // it now is.
    temp.open().rebase_onto(&trunk).unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    let repo = temp.open();
    assert_eq!(parent(&repo, "report-export"), None);
    assert_eq!(on(&repo, &trunk), ["report", "report-export"]);
    assert_eq!(on(&repo, "report"), Vec::<String>::new());
}
