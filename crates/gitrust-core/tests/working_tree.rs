//! Tests of the stage -> commit cycle on a real, temporary repository.

mod common;

use std::fs;

use common::TempRepo;
use gitrust_core::{ChangeKind, HunkId, Repo};

#[test]
fn a_new_file_goes_from_untracked_to_staged() {
    let temp = TempRepo::new("stage");
    temp.write("a.txt", "hello\n");
    let repo = temp.open();

    let state = repo.status().unwrap();
    assert!(state.staged.is_empty());
    assert_eq!(state.unstaged.len(), 1);
    assert_eq!(state.unstaged[0].path, "a.txt");
    assert_eq!(state.unstaged[0].kind, ChangeKind::Untracked);

    repo.stage("a.txt").unwrap();

    let state = repo.status().unwrap();
    assert!(state.unstaged.is_empty());
    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.staged[0].kind, ChangeKind::Added);
}

#[test]
fn the_index_diff_shows_the_added_lines() {
    let temp = TempRepo::new("diff");
    temp.write("a.txt", "one\ntwo\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();

    let diff = repo.file_diff("a.txt", true).unwrap();
    assert!(!diff.binary);

    let (added, removed) = diff.counts();
    assert_eq!((added, removed), (2, 0));

    let lines: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter(|line| line.origin == '+')
        .map(|line| line.content.as_str())
        .collect();
    assert_eq!(lines, vec!["one", "two"]);
}

#[test]
fn committing_leaves_the_tree_clean_and_makes_history() {
    let temp = TempRepo::new("commit");
    temp.write("a.txt", "hello\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("first commit").unwrap();

    assert!(repo.status().unwrap().is_clean());

    let commits = repo.commits(10).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].summary, "first commit");
    assert!(commits[0].parents.is_empty());
    // HEAD and the branch point at the commit just made.
    assert!(commits[0].refs.len() >= 2);
}

#[test]
fn unstaging_puts_the_change_back_in_the_working_tree() {
    let temp = TempRepo::new("unstage");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    temp.write("a.txt", "one\ntwo\n");
    let state = repo.status().unwrap();
    assert_eq!(state.unstaged[0].kind, ChangeKind::Modified);

    repo.stage("a.txt").unwrap();
    let state = repo.status().unwrap();
    assert_eq!(state.staged[0].kind, ChangeKind::Modified);
    assert!(state.unstaged.is_empty());

    repo.unstage("a.txt").unwrap();
    let state = repo.status().unwrap();
    assert!(state.staged.is_empty(), "the index must be left empty");
    assert_eq!(state.unstaged[0].kind, ChangeKind::Modified);
}

#[test]
fn deleting_a_file_stages_as_a_deletion() {
    let temp = TempRepo::new("delete");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    fs::remove_file(temp.path().join("a.txt")).unwrap();
    repo.stage("a.txt").unwrap();

    let state = repo.status().unwrap();
    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.staged[0].kind, ChangeKind::Deleted);
}

/// Twenty numbered lines, with the ones named rewritten.
fn numbered(changed: &[u32]) -> String {
    (1..=20)
        .map(|n| {
            if changed.contains(&n) {
                format!("line {n} changed\n")
            } else {
                format!("line {n}\n")
            }
        })
        .collect()
}

/// Stages a file of twenty lines and commits it as a baseline.
fn repo_with_base(temp: &TempRepo) -> Repo {
    temp.write("a.txt", &numbered(&[]));
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();
    repo
}

fn has_line(diff: &gitrust_core::FileDiff, origin: char, needle: &str) -> bool {
    diff.hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .any(|line| line.origin == origin && line.content.contains(needle))
}

