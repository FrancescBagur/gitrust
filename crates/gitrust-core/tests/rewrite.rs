//! Rewriting a run of commits: squash, fixup, reword, drop, reorder.
//!
//! The thing worth testing hardest is not any one of those. It is that a plan
//! which cannot be carried out leaves the repository exactly as it was --
//! this is the one operation in gitrust that cannot be stopped halfway and
//! picked up again, so "nothing happened" has to be true and not nearly true.

mod common;

use common::TempRepo;
use gitrust_core::{Oid, Step};

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

fn subjects(temp: &TempRepo) -> Vec<String> {
    temp.open()
        .commits(50)
        .unwrap()
        .into_iter()
        .map(|commit| commit.summary)
        .collect()
}

/// A base commit and three on top of it, each adding a file of its own, so
/// that any arrangement of them applies cleanly.
fn three(name: &str) -> (TempRepo, Oid) {
    let temp = TempRepo::new(name);
    commit(&temp, "a.txt", "one\n", "feat: the base");
    let first = commit(&temp, "b.txt", "two\n", "feat: b");
    commit(&temp, "c.txt", "three\n", "feat: c");
    commit(&temp, "d.txt", "four\n", "feat: d");
    (temp, first)
}

#[test]
fn a_plan_starts_as_everything_kept_oldest_first() {
    let (temp, first) = three("plan-starts");

    let plan = temp.open().plan_from(first).unwrap();

    assert_eq!(plan.lines.len(), 3);
    assert!(plan.lines.iter().all(|line| line.step == Step::Pick));
    assert_eq!(plan.lines[0].commit, first, "oldest first");
    assert_eq!(plan.lines[0].summary, "feat: b");
    assert_eq!(plan.lines[2].summary, "feat: d");
    assert_eq!(plan.onto, temp.open().commits(50).unwrap()[3].id);
    assert_eq!(plan.branch, temp.open().head_name());
    assert_eq!(plan.kept(), 3);
}

/// A plan nobody has touched says to leave everything exactly as it is, and
/// that is what it does. Whether the hashes come out the same is not asserted
/// and is not interesting: a commit is its author, its committer, its message,
/// its tree and its parent, and the only one of those that moves here is the
/// committer's clock. Within the same second it is the same commit.
#[test]
fn keeping_everything_leaves_the_history_saying_the_same_things() {
    let (temp, first) = three("plan-keep-all");
    let before = subjects(&temp);

    let plan = temp.open().plan_from(first).unwrap();
    temp.open().rewrite(&plan).unwrap();

    assert_eq!(subjects(&temp), before, "the same things in the same order");
    assert!(temp.open().status().unwrap().is_clean());
    assert_eq!(read(&temp, "b.txt"), "two\n");
    assert_eq!(read(&temp, "c.txt"), "three\n");
    assert_eq!(read(&temp, "d.txt"), "four\n");
}

#[test]
fn squashing_folds_a_commit_into_the_one_above_and_keeps_both_messages() {
    let (temp, first) = three("plan-squash");

    let mut plan = temp.open().plan_from(first).unwrap();
    plan.lines[1].step = Step::Squash;
    assert_eq!(plan.kept(), 2);

    temp.open().rewrite(&plan).unwrap();

    assert_eq!(subjects(&temp), vec!["feat: d", "feat: b", "feat: the base"]);
    // What the folded commit did is still there, in the one above it.
    assert_eq!(read(&temp, "c.txt"), "three\n");

    let walk = temp.open().commits(50).unwrap();
    let folded = temp.open().commit_detail(walk[1].id).unwrap();
    assert_eq!(folded.summary, "feat: b");
    assert!(folded.body.contains("feat: c"), "both messages: {}", folded.body);
}

#[test]
fn a_fixup_folds_it_in_and_throws_its_message_away() {
    let (temp, first) = three("plan-fixup");

    let mut plan = temp.open().plan_from(first).unwrap();
    plan.lines[1].step = Step::Fixup;
    temp.open().rewrite(&plan).unwrap();

    assert_eq!(subjects(&temp), vec!["feat: d", "feat: b", "feat: the base"]);
    assert_eq!(read(&temp, "c.txt"), "three\n", "what it did is kept");

    let walk = temp.open().commits(50).unwrap();
    let folded = temp.open().commit_detail(walk[1].id).unwrap();
    assert_eq!(folded.summary, "feat: b");
    assert!(!folded.body.contains("feat: c"), "but not what it said");
}

