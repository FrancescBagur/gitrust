//! Tests of the same repository checked out in more than one place.
//!
//! These make real worktrees on disk, because the whole point of the feature
//! is what happens on disk: a second folder, sharing one history, with its own
//! HEAD. A fake would prove nothing.

mod common;

use common::TempRepo;
use gitrust_core::Repo;

/// Leaves the repository with one commit and a branch to open elsewhere.
fn ready(temp: &TempRepo) -> Repo {
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: one").unwrap();
    repo.branch_here("fix/footer").unwrap();
    // Back to where we started, so fix/footer is nobody's HEAD.
    let trunk = repo.trunk().unwrap();
    repo.checkout_branch(&trunk).unwrap();
    temp.open()
}

/// Everything these tests make outside the repository's own folder.
fn sweep(repo: &Repo) {
    let _ = std::fs::remove_dir_all(repo.worktree_root());
}

#[test]
fn a_branch_opened_elsewhere_has_its_own_folder_and_keeps_the_history() {
    let temp = TempRepo::new("worktree-add");
    let repo = ready(&temp);

    let where_to = repo.worktree_path_for("fix/footer");
    assert!(!where_to.exists(), "saying where is not making it");

    let note = repo.add_worktree("fix/footer").unwrap();
    assert!(note.contains("fix/footer"), "{note}");
    assert!(where_to.is_dir());
    // The checkout is beside the repository, never inside it.
    assert!(!where_to.starts_with(temp.path()));

    let trees = temp.open().worktrees().unwrap();
    assert_eq!(trees.len(), 2);
    assert!(trees[0].main, "the one you opened comes first");
    assert_eq!(trees[1].branch.as_deref(), Some("fix/footer"));
    assert!(!trees[1].main && !trees[1].missing);

    // One repository, two folders: the history is the same from over there.
    let theirs = Repo::open(&where_to).unwrap();
    assert_eq!(theirs.head_name(), "fix/footer");
    assert_eq!(theirs.commits(100).unwrap().len(), 1);

    sweep(&repo);
}

#[test]
fn a_branch_can_only_be_open_in_one_place_and_it_says_which() {
    let temp = TempRepo::new("worktree-held");
    let repo = ready(&temp);
    repo.add_worktree("fix/footer").unwrap();

    let held = temp.open().held_by("fix/footer").expect("it is open somewhere");
    assert_eq!(held.branch.as_deref(), Some("fix/footer"));
    assert!(held.path.is_dir());

    let err = temp.open().add_worktree("fix/footer").unwrap_err().to_string();
    assert!(err.contains("already open"), "{err}");
    assert!(err.contains(&held.path.display().to_string()), "{err}");

    sweep(&repo);
}

#[test]
fn the_branch_you_are_on_is_already_open_where_you_are() {
    let temp = TempRepo::new("worktree-here");
    let repo = ready(&temp);
    let trunk = repo.trunk().unwrap();

    let err = repo.add_worktree(&trunk).unwrap_err().to_string();
    assert!(err.contains("already open here"), "{err}");
    assert!(temp.open().held_by(&trunk).is_none(), "here is not elsewhere");

    sweep(&repo);
}

#[test]
fn a_branch_that_is_not_there_is_refused_before_anything_is_made() {
    let temp = TempRepo::new("worktree-missing-branch");
    let repo = ready(&temp);

    let err = repo.add_worktree("nothing-like-this").unwrap_err().to_string();
    assert!(err.contains("no branch called"), "{err}");
    assert!(!repo.worktree_root().exists(), "nothing was written");

    sweep(&repo);
}

#[test]
fn what_is_uncommitted_over_there_is_counted_before_it_is_thrown_away() {
    let temp = TempRepo::new("worktree-dirty");
    let repo = ready(&temp);
    repo.add_worktree("fix/footer").unwrap();

    let name = temp.open().worktrees().unwrap()[1].name.clone();
    assert_eq!(temp.open().worktree_changes(&name).unwrap(), 0);

    let elsewhere = repo.worktree_path_for("fix/footer");
    std::fs::write(elsewhere.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(elsewhere.join("b.txt"), "new\n").unwrap();
    assert_eq!(temp.open().worktree_changes(&name).unwrap(), 2);

    sweep(&repo);
}

#[test]
fn removing_a_checkout_takes_the_folder_and_leaves_the_branch() {
    let temp = TempRepo::new("worktree-remove");
    let repo = ready(&temp);
    repo.add_worktree("fix/footer").unwrap();

    let elsewhere = repo.worktree_path_for("fix/footer");
    let name = temp.open().worktrees().unwrap()[1].name.clone();
    temp.open().remove_worktree(&name).unwrap();

    assert!(!elsewhere.exists());
    assert_eq!(temp.open().worktrees().unwrap().len(), 1);
    // The branch was never the thing being removed.
    let repo = temp.open();
    assert!(repo.held_by("fix/footer").is_none());
    assert!(repo.add_worktree("fix/footer").is_ok(), "it can be opened again");

    sweep(&repo);
}

#[test]
fn a_folder_deleted_by_hand_is_said_to_be_missing_and_then_forgotten() {
    let temp = TempRepo::new("worktree-prune");
    let repo = ready(&temp);
    repo.add_worktree("fix/footer").unwrap();

    // Somebody deletes the folder in a file manager, which happens.
    std::fs::remove_dir_all(repo.worktree_path_for("fix/footer")).unwrap();

    let trees = temp.open().worktrees().unwrap();
    assert!(trees[1].missing, "it says so rather than pretending");
    assert_eq!(trees[1].branch, None, "there is nothing left to ask");

    let note = temp.open().prune_worktrees().unwrap();
    assert!(note.contains("gone"), "{note}");
    assert_eq!(temp.open().worktrees().unwrap().len(), 1);

    sweep(&repo);
}

#[test]
fn switching_to_a_branch_that_is_open_elsewhere_says_where_it_is() {
    let temp = TempRepo::new("worktree-switch");
    let repo = ready(&temp);
    repo.add_worktree("fix/footer").unwrap();
    let trunk = repo.trunk().unwrap();

    let err = temp.open().checkout_branch("fix/footer").unwrap_err().to_string();
    assert!(err.contains("is open in"), "{err}");
    assert!(
        err.contains(&repo.worktree_path_for("fix/footer").display().to_string()),
        "{err}"
    );

    // And it stopped before touching anything: we are still where we were.
    assert_eq!(temp.open().head_name(), trunk);

    sweep(&repo);
}

#[test]
fn a_commit_made_over_there_is_in_the_history_here() {
    let temp = TempRepo::new("worktree-shared");
    let repo = ready(&temp);
    repo.add_worktree("fix/footer").unwrap();

    let elsewhere = repo.worktree_path_for("fix/footer");
    std::fs::write(elsewhere.join("a.txt"), "one\ntwo\n").unwrap();
    let theirs = Repo::open(&elsewhere).unwrap();
    theirs.stage("a.txt").unwrap();
    theirs.commit("fix: the footer at 320px").unwrap();

    // Two folders, one object database: the commit is here without fetching.
    let subjects: Vec<String> = temp
        .open()
        .commits(100)
        .unwrap()
        .into_iter()
        .map(|commit| commit.summary)
        .collect();
    assert!(subjects.contains(&"fix: the footer at 320px".to_string()), "{subjects:?}");

    // And the board sees it as work in progress on that branch.
    let board = temp.open().board().unwrap();
    let card = board.cards.iter().find(|card| card.branch == "fix/footer").unwrap();
    assert_eq!(card.ahead, 1);

    sweep(&repo);
}