#[test]
fn staging_one_hunk_leaves_the_other_unstaged() {
    let temp = TempRepo::new("stage-hunk");
    let repo = repo_with_base(&temp);

    // Two changes far enough apart to fall in different hunks.
    temp.write("a.txt", &numbered(&[2, 18]));

    let pending = repo.file_diff("a.txt", false).unwrap();
    assert_eq!(pending.hunks.len(), 2, "two separate hunks were expected");

    repo.stage_hunk("a.txt", pending.hunks[0].id).unwrap();

    // The index takes only the first change.
    let staged = repo.file_diff("a.txt", true).unwrap();
    assert_eq!(staged.hunks.len(), 1);
    assert!(has_line(&staged, '+', "line 2 changed"));
    assert!(!has_line(&staged, '+', "line 18 changed"));

    // The second is still pending in the working tree.
    let left = repo.file_diff("a.txt", false).unwrap();
    assert_eq!(left.hunks.len(), 1);
    assert!(has_line(&left, '+', "line 18 changed"));

    // And the file shows up in both lists at once.
    let state = repo.status().unwrap();
    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.unstaged.len(), 1);
}

#[test]
fn unstaging_a_hunk_puts_it_back_in_the_working_tree() {
    let temp = TempRepo::new("unstage-hunk");
    let repo = repo_with_base(&temp);

    temp.write("a.txt", &numbered(&[2, 18]));
    repo.stage("a.txt").unwrap();

    let staged = repo.file_diff("a.txt", true).unwrap();
    assert_eq!(staged.hunks.len(), 2);
    assert!(repo.status().unwrap().unstaged.is_empty());

    repo.unstage_hunk("a.txt", staged.hunks[1].id).unwrap();

    // The first stays in the index; the second is pending again.
    let left_staged = repo.file_diff("a.txt", true).unwrap();
    assert_eq!(left_staged.hunks.len(), 1);
    assert!(has_line(&left_staged, '+', "line 2 changed"));

    let pending = repo.file_diff("a.txt", false).unwrap();
    assert_eq!(pending.hunks.len(), 1);
    assert!(has_line(&pending, '+', "line 18 changed"));

    // The file on disk is untouched: it still has both changes.
    let contents = fs::read_to_string(temp.path().join("a.txt")).unwrap();
    assert!(contents.contains("line 2 changed"));
    assert!(contents.contains("line 18 changed"));
}

#[test]
fn a_hunk_that_no_longer_exists_errors_instead_of_passing_quietly() {
    let temp = TempRepo::new("stale-hunk");
    let repo = repo_with_base(&temp);

    temp.write("a.txt", &numbered(&[2]));
    let diff = repo.file_diff("a.txt", false).unwrap();
    let hunk = diff.hunks[0].id;

    repo.stage_hunk("a.txt", hunk).unwrap();
    // It is already in the index: the second try does not find that hunk.
    assert!(repo.stage_hunk("a.txt", hunk).is_err());
}

#[test]
fn the_detail_splits_subject_and_body_and_lists_the_files() {
    let temp = TempRepo::new("detail");
    temp.write("a.txt", "one\n");
    temp.write("b.txt", "two\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: two files\n\nAnd an explanation\nin two lines.")
        .unwrap();

    let head = repo.commits(1).unwrap()[0].id;
    let detail = repo.commit_detail(head).unwrap();

    assert_eq!(detail.summary, "feat: two files");
    assert_eq!(detail.body, "And an explanation\nin two lines.");
    assert!(!detail.is_merge());

    let paths: Vec<&str> = detail.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["a.txt", "b.txt"]);
    assert!(detail.files.iter().all(|f| f.kind == ChangeKind::Added));
    assert_eq!(detail.counts(), (2, 0, 0));

    // And one of their diffs comes from the commit itself, not the tree.
    let diff = repo.commit_file_diff(head, "a.txt").unwrap();
    assert!(has_line(&diff, '+', "one"));
}