#[test]
fn dropping_a_commit_leaves_out_what_it_did() {
    let (temp, first) = three("plan-drop");

    let mut plan = temp.open().plan_from(first).unwrap();
    plan.lines[1].step = Step::Drop;
    assert_eq!(plan.kept(), 2);

    temp.open().rewrite(&plan).unwrap();

    assert_eq!(subjects(&temp), vec!["feat: d", "feat: b", "feat: the base"]);
    assert_eq!(read(&temp, "c.txt"), "", "and it is gone from the files");
    assert_eq!(read(&temp, "d.txt"), "four\n");
}

#[test]
fn rewording_changes_what_it_says_and_nothing_else() {
    let (temp, first) = three("plan-reword");

    let mut plan = temp.open().plan_from(first).unwrap();
    plan.lines[1].step = Step::Reword;
    plan.lines[1].message = "fix: c, which was never a feature".to_string();

    temp.open().rewrite(&plan).unwrap();

    assert_eq!(
        subjects(&temp),
        vec!["feat: d", "fix: c, which was never a feature", "feat: b", "feat: the base"]
    );
    assert_eq!(read(&temp, "c.txt"), "three\n", "what it did is untouched");
}

#[test]
fn moving_a_line_puts_the_commits_the_other_way_round() {
    let (temp, first) = three("plan-reorder");

    let mut plan = temp.open().plan_from(first).unwrap();
    plan.lines.swap(0, 1);

    temp.open().rewrite(&plan).unwrap();

    // Oldest first in the plan, newest first out of the walk.
    assert_eq!(
        subjects(&temp),
        vec!["feat: d", "feat: b", "feat: c", "feat: the base"]
    );
    assert_eq!(read(&temp, "b.txt"), "two\n");
    assert_eq!(read(&temp, "c.txt"), "three\n");
}

/// Said while arranging rather than after pressing: there is nothing above
/// the first one to fold it into, and that is a mistake made with the mouse.
#[test]
fn folding_the_first_one_into_nothing_is_refused_before_it_is_run() {
    let (temp, first) = three("plan-fold-nothing");

    let mut plan = temp.open().plan_from(first).unwrap();
    plan.lines[0].step = Step::Squash;

    let trouble = plan.trouble().expect("it has to say so");
    assert!(trouble.contains("nothing above it"), "{trouble}");

    let err = temp.open().rewrite(&plan).unwrap_err().to_string();
    assert!(err.contains("nothing above it"), "{err}");
}

/// The one that matters most. This is the only thing here that cannot be
/// stopped halfway, so a plan that cannot be carried out has to leave no
/// trace at all.
#[test]
fn a_plan_that_clashes_writes_nothing_and_says_which_commit() {
    let temp = TempRepo::new("plan-clash");
    commit(&temp, "a.txt", "one\n", "feat: the base");
    let first = commit(&temp, "a.txt", "two\n", "feat: two");
    commit(&temp, "a.txt", "three\n", "feat: three");

    let tip = temp.open().head_oid().unwrap();
    let before = subjects(&temp);

    // Leaving out the step in the middle means the one after it no longer
    // has anything to change.
    let mut plan = temp.open().plan_from(first).unwrap();
    plan.lines[0].step = Step::Drop;

    let err = temp.open().rewrite(&plan).unwrap_err().to_string();
    assert!(err.contains("clashes"), "{err}");
    assert!(err.contains("feat: three"), "it names the one that would not go: {err}");
    assert!(err.contains("Nothing was written"), "{err}");

    let repo = temp.open();
    assert_eq!(repo.head_oid().unwrap(), tip, "the branch did not move");
    assert_eq!(subjects(&temp), before);
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(read(&temp, "a.txt"), "three\n");
    assert_eq!(repo.half_done(), gitrust_core::Half::None);
}