#[test]
fn the_references_mark_heads_branch() {
    let temp = TempRepo::new("refs");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    let refs = repo.references().unwrap();
    assert_eq!(refs.len(), 1, "only the initial branch");

    let branch = &refs[0];
    assert!(branch.is_head);
    assert_eq!(branch.kind, gitrust_core::RefKind::LocalBranch);
    // With no slashes in the name there is no folder and the leaf is the whole name.
    assert_eq!(branch.folder(), None);
    assert_eq!(branch.leaf(), branch.name);
}

#[test]
fn a_branch_with_slashes_is_grouped_by_its_folder() {
    let temp = TempRepo::new("folders");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    // Two branches with a prefix, like the ones any real repository uses.
    let git = git2::Repository::open(temp.path()).unwrap();
    let head = git.head().unwrap().peel_to_commit().unwrap();
    git.branch("feat/login", &head, false).unwrap();
    git.branch("feat/signup", &head, false).unwrap();

    let refs = repo.references().unwrap();
    let feat: Vec<_> = refs.iter().filter(|r| r.folder() == Some("feat")).collect();

    assert_eq!(feat.len(), 2);
    let leaves: Vec<&str> = feat.iter().map(|r| r.leaf()).collect();
    assert_eq!(leaves, vec!["login", "signup"]);
}

#[test]
fn a_commit_with_no_message_is_refused() {
    let temp = TempRepo::new("empty-msg");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();

    assert!(repo.commit("   ").is_err());
}

// --------------------------------------------------------------- amending

#[test]
fn amending_changes_the_message_without_writing_history() {
    let temp = TempRepo::new("amend-message");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    let old_one = repo.commit("feat: sbuject spelled wrong").unwrap();

    let new_file = temp.open().amend("feat: subject spelled right").unwrap();
    assert_ne!(new_file, old_one, "amending writes another commit, it does not edit the old one");

    let repo = temp.open();
    let detail = repo.commit_detail(new_file).unwrap();
    assert_eq!(detail.summary, "feat: subject spelled right");
    assert!(detail.parents.is_empty(), "the parent is still the same: none");
    assert_eq!(repo.commits(50).unwrap().len(), 1, "there is no extra commit");
}

#[test]
fn amending_puts_what_is_staged_into_the_last_commit() {
    let temp = TempRepo::new("amend-files");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    // The file that was forgotten, which is what amending is for.
    temp.write("b.txt", "two\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    let new_file = repo.amend("feat: a and b").unwrap();

    let repo = temp.open();
    let files: Vec<String> = repo
        .commit_detail(new_file)
        .unwrap()
        .files
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert_eq!(files, ["a.txt", "b.txt"]);
    assert_eq!(repo.commits(50).unwrap().len(), 1);
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn amending_keeps_the_author_and_changes_who_closes_it() {
    let temp = TempRepo::new("amend-authorship");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    let old_one = repo.commit("feat: a").unwrap();

    let when = {
        let raw = git2::Repository::open(temp.path()).unwrap();
        let seconds = raw.find_commit(old_one).unwrap().author().when().seconds();
        seconds
    };

    // Somebody else amends the first person's commit.
    {
        let raw = git2::Repository::open(temp.path()).unwrap();
        let mut config = raw.config().unwrap();
        config.set_str("user.name", "Other").unwrap();
        config.set_str("user.email", "other@example.invalid").unwrap();
    }

    let new_file = temp.open().amend("feat: a, better told").unwrap();

    let raw = git2::Repository::open(temp.path()).unwrap();
    let commit = raw.find_commit(new_file).unwrap();
    assert_eq!(commit.author().name(), Some("gitrust test"));
    assert_eq!(commit.author().when().seconds(), when);
    assert_eq!(commit.committer().name(), Some("Other"));
}

#[test]
fn with_no_commits_there_is_nothing_to_amend() {
    let temp = TempRepo::new("amend-empty");
    let err = temp.open().amend("feat: nothing").unwrap_err().to_string();
    assert!(err.contains("no commit to amend"), "{err}");
}

#[test]
fn amending_with_no_message_is_refused() {
    let temp = TempRepo::new("amend-no-message");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    assert!(temp.open().amend("   ").is_err());
}

#[test]
fn heads_message_comes_back_split_into_subject_and_body() {
    let temp = TempRepo::new("head-message");
    assert!(temp.open().head_message().is_none(), "with no commits there is no message");

    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: the subject\n\nAnd the body,\nin two lines.\n").unwrap();

    let (subject, body) = temp.open().head_message().unwrap();
    assert_eq!(subject, "feat: the subject");
    assert_eq!(body, "And the body,\nin two lines.");
}

// ------------------------------------------------------------- discarding

#[test]
fn discarding_puts_the_file_back_to_what_the_index_says() {
    let temp = TempRepo::new("discard");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    temp.write("a.txt", "one\nwhat I am making up\n");
    assert!(!temp.open().status().unwrap().is_clean());

    temp.open().discard("a.txt").unwrap();

    assert_eq!(fs::read_to_string(temp.path().join("a.txt")).unwrap(), "one\n");
    assert!(temp.open().status().unwrap().is_clean());
}

#[test]
fn discarding_keeps_what_was_already_staged() {
    let temp = TempRepo::new("discard-staged");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    // One part staged and another on top unstaged.
    temp.write("a.txt", "one\ntwo\n");
    temp.open().stage("a.txt").unwrap();
    temp.write("a.txt", "one\ntwo\nthree half-done\n");

    temp.open().discard("a.txt").unwrap();

    // It goes back to the index, not the commit: "two" survives because it was staged.
    assert_eq!(fs::read_to_string(temp.path().join("a.txt")).unwrap(), "one\ntwo\n");
    let state = temp.open().status().unwrap();
    assert!(state.unstaged.is_empty());
    assert_eq!(state.staged.len(), 1);
}

#[test]
fn discarding_an_untracked_file_deletes_it() {
    let temp = TempRepo::new("discard-new");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    temp.write("new.txt", "nobody knows it\n");
    assert!(temp.path().join("new.txt").exists());

    temp.open().discard("new.txt").unwrap();

    assert!(!temp.path().join("new.txt").exists());
    assert!(temp.open().status().unwrap().is_clean());
}

#[test]
fn discarding_a_deleted_file_puts_it_back_on_disk() {
    let temp = TempRepo::new("discard-deleted");
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    fs::remove_file(temp.path().join("a.txt")).unwrap();
    temp.open().discard("a.txt").unwrap();

    assert_eq!(fs::read_to_string(temp.path().join("a.txt")).unwrap(), "one\n");
}

#[test]
fn discarding_one_hunk_leaves_the_other_where_it_was() {
    let temp = TempRepo::new("discard-hunk");
    let repo = repo_with_base(&temp);

    // Two changes far from each other, which is what makes two hunks.
    temp.write("a.txt", &numbered(&[2, 18]));

    let diff = repo.file_diff("a.txt", false).unwrap();
    assert_eq!(diff.hunks.len(), 2, "this test needs two hunks");

    temp.open().discard_hunk("a.txt", diff.hunks[0].id).unwrap();

    let text = fs::read_to_string(temp.path().join("a.txt")).unwrap();
    assert!(text.contains("line 2\n"), "the first comes back: {text}");
    assert!(!text.contains("line 2 changed"), "and really: {text}");
    assert!(text.contains("line 18 changed"), "the second stays: {text}");
}

#[test]
fn discarding_a_hunk_of_a_new_file_says_to_discard_it_whole() {
    let temp = TempRepo::new("discard-hunk-new");
    repo_with_base(&temp);
    temp.write("new.txt", "just arrived\n");

    // An untracked file brings no hunks: the diff does not go into its
    // contents while Git knows nothing about it.
    let repo = temp.open();
    assert!(repo.file_diff("new.txt", false).unwrap().hunks.is_empty());

    let err = temp
        .open()
        .discard_hunk("new.txt", HunkId::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("discard it whole"), "{err}");
}