#[test]
fn whoever_wrote_a_commit_still_wrote_it_afterwards() {
    let temp = TempRepo::new("plan-author");
    commit(&temp, "a.txt", "one\n", "feat: the base");

    // Written by somebody else, the way a commit that arrived from a remote
    // would have been.
    let first = {
        let raw = git2::Repository::open(temp.path()).unwrap();
        let theirs = git2::Signature::new(
            "Someone Else",
            "else@example.invalid",
            &git2::Time::new(1_600_000_000, 0),
        )
        .unwrap();
        let mine = raw.signature().unwrap();
        let head = raw.head().unwrap().peel_to_commit().unwrap();
        std::fs::write(temp.path().join("b.txt"), "two\n").unwrap();
        let mut index = raw.index().unwrap();
        index.add_path(std::path::Path::new("b.txt")).unwrap();
        index.write().unwrap();
        let tree = raw.find_tree(index.write_tree().unwrap()).unwrap();
        raw.commit(Some("HEAD"), &theirs, &mine, "feat: theirs", &tree, &[&head])
            .unwrap()
    };

    commit(&temp, "c.txt", "three\n", "feat: mine");

    let plan = temp.open().plan_from(first).unwrap();
    temp.open().rewrite(&plan).unwrap();

    let walk = temp.open().commits(50).unwrap();
    let theirs = walk.iter().find(|c| c.summary == "feat: theirs").unwrap();
    assert_eq!(theirs.author, "Someone Else", "it is still theirs");
}

#[test]
fn a_merge_inside_the_range_is_refused_rather_than_flattened() {
    let temp = TempRepo::new("plan-merge");
    commit(&temp, "a.txt", "one\n", "feat: the base");
    let trunk = temp.open().head_name();
    let first = commit(&temp, "b.txt", "two\n", "feat: b");

    temp.open().branch_here("side").unwrap();
    commit(&temp, "side.txt", "side\n", "feat: on the side");
    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "c.txt", "three\n", "feat: c");
    temp.open().merge("side").unwrap();

    let err = temp.open().plan_from(first).unwrap_err().to_string();
    assert!(err.contains("merge"), "{err}");
}

#[test]
fn the_very_first_commit_has_nothing_to_be_rebuilt_onto() {
    let temp = TempRepo::new("plan-root");
    let root = commit(&temp, "a.txt", "one\n", "feat: the base");
    commit(&temp, "b.txt", "two\n", "feat: b");

    let err = temp.open().plan_from(root).unwrap_err().to_string();
    assert!(err.contains("first commit"), "{err}");
}

#[test]
fn a_commit_that_is_not_on_this_branch_is_refused() {
    let temp = TempRepo::new("plan-elsewhere");
    commit(&temp, "a.txt", "one\n", "feat: the base");
    let trunk = temp.open().head_name();

    temp.open().branch_here("side").unwrap();
    let theirs = commit(&temp, "side.txt", "side\n", "feat: on the side");
    temp.open().checkout_branch(&trunk).unwrap();
    commit(&temp, "b.txt", "two\n", "feat: b");

    let err = temp.open().plan_from(theirs).unwrap_err().to_string();
    assert!(err.contains("not on"), "{err}");
}

#[test]
fn a_branch_that_moved_since_the_plan_was_made_is_refused() {
    let (temp, first) = three("plan-moved");
    let plan = temp.open().plan_from(first).unwrap();

    // Somebody commits while the screen is open.
    commit(&temp, "e.txt", "five\n", "feat: e");

    let err = temp.open().rewrite(&plan).unwrap_err().to_string();
    assert!(err.contains("has moved"), "{err}");
    assert_eq!(subjects(&temp).len(), 5, "and it did not run");
}

#[test]
fn nothing_may_be_rewritten_on_top_of_uncommitted_work() {
    let (temp, first) = three("plan-dirty");
    let plan = temp.open().plan_from(first).unwrap();

    temp.write("a.txt", "something I was in the middle of\n");

    let err = temp.open().rewrite(&plan).unwrap_err().to_string();
    assert!(err.contains("commit or stash"), "{err}");
    assert_eq!(
        read(&temp, "a.txt"),
        "something I was in the middle of\n",
        "and it is still there"
    );
}

#[test]
fn a_plan_says_what_it_would_do_before_it_is_run() {
    let (temp, first) = three("plan-says");
    let mut plan = temp.open().plan_from(first).unwrap();

    assert!(plan.says().contains("still 3"), "{}", plan.says());

    plan.lines[1].step = Step::Squash;
    assert_eq!(plan.says(), "3 commits become 2");

    plan.lines[0].step = Step::Drop;
    plan.lines[1].step = Step::Drop;
    plan.lines[2].step = Step::Drop;
    assert!(plan.says().contains("left out altogether"), "{}", plan.says());
}
