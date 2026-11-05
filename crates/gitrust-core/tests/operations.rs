//! Tests of what the action bar does: branch, stash and remote.
//!
//! The remote is a bare repository in another temporary directory. Talking to
//! it over a disk path walks the same road as a real one -- the refspecs, the
//! fast forward, the branch that starts tracking another -- without depending
//! on there being a network or credentials.

mod common;

use common::{temp_dir, TempRepo};
use gitrust_core::{
    layout, Lane, PullMode, PullRequest, PullRequestState, PullRequests, RefKind, Repo,
};

/// Leaves the repository with one commit and gives back its branch's name.
fn first_commit(temp: &TempRepo) -> String {
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: one").unwrap();
    repo.head_name()
}

// ------------------------------------------------------------- branches

#[test]
fn the_new_branch_takes_what_was_half_done_with_it() {
    let temp = TempRepo::new("branch");
    first_commit(&temp);

    temp.write("a.txt", "one\ntwo\n");
    temp.write("b.txt", "new\n");

    let repo = temp.open();
    let before = repo.head_id().unwrap();
    repo.branch_here("fix").unwrap();

    let repo = temp.open();
    assert_eq!(repo.head_name(), "fix");
    // The new branch points at the same place: no history has been written.
    assert_eq!(repo.head_id().unwrap(), before);

    // And what was half-done is still half-done, which is the only reason to
    // make a branch in the middle of a job.
    let state = repo.status().unwrap();
    let pending: Vec<&str> = state.unstaged.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(pending, ["a.txt", "b.txt"]);
}

#[test]
fn a_branch_with_no_name_is_not_made() {
    let temp = TempRepo::new("branch-no-name");
    first_commit(&temp);

    let repo = temp.open();
    let err = repo.branch_here("   ").unwrap_err().to_string();
    assert!(err.contains("needs a name"), "{err}");
}

#[test]
fn two_branches_with_the_same_name_do_not_fit() {
    let temp = TempRepo::new("branch-duplicate");
    first_commit(&temp);

    let repo = temp.open();
    repo.branch_here("two").unwrap();
    let err = temp.open().branch_here("two").unwrap_err().to_string();
    assert!(err.contains("two"), "{err}");
}

#[test]
fn switching_branch_brings_the_other_ones_files() {
    let temp = TempRepo::new("switch");
    let old = first_commit(&temp);

    temp.open().branch_here("other").unwrap();
    temp.write("b.txt", "only on the other\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();
    assert!(temp.path().join("b.txt").exists());

    let note = temp.open().checkout_branch(&old).unwrap();
    assert_eq!(note, format!("you are now on {old}"));

    let repo = temp.open();
    assert_eq!(repo.head_name(), old);
    // The file that only existed on the other branch is gone from disk, and
    // the tree is clean: the index heard about the change too.
    assert!(!temp.path().join("b.txt").exists());
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn changes_that_are_not_in_the_way_travel_with_you() {
    let temp = TempRepo::new("switch-with-work");
    let old = first_commit(&temp);

    temp.open().branch_here("other").unwrap();
    temp.write("b.txt", "only on the other\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    // a.txt is the same on both branches, so whatever anyone did to it clashes
    // with nothing and has to survive the switch.
    temp.write("a.txt", "one\nhalf-done\n");
    temp.open().checkout_branch(&old).unwrap();

    let repo = temp.open();
    assert_eq!(repo.head_name(), old);
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt")).unwrap(),
        "one\nhalf-done\n"
    );
    let state = repo.status().unwrap();
    assert_eq!(state.unstaged.len(), 1);
    assert_eq!(state.unstaged[0].path, "a.txt");
}

#[test]
fn with_clashing_changes_nothing_switches_not_even_halfway() {
    let temp = TempRepo::new("switch-clash");
    let old = first_commit(&temp);

    temp.open().branch_here("other").unwrap();
    temp.write("a.txt", "what another says\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a different").unwrap();

    // Now a.txt differs between the two branches and is half-done here as well.
    temp.write("a.txt", "what I am writing\n");

    let err = temp.open().checkout_branch(&old).unwrap_err().to_string();
    assert!(err.contains("clash with") && err.contains("Stash"), "{err}");

    // And nothing has moved: neither HEAD nor the file.
    let repo = temp.open();
    assert_eq!(repo.head_name(), "other");
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt")).unwrap(),
        "what I am writing\n"
    );
}

#[test]
fn switching_to_a_branch_that_does_not_exist_says_so() {
    let temp = TempRepo::new("switch-ghost");
    first_commit(&temp);

    let err = temp.open().checkout_branch("ghost").unwrap_err().to_string();
    assert!(err.contains("ghost"), "{err}");
}

// ---------------------------------------------------------------- stash

#[test]
fn stashing_leaves_the_tree_clean_and_popping_gives_it_back() {
    let temp = TempRepo::new("stash");
    first_commit(&temp);
    temp.write("a.txt", "one\ntwo\n");

    let mut repo = temp.open();
    assert!(!repo.status().unwrap().is_clean());

    repo.stash_save("half-done").unwrap();
    assert!(repo.status().unwrap().is_clean());

    let pile = repo.stashes().unwrap();
    assert_eq!(pile.len(), 1);
    assert!(pile[0].message.contains("half-done"));
    assert!(pile[0].time > 0);

    repo.stash_pop(0).unwrap();
    assert!(!repo.status().unwrap().is_clean());
    assert!(repo.stashes().unwrap().is_empty());
}

#[test]
fn applying_gives_the_changes_back_without_emptying_the_pile() {
    let temp = TempRepo::new("stash-apply");
    first_commit(&temp);
    temp.write("a.txt", "one\ntwo\n");

    let mut repo = temp.open();
    repo.stash_save("").unwrap();
    repo.stash_apply(0).unwrap();

    assert!(!repo.status().unwrap().is_clean());
    assert_eq!(repo.stashes().unwrap().len(), 1);
}

#[test]
fn dropping_a_stash_takes_it_off_the_pile_without_touching_the_tree() {
    let temp = TempRepo::new("stash-drop");
    first_commit(&temp);
    temp.write("a.txt", "one\ntwo\n");

    let mut repo = temp.open();
    repo.stash_save("surplus").unwrap();
    repo.stash_drop(0).unwrap();

    assert!(repo.stashes().unwrap().is_empty());
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn stashing_a_clean_tree_says_so_instead_of_failing_oddly() {
    let temp = TempRepo::new("stash-clean");
    first_commit(&temp);

    let mut repo = temp.open();
    let err = repo.stash_save("nothing").unwrap_err().to_string();
    assert_eq!(err, "there is nothing to stash");
}

// --------------------------------------------------------------- remote

/// A bare repository to push things to, set up as "origin".
fn with_remote(temp: &TempRepo, tag: &str) -> std::path::PathBuf {
    let bare = temp_dir(tag);
    git2::Repository::init_bare(&bare).unwrap();

    let repo = git2::Repository::open(temp.path()).unwrap();
    repo.remote("origin", &bare.display().to_string()).unwrap();
    bare
}

#[test]
fn with_no_remote_branch_there_are_no_counts_to_give() {
    let temp = TempRepo::new("no-remote");
    first_commit(&temp);
    assert!(temp.open().tracking().is_none());
}

#[test]
fn the_first_push_leaves_the_branch_tracking_the_remote() {
    let temp = TempRepo::new("push");
    let branch = first_commit(&temp);
    let bare = with_remote(&temp, "push-bare");

    let repo = temp.open();
    assert!(repo.tracking().is_none());
    repo.push().unwrap();

    let tracked = temp.open().tracking().expect("it should already track origin");
    assert_eq!(tracked.remote, format!("origin/{branch}"));
    assert_eq!((tracked.ahead, tracked.behind), (0, 0));

    // And the commit really is on the other side.
    let remote = git2::Repository::open(&bare).unwrap();
    assert_eq!(
        remote.find_reference(&format!("refs/heads/{branch}")).unwrap().target(),
        Some(repo.head_id().unwrap()),
    );

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn fetching_moves_the_branch_up_and_the_counts_say_so_first() {
    let origin = TempRepo::new("pull-origin");
    first_commit(&origin);
    let bare = with_remote(&origin, "pull-bare");
    origin.open().push().unwrap();

    // The other side: a copy that knows nothing yet of what is coming.
    let copy = temp_dir("pull-copy");
    git2::Repository::clone(&bare.display().to_string(), &copy).unwrap();

    origin.write("a.txt", "one\ntwo\n");
    let repo = origin.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: two").unwrap();
    repo.push().unwrap();
    let expected = repo.head_id().unwrap();

    let here = Repo::open(&copy).unwrap();
    // Before fetching, the counts are those of the last time they spoke.
    assert_eq!(here.tracking().map(|t| t.behind), Some(0));

    let note = here.fetch().unwrap();
    assert!(note.contains("1 commit"), "{note}");
    assert_eq!(Repo::open(&copy).unwrap().tracking().map(|t| t.behind), Some(1));

    let here = Repo::open(&copy).unwrap();
    here.pull(PullMode::FastForward).unwrap();
    assert_eq!(Repo::open(&copy).unwrap().head_id(), Some(expected));

    // And the file has reached the working tree, not just the reference.
    assert_eq!(std::fs::read_to_string(copy.join("a.txt")).unwrap(), "one\ntwo\n");

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copy);
}

#[test]
fn fetching_when_there_is_nothing_new_does_not_lie() {
    let temp = TempRepo::new("pull-uptodate");
    let branch = first_commit(&temp);
    let bare = with_remote(&temp, "pull-uptodate-bare");
    temp.open().push().unwrap();

    let note = temp.open().pull(PullMode::FastForward).unwrap();
    assert_eq!(note, format!("{branch} was already up to date"));

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn with_no_remote_configured_fetching_says_so_plainly() {
    let temp = TempRepo::new("pull-no-remote");
    first_commit(&temp);

    let err = temp.open().pull(PullMode::FastForward).unwrap_err().to_string();
    assert!(err.contains("does not track any remote branch"), "{err}");
}

#[test]
fn switching_to_a_remote_makes_the_local_one_and_leaves_it_tracking() {
    let temp = TempRepo::new("remote-branch");
    let old = first_commit(&temp);
    let bare = with_remote(&temp, "remote-bare");
    temp.open().push().unwrap();

    // A branch that lives only on the remote, for whoever comes next.
    temp.open().branch_here("release").unwrap();
    temp.write("c.txt", "from the release\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();
    repo.push().unwrap();

    let copy = temp_dir("remote-copy");
    git2::Repository::clone(&bare.display().to_string(), &copy).unwrap();
    assert_eq!(Repo::open(&copy).unwrap().head_name(), old);

    let note = Repo::open(&copy).unwrap().checkout_remote("origin/release").unwrap();
    assert_eq!(note, "you are now on release");

    let repo = Repo::open(&copy).unwrap();
    assert_eq!(repo.head_name(), "release");
    assert_eq!(repo.tracking().map(|t| t.remote), Some("origin/release".to_string()));
    assert!(copy.join("c.txt").exists());

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copy);
}

#[test]
fn if_the_local_one_exists_you_go_to_it_untouched() {
    let temp = TempRepo::new("remote-existing");
    let old = first_commit(&temp);
    let bare = with_remote(&temp, "remote-existing-bare");
    temp.open().push().unwrap();

    temp.open().branch_here("release").unwrap();
    temp.write("c.txt", "from the release\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    let theirs = repo.commit("feat: c").unwrap();
    repo.push().unwrap();
    temp.open().checkout_branch(&old).unwrap();

    // The local one exists and points where it points: going to the remote is going to it.
    temp.open().checkout_remote("origin/release").unwrap();
    let repo = temp.open();
    assert_eq!(repo.head_name(), "release");
    assert_eq!(repo.head_id(), Some(theirs));

    let _ = std::fs::remove_dir_all(&bare);
}

// ------------------------------------------------------------------ tags

#[test]
fn a_tag_with_no_message_comes_out_lightweight_and_with_one_annotated() {
    let temp = TempRepo::new("tags");
    first_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();

    repo.tag("v1.0", commit, "").unwrap();
    repo.tag("v2.0", commit, "The first real release").unwrap();

    let raw = git2::Repository::open(temp.path()).unwrap();

    // The lightweight one is a plain reference: it points at the commit.
    let lightweight = raw.find_reference("refs/tags/v1.0").unwrap();
    assert_eq!(lightweight.target(), Some(commit));
    assert!(raw.find_tag(lightweight.target().unwrap()).is_err());

    // The annotated one is an object of its own, with its message and who put it there.
    let annotated = raw.find_reference("refs/tags/v2.0").unwrap();
    let object = raw.find_tag(annotated.target().unwrap()).unwrap();
    assert_eq!(object.message(), Some("The first real release"));
    assert_eq!(object.tagger().unwrap().name(), Some("gitrust test"));
    assert_eq!(object.target_id(), commit);
}

#[test]
fn both_kinds_show_in_the_references_and_on_their_commit() {
    let temp = TempRepo::new("tags-refs");
    first_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();
    repo.tag("v1.0", commit, "").unwrap();
    repo.tag("v2.0", commit, "annotated").unwrap();

    let repo = temp.open();
    let refs = repo.references().unwrap();
    let tags: Vec<&str> = refs
        .iter()
        .filter(|entry| entry.kind == RefKind::Tag)
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(tags, ["v1.0", "v2.0"]);

    // And in the graph they hang off the commit, the annotated one included:
    // peeling it is what keeps it from floating in its own object.
    let graph = layout(repo.commits(10).unwrap());
    let names: Vec<&str> = graph.rows[0]
        .commit
        .refs
        .iter()
        .filter(|label| label.kind == RefKind::Tag)
        .map(|label| label.name.as_str())
        .collect();
    assert_eq!(names, ["v1.0", "v2.0"]);
}

#[test]
fn a_tag_does_not_walk_over_another_with_its_name() {
    let temp = TempRepo::new("tags-duplicate");
    first_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();

    repo.tag("v1.0", commit, "").unwrap();
    let err = repo.tag("v1.0", commit, "").unwrap_err().to_string();
    assert!(err.contains("v1.0"), "{err}");
}

#[test]
fn deleting_a_tag_does_not_touch_the_commit() {
    let temp = TempRepo::new("tags-delete");
    first_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();
    repo.tag("v1.0", commit, "with a message").unwrap();

    repo.delete_tag("v1.0").unwrap();

    let repo = temp.open();
    assert!(!repo.references().unwrap().iter().any(|e| e.kind == RefKind::Tag));
    assert_eq!(repo.head_id(), Some(commit));
}

#[test]
fn a_tag_with_no_name_is_not_made() {
    let temp = TempRepo::new("tags-no-name");
    first_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();

    let err = repo.tag("  ", commit, "").unwrap_err().to_string();
    assert!(err.contains("needs a name"), "{err}");
}

// --------------------------------------------------- deleting branches

#[test]
fn deleting_a_branch_does_not_touch_the_commits_it_shares() {
    let temp = TempRepo::new("delete-branch");
    let old = first_commit(&temp);
    let before = temp.open().head_id().unwrap();

    temp.open().branch_here("surplus").unwrap();
    temp.open().checkout_branch(&old).unwrap();

    // It has nothing of its own: it was born where everything else was.
    assert_eq!(temp.open().unmerged("surplus").unwrap(), 0);
    temp.open().delete_branch("surplus").unwrap();

    let repo = temp.open();
    assert!(!repo.references().unwrap().iter().any(|e| e.name == "surplus"));
    assert_eq!(repo.head_id(), Some(before));
}

#[test]
fn the_count_says_what_would_be_lost() {
    let temp = TempRepo::new("delete-count");
    let old = first_commit(&temp);

    temp.open().branch_here("theirs").unwrap();
    for n in 1..=2 {
        temp.write("b.txt", &format!("version {n}\n"));
        let repo = temp.open();
        repo.stage("b.txt").unwrap();
        repo.commit(&format!("feat: b{n}")).unwrap();
    }
    temp.open().checkout_branch(&old).unwrap();

    // Two commits that are nowhere else.
    assert_eq!(temp.open().unmerged("theirs").unwrap(), 2);

    // And as soon as a tag reaches them, they are no longer alone.
    let repo = temp.open();
    let tip = repo
        .references()
        .unwrap()
        .iter()
        .find(|e| e.name == "theirs")
        .unwrap()
        .target;
    repo.tag("saved", tip, "").unwrap();
    assert_eq!(temp.open().unmerged("theirs").unwrap(), 0);
}

#[test]
fn the_branch_you_are_on_is_not_deleted() {
    let temp = TempRepo::new("delete-current");
    let actual = first_commit(&temp);

    let err = temp.open().delete_branch(&actual).unwrap_err().to_string();
    assert!(err.contains("branch you are on"), "{err}");
}

#[test]
fn deleting_a_branch_that_does_not_exist_says_so() {
    let temp = TempRepo::new("delete-ghost");
    first_commit(&temp);
    let err = temp.open().delete_branch("ghost").unwrap_err().to_string();
    assert!(err.contains("ghost"), "{err}");
}

#[test]
fn deleting_on_the_remote_takes_it_off_there_and_here() {
    let temp = TempRepo::new("delete-remote");
    let old = first_commit(&temp);
    let bare = with_remote(&temp, "delete-remote-bare");
    temp.open().push().unwrap();

    temp.open().branch_here("release").unwrap();
    temp.write("c.txt", "from the release\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();
    repo.push().unwrap();
    temp.open().checkout_branch(&old).unwrap();

    let remote = git2::Repository::open(&bare).unwrap();
    assert!(remote.find_reference("refs/heads/release").is_ok());

    temp.open().delete_remote_branch("origin/release").unwrap();

    // It is no longer there, nor is the trace of it here.
    let remote = git2::Repository::open(&bare).unwrap();
    assert!(remote.find_reference("refs/heads/release").is_err());

    let repo = temp.open();
    let refs = repo.references().unwrap();
    assert!(!refs.iter().any(|e| e.name == "origin/release"));
    // And the local one is still where it was: deleting there does not delete here.
    assert!(refs.iter().any(|e| e.name == "release"));

    let _ = std::fs::remove_dir_all(&bare);
}

// --------------------------------------------------------------- merging

/// Leaves `branch` with a commit of its own touching `file`, and goes back.
fn branch_with_work(temp: &TempRepo, branch: &str, file: &str, text: &str) -> String {
    let back = temp.open().head_name();
    temp.open().branch_here(branch).unwrap();
    temp.write(file, text);
    let repo = temp.open();
    repo.stage(file).unwrap();
    repo.commit(&format!("feat: {file} on {branch}")).unwrap();
    temp.open().checkout_branch(&back).unwrap();
    back
}

#[test]
fn having_not_moved_the_branch_only_moves_up() {
    let temp = TempRepo::new("merge-fast");
    first_commit(&temp);
    branch_with_work(&temp, "theirs", "b.txt", "from the other\n");

    let before = temp.open().commits(50).unwrap().len();
    let note = temp.open().merge("theirs").unwrap();
    assert!(note.contains("no merge to write"), "{note}");

    let repo = temp.open();
    // There is no merge commit: the history is the same, only longer.
    assert_eq!(repo.commits(50).unwrap().len(), before);
    assert!(temp.path().join("b.txt").exists());
    assert_eq!(repo.commit_detail(repo.head_id().unwrap()).unwrap().parents.len(), 1);
}

#[test]
fn two_different_histories_join_in_a_commit_with_two_parents() {
    let temp = TempRepo::new("merge-real");
    first_commit(&temp);
    branch_with_work(&temp, "theirs", "b.txt", "from the other\n");

    // And here, meanwhile, something else in another file.
    temp.write("c.txt", "from mine\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    let note = temp.open().merge("theirs").unwrap();
    assert!(note.contains("merged into"), "{note}");

    let repo = temp.open();
    let head = repo.commit_detail(repo.head_id().unwrap()).unwrap();
    assert_eq!(head.parents.len(), 2);
    assert!(head.summary.starts_with("Merge theirs"));

    // Both files are on disk, and the tree is clean.
    assert!(temp.path().join("b.txt").exists());
    assert!(temp.path().join("c.txt").exists());
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn a_clash_starts_the_merge_rather_than_writing_it() {
    let temp = TempRepo::new("merge-clash");
    first_commit(&temp);
    branch_with_work(&temp, "theirs", "a.txt", "what the other one says\n");

    temp.write("a.txt", "what I say\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    let mine = repo.commit("feat: a my way").unwrap();

    // This used to refuse and touch nothing, because there was nothing in
    // the application that could get anybody out of a half-done merge. There
    // is now, so it really starts one.
    let said = temp.open().merge("theirs").unwrap();
    assert!(said.contains("a.txt"), "it says what clashes: {said}");

    let repo = temp.open();
    // No commit has been written: that is still waiting on a decision.
    assert_eq!(repo.head_id(), Some(mine));
    assert_eq!(repo.half_done(), gitrust_core::Half::Merge);
    assert_eq!(repo.clashes().unwrap().len(), 1);

    // And the file in front of you has both answers in it.
    let text = std::fs::read_to_string(temp.path().join("a.txt")).unwrap();
    assert!(text.contains("<<<<<<<"), "{text}");
    assert!(text.contains("what I say"), "{text}");
    assert!(text.contains("what the other one says"), "{text}");
}

#[test]
fn merging_with_a_half_done_tree_stops_before_it_starts() {
    let temp = TempRepo::new("merge-dirty");
    first_commit(&temp);
    branch_with_work(&temp, "theirs", "b.txt", "from the other\n");

    temp.write("c.txt", "from mine\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    // Unsaved work on top of a merge that does have to be written.
    temp.write("a.txt", "one\nhalf-done\n");

    let err = temp.open().merge("theirs").unwrap_err().to_string();
    assert!(err.contains("stash what you have"), "{err}");
    assert!(!temp.open().status().unwrap().is_clean(), "still half-done");
}

#[test]
fn merging_what_is_already_inside_writes_nothing() {
    let temp = TempRepo::new("merge-uptodate");
    let old = first_commit(&temp);
    temp.open().branch_here("theirs").unwrap();
    temp.open().checkout_branch(&old).unwrap();

    let note = temp.open().merge("theirs").unwrap();
    assert!(note.contains("already inside"), "{note}");
    assert_eq!(temp.open().commits(50).unwrap().len(), 1);
}

#[test]
fn a_branch_is_not_merged_with_itself() {
    let temp = TempRepo::new("merge-same");
    let actual = first_commit(&temp);
    let err = temp.open().merge(&actual).unwrap_err().to_string();
    assert!(err.contains("branch you are on"), "{err}");
}

// ---------------------------------------------------- forcing and tags

#[test]
fn forcing_takes_the_redone_history_to_the_remote() {
    let temp = TempRepo::new("force");
    let branch = first_commit(&temp);
    let bare = with_remote(&temp, "force-bare");
    temp.open().push().unwrap();

    // What had already gone out is redone: the remote and this stop lining up.
    let redone = temp.open().amend("feat: one, better told").unwrap();
    assert!(temp.open().push().is_err(), "without forcing it should not be able to");

    let note = temp.open().push_force().unwrap();
    assert!(note.contains("forced onto"), "{note}");

    let remote = git2::Repository::open(&bare).unwrap();
    assert_eq!(
        remote.find_reference(&format!("refs/heads/{branch}")).unwrap().target(),
        Some(redone),
    );

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn forcing_refuses_if_the_remote_changed_since_the_last_visit() {
    let temp = TempRepo::new("force-foreign");
    first_commit(&temp);
    let bare = with_remote(&temp, "force-foreign-bare");
    temp.open().push().unwrap();

    // Somebody else writes to the remote without us hearing about it.
    let other = temp_dir("force-foreign-other");
    git2::Repository::clone(&bare.display().to_string(), &other).unwrap();
    {
        let raw = git2::Repository::open(&other).unwrap();
        let mut config = raw.config().unwrap();
        config.set_str("user.name", "Other").unwrap();
        config.set_str("user.email", "other@example.invalid").unwrap();
    }
    std::fs::write(other.join("theirs.txt"), "mine\n").unwrap();
    let repo = Repo::open(&other).unwrap();
    repo.stage("theirs.txt").unwrap();
    repo.commit("feat: theirs").unwrap();
    repo.push().unwrap();

    temp.open().amend("feat: one, my way").unwrap();
    let err = temp.open().push_force().unwrap_err().to_string();
    assert!(err.contains("has changed since"), "{err}");

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn tags_go_out_once_and_are_not_sent_again() {
    let temp = TempRepo::new("tags-push");
    first_commit(&temp);
    let bare = with_remote(&temp, "tags-push-bare");
    temp.open().push().unwrap();

    let repo = temp.open();
    let commit = repo.head_id().unwrap();
    repo.tag("v1.0", commit, "").unwrap();
    repo.tag("v1.1", commit, "annotated").unwrap();

    let note = temp.open().push_tags().unwrap();
    assert!(note.contains("2 tags sent"), "{note}");

    let remote = git2::Repository::open(&bare).unwrap();
    assert!(remote.find_reference("refs/tags/v1.0").is_ok());
    assert!(remote.find_reference("refs/tags/v1.1").is_ok());

    // The second time there is nothing to send, and it says so.
    let note = temp.open().push_tags().unwrap();
    assert!(note.contains("already has every tag"), "{note}");

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn with_no_tags_no_push_is_invented() {
    let temp = TempRepo::new("tags-push-empty");
    first_commit(&temp);
    let bare = with_remote(&temp, "tags-push-empty-bare");
    temp.open().push().unwrap();

    let note = temp.open().push_tags().unwrap();
    assert!(note.contains("already has every tag"), "{note}");

    let _ = std::fs::remove_dir_all(&bare);
}

// ------------------------------------------------------------- the board

/// That branch's card, or the test's panic.
fn card<'a>(board: &'a gitrust_core::Board, branch: &str) -> &'a gitrust_core::Card {
    board
        .cards
        .iter()
        .find(|card| card.branch == branch)
        .unwrap_or_else(|| panic!("no card for {branch} in {:?}", board.cards))
}

#[test]
fn a_brand_new_branch_is_to_do_and_with_commits_in_progress() {
    let temp = TempRepo::new("board");
    let trunk = first_commit(&temp);

    temp.open().branch_here("not-started").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    let board = temp.open().board().unwrap();
    assert_eq!(board.trunk, trunk);
    assert_eq!(card(&board, "not-started").lane, Lane::Todo);
    assert_eq!(card(&board, "not-started").ahead, 0);

    // The moment it has something of its own, it changes column on its own.
    temp.open().checkout_branch("not-started").unwrap();
    temp.write("b.txt", "something already\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    let board = temp.open().board().unwrap();
    let card = card(&board, "not-started");
    assert_eq!(card.lane, Lane::InProgress);
    assert_eq!(card.ahead, 1);
    assert!(card.current, "it is the one checked out");
    assert!(card.time > 0);
}

#[test]
fn pushed_and_unmerged_it_moves_to_in_review() {
    let temp = TempRepo::new("board-review");
    let trunk = first_commit(&temp);
    let bare = with_remote(&temp, "board-review-bare");
    temp.open().push().unwrap();

    temp.open().branch_here("proposal").unwrap();
    temp.write("b.txt", "for somebody to look at\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    // Before it leaves here, it is in progress.
    assert_eq!(card(&temp.open().board().unwrap(), "proposal").lane, Lane::InProgress);

    temp.open().push().unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    assert_eq!(card(&temp.open().board().unwrap(), "proposal").lane, Lane::InReview);

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn merged_into_the_main_branch_it_is_done() {
    let temp = TempRepo::new("board-done");
    let trunk = first_commit(&temp);

    temp.open().branch_here("finished").unwrap();
    temp.write("b.txt", "what was needed\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    // The main branch moves on by itself, so that the merge is a real one.
    temp.write("c.txt", "meanwhile\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    temp.open().merge("finished").unwrap();

    let board = temp.open().board().unwrap();
    let card = card(&board, "finished");
    assert_eq!(card.lane, Lane::Done);
    assert_eq!(card.ahead, 0);
    assert!(card.behind > 0);
}

#[test]
fn the_main_branch_is_not_a_card() {
    let temp = TempRepo::new("board-trunk");
    let trunk = first_commit(&temp);

    let board = temp.open().board().unwrap();
    assert_eq!(board.trunk, trunk);
    assert!(!board.cards.iter().any(|card| card.branch == trunk));
}

#[test]
fn the_project_comes_from_the_folder_in_the_name() {
    let temp = TempRepo::new("board-project");
    let trunk = first_commit(&temp);

    for branch in ["theme/colors", "theme/typography", "loose"] {
        temp.open().branch_here(branch).unwrap();
        temp.open().checkout_branch(&trunk).unwrap();
    }

    let board = temp.open().board().unwrap();
    assert_eq!(card(&board, "theme/colors").project.as_deref(), Some("theme"));
    assert_eq!(card(&board, "theme/colors").leaf(), "colors");
    assert_eq!(card(&board, "loose").project, None);
    assert_eq!(board.projects(), ["theme"]);
}

#[test]
fn the_columns_hold_every_card_and_none_of_them_twice() {
    let temp = TempRepo::new("board-columns");
    let trunk = first_commit(&temp);

    temp.open().branch_here("a").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();
    temp.open().branch_here("b").unwrap();
    temp.write("b.txt", "something\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    let board = temp.open().board().unwrap();
    let spread: usize = Lane::ALL.iter().map(|lane| board.lane(*lane).count()).sum();
    assert_eq!(spread, board.cards.len());
    assert_eq!(board.lane(Lane::Todo).count(), 1);
    assert_eq!(board.lane(Lane::InProgress).count(), 1);
}

#[test]
fn an_empty_branch_left_behind_is_still_to_do() {
    let temp = TempRepo::new("board-forgotten");
    let trunk = first_commit(&temp);

    temp.open().branch_here("someday").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    // The main branch moves on without it. It never did anything, so it has
    // finished nothing: being left behind is not being done.
    temp.write("c.txt", "meanwhile\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    let board = temp.open().board().unwrap();
    let card = card(&board, "someday");
    assert_eq!(card.lane, Lane::Todo);
    assert_eq!(card.ahead, 0);
    assert!(card.behind > 0, "left behind, but not done");
}

// ------------------------------------------------------ the board and gh

/// A pull request on that branch, without going near a network.
fn pull(branch: &str, state: PullRequestState, draft: bool) -> PullRequests {
    let mut pulls = PullRequests::new();
    pulls.insert(
        branch.to_string(),
        PullRequest {
            number: 7,
            state,
            draft,
            url: format!("https://example.invalid/pull/7#{branch}"),
        },
    );
    pulls
}

#[test]
fn a_squashed_branch_is_done_once_the_forge_has_been_asked() {
    let temp = TempRepo::new("board-squashed");
    let trunk = first_commit(&temp);

    // Squashing is what the graph cannot see: the branch's commit never
    // reaches the main branch, so nothing here ever swallowed it.
    branch_with_work(&temp, "squashed", "b.txt", "theirs\n");
    temp.write("b.txt", "theirs\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b, squashed onto the trunk").unwrap();
    assert_eq!(temp.open().head_name(), trunk);

    // On its own the graph can only say it is in progress.
    let board = temp.open().board().unwrap();
    assert_eq!(card(&board, "squashed").lane, Lane::InProgress);

    let pulls = pull("squashed", PullRequestState::Merged, false);
    let board = temp.open().board_with(Some(&pulls)).unwrap();
    let card = card(&board, "squashed");
    assert_eq!(card.lane, Lane::Done);
    assert_eq!(card.pull.as_ref().unwrap().number, 7);
    assert!(board.asked);
}

#[test]
fn asked_and_told_of_no_pull_request_a_pushed_branch_stays_in_progress() {
    let temp = TempRepo::new("board-asked");
    let trunk = first_commit(&temp);
    let bare = with_remote(&temp, "board-asked-bare");
    temp.open().push().unwrap();

    branch_with_work(&temp, "proposal", "b.txt", "for somebody to look at\n");
    temp.open().push_branch("proposal").unwrap();
    assert_eq!(temp.open().head_name(), trunk);

    // Unasked, being pushed is the only evidence there is, and it reads as
    // being in review. That is the guess this whole thing exists to replace.
    let board = temp.open().board().unwrap();
    assert_eq!(card(&board, "proposal").lane, Lane::InReview);
    assert!(!board.asked);

    let board = temp.open().board_with(Some(&PullRequests::new())).unwrap();
    let card = card(&board, "proposal");
    assert_eq!(card.lane, Lane::InProgress);
    assert!(card.pull.is_none());

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn an_open_pull_request_is_what_in_review_means_and_a_draft_is_not() {
    let temp = TempRepo::new("board-open-pull");
    first_commit(&temp);
    branch_with_work(&temp, "proposal", "b.txt", "for somebody to look at\n");

    let open = pull("proposal", PullRequestState::Open, false);
    let board = temp.open().board_with(Some(&open)).unwrap();
    assert_eq!(card(&board, "proposal").lane, Lane::InReview);

    let draft = pull("proposal", PullRequestState::Open, true);
    let board = temp.open().board_with(Some(&draft)).unwrap();
    assert_eq!(card(&board, "proposal").lane, Lane::InProgress);
    assert!(card(&board, "proposal").pull.as_ref().unwrap().draft);
}

// --------------------------------------------------------- the pull modes

/// Leaves the remote with a commit that is not here, and here one that is not
/// there: the two histories parted, which is when the mode matters.
fn parted_histories(temp: &TempRepo, tag: &str) -> std::path::PathBuf {
    let bare = with_remote(temp, tag);
    temp.open().push().unwrap();

    let other = temp_dir(&format!("{tag}-other"));
    git2::Repository::clone(&bare.display().to_string(), &other).unwrap();
    {
        let raw = git2::Repository::open(&other).unwrap();
        let mut config = raw.config().unwrap();
        config.set_str("user.name", "Other").unwrap();
        config.set_str("user.email", "other@example.invalid").unwrap();
    }
    std::fs::write(other.join("theirs.txt"), "their side\n").unwrap();
    let repo = Repo::open(&other).unwrap();
    repo.stage("theirs.txt").unwrap();
    repo.commit("feat: theirs").unwrap();
    repo.push().unwrap();
    let _ = std::fs::remove_dir_all(&other);

    temp.write("mine.txt", "mine\n");
    let repo = temp.open();
    repo.stage("mine.txt").unwrap();
    repo.commit("feat: mine").unwrap();

    bare
}

#[test]
fn with_the_histories_parted_merge_mode_writes_the_joining_commit() {
    let temp = TempRepo::new("pull-merge");
    first_commit(&temp);
    let bare = parted_histories(&temp, "pull-merge-bare");

    let note = temp.open().pull(PullMode::Merge).unwrap();
    assert!(note.contains("merged into"), "{note}");

    let repo = temp.open();
    let head = repo.commit_detail(repo.head_id().unwrap()).unwrap();
    assert_eq!(head.parents.len(), 2);
    assert!(temp.path().join("theirs.txt").exists());
    assert!(temp.path().join("mine.txt").exists());

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn rebase_mode_leaves_the_history_straight() {
    let temp = TempRepo::new("pull-rebase");
    first_commit(&temp);
    let bare = parted_histories(&temp, "pull-rebase-bare");

    let note = temp.open().pull(PullMode::Rebase).unwrap();
    assert!(note.contains("1 commit replayed"), "{note}");

    let repo = temp.open();
    let head = repo.commit_detail(repo.head_id().unwrap()).unwrap();
    // One single parent: there is no merge commit, mine went on top.
    assert_eq!(head.parents.len(), 1);
    assert_eq!(head.summary, "feat: mine");

    // And theirs is just below.
    let parent = repo.commit_detail(head.parents[0]).unwrap();
    assert_eq!(parent.summary, "feat: theirs");
    assert!(repo.status().unwrap().is_clean());

    let _ = std::fs::remove_dir_all(&bare);
}

/// Pulling with a rebase used to undo the whole thing at the first clash.
/// Now it stops there and waits, which is the same promise the merge path
/// makes: you are never left somewhere you have no way out of.
#[test]
fn a_rebase_that_clashes_stops_there_and_waits_to_be_decided() {
    let temp = TempRepo::new("pull-rebase-clash");
    first_commit(&temp);

    let bare = with_remote(&temp, "pull-rebase-clash-bare");
    temp.open().push().unwrap();

    // The two touch the same file in different ways.
    let other = temp_dir("pull-rebase-clash-other");
    git2::Repository::clone(&bare.display().to_string(), &other).unwrap();
    {
        let raw = git2::Repository::open(&other).unwrap();
        let mut config = raw.config().unwrap();
        config.set_str("user.name", "Other").unwrap();
        config.set_str("user.email", "other@example.invalid").unwrap();
    }
    std::fs::write(other.join("a.txt"), "what they say\n").unwrap();
    let repo = Repo::open(&other).unwrap();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a their way").unwrap();
    repo.push().unwrap();
    let _ = std::fs::remove_dir_all(&other);

    temp.write("a.txt", "what I say\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    let mine = repo.commit("feat: a my way").unwrap();

    let note = temp.open().pull(PullMode::Rebase).unwrap();
    assert!(note.contains("a.txt"), "it says what it clashes in: {note}");

    // Half-done, with the file waiting rather than put back.
    let repo = temp.open();
    assert_eq!(repo.half_done(), gitrust_core::Half::Rebase);
    let clashes = repo.clashes().unwrap();
    assert_eq!(clashes.len(), 1);
    assert_eq!(clashes[0].path, "a.txt");

    // Our commit is still ours to get back to, whichever way this is decided.
    temp.open().abandon().unwrap();
    let repo = temp.open();
    assert_eq!(repo.half_done(), gitrust_core::Half::None);
    assert_eq!(repo.head_id(), Some(mine));
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt")).unwrap(),
        "what I say\n"
    );

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn fetching_from_every_remote_needs_the_branch_to_track_none() {
    let temp = TempRepo::new("fetch-all");
    first_commit(&temp);
    let bare = with_remote(&temp, "fetch-all-bare");

    // Having never pushed, the branch tracks nothing and is fetched all the same.
    assert!(temp.open().fetch().is_err());
    let note = temp.open().fetch_all().unwrap();
    assert!(note.contains("origin"), "{note}");

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn with_no_remotes_there_is_nowhere_to_fetch_from() {
    let temp = TempRepo::new("fetch-all-empty");
    first_commit(&temp);
    let err = temp.open().fetch_all().unwrap_err().to_string();
    assert!(err.contains("no remotes"), "{err}");
}

#[test]
fn bringing_a_branch_into_the_main_one_switches_to_it_first() {
    let temp = TempRepo::new("land");
    let trunk = first_commit(&temp);

    temp.open().branch_here("theirs").unwrap();
    temp.write("b.txt", "theirs\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    // Standing on "theirs", bringing it into the main one means switching first.
    assert_eq!(temp.open().head_name(), "theirs");
    let note = temp.open().land("theirs").unwrap();
    assert!(note.contains(&format!("you are now on {trunk}")), "{note}");

    let repo = temp.open();
    assert_eq!(repo.head_name(), trunk);
    assert!(temp.path().join("b.txt").exists());
}

#[test]
fn pushing_a_branch_you_are_not_on() {
    let temp = TempRepo::new("push-branch");
    let trunk = first_commit(&temp);
    let bare = with_remote(&temp, "push-branch-bare");
    temp.open().push().unwrap();

    temp.open().branch_here("other").unwrap();
    temp.write("b.txt", "theirs\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    let theirs = repo.commit("feat: b").unwrap();
    temp.open().checkout_branch(&trunk).unwrap();

    // From the main branch, the other is brought in without moving.
    //
    // What it says matters as much as what it does. The trunk is in step with
    // its remote, and for a while this counted the trunk's distance rather
    // than the pushed branch's, so a push that carried a commit announced that
    // it had carried none.
    let said = temp.open().push_branch("other").unwrap();
    assert!(!said.contains("0 commits"), "{said}");
    assert!(said.contains("other"), "{said}");
    assert_eq!(temp.open().head_name(), trunk);

    // Pushed again with nothing new, now that it follows something, it can
    // say so with a number.
    let again = temp.open().push_branch("other").unwrap();
    assert!(again.contains("0 commits"), "{again}");

    let remote = git2::Repository::open(&bare).unwrap();
    assert_eq!(remote.find_reference("refs/heads/other").unwrap().target(), Some(theirs));

    let err = temp.open().push_branch("ghost").unwrap_err().to_string();
    assert!(err.contains("ghost"), "{err}");

    let _ = std::fs::remove_dir_all(&bare);
}

// ------------------------------------------------------------- the backlog

#[test]
fn an_item_written_down_comes_back_with_the_branch_it_would_become() {
    let temp = TempRepo::new("backlog-add");
    first_commit(&temp);

    let repo = temp.open();
    repo.backlog_add("The footer breaks at 320px", Some("fix")).unwrap();

    let backlog = temp.open().backlog().unwrap();
    assert_eq!(backlog.items.len(), 1);
    assert_eq!(backlog.items[0].title, "The footer breaks at 320px");
    assert_eq!(backlog.items[0].project.as_deref(), Some("fix"));
    assert_eq!(backlog.items[0].branch(), "fix/the-footer-breaks-at-320px");
}

#[test]
fn the_backlog_is_invisible_to_the_history_and_to_the_references() {
    let temp = TempRepo::new("backlog-hidden");
    first_commit(&temp);

    let repo = temp.open();
    repo.backlog_add("Dark mode", None).unwrap();
    repo.backlog_add("Keyboard shortcuts", None).unwrap();

    // Two commits were written on refs/gitrust/backlog, and the graph has to
    // go on showing the one commit the repository actually has.
    let repo = temp.open();
    assert_eq!(repo.commits(100).unwrap().len(), 1);

    let names: Vec<String> = repo.references().unwrap().into_iter().map(|r| r.name).collect();
    assert!(
        !names.iter().any(|name| name.contains("gitrust") || name.contains("backlog")),
        "{names:?}"
    );
}

#[test]
fn starting_an_item_makes_its_branch_and_empties_its_place() {
    let temp = TempRepo::new("backlog-start");
    first_commit(&temp);

    let repo = temp.open();
    repo.backlog_add("Dark mode", Some("theme")).unwrap();
    let id = temp.open().backlog().unwrap().items[0].id.clone();

    let note = temp.open().backlog_start(&id).unwrap();
    assert!(note.contains("theme/dark-mode"), "{note}");

    let repo = temp.open();
    assert_eq!(repo.head_name(), "theme/dark-mode");
    assert!(repo.backlog().unwrap().items.is_empty());
}

#[test]
fn an_item_whose_name_is_taken_is_not_thrown_away() {
    let temp = TempRepo::new("backlog-clash");
    first_commit(&temp);

    let repo = temp.open();
    repo.branch_here("dark-mode").unwrap();
    temp.open().backlog_add("Dark mode", None).unwrap();

    let id = temp.open().backlog().unwrap().items[0].id.clone();
    let err = temp.open().backlog_start(&id).unwrap_err().to_string();
    assert!(err.contains("dark-mode"), "{err}");

    // The branch could not be made, so the note is still where it was.
    assert_eq!(temp.open().backlog().unwrap().items.len(), 1);
}

#[test]
fn a_title_a_branch_could_not_be_named_after_is_refused_outright() {
    let temp = TempRepo::new("backlog-unnameable");
    first_commit(&temp);

    // Nothing in it a branch name can be built from, so it is turned down
    // where it is typed rather than becoming a branch called nothing.
    let err = temp.open().backlog_add("¿¡—…!?", None).unwrap_err().to_string();
    assert!(err.contains("branch could be called"), "{err}");
    assert!(temp.open().backlog().unwrap().items.is_empty());
}

#[test]
fn the_backlog_travels_to_everyone_who_has_the_repository() {
    let mine = TempRepo::new("backlog-mine");
    let branch = first_commit(&mine);
    let bare = with_remote(&mine, "backlog-bare");
    mine.open().push().unwrap();

    mine.open().backlog_add("The footer breaks at 320px", None).unwrap();
    assert_eq!(mine.open().backlog().unwrap().unshared, 1);
    mine.open().backlog_share().unwrap();
    assert_eq!(mine.open().backlog().unwrap().unshared, 0);

    // Somebody else's copy of the same repository, which has never seen it.
    let yours = temp_dir("backlog-yours");
    let _ = std::fs::remove_dir_all(&yours);
    git2::Repository::clone(&bare.display().to_string(), &yours).unwrap();

    let theirs = Repo::open(&yours).unwrap();
    assert!(theirs.backlog().unwrap().items.is_empty(), "a clone fetches branches only");

    theirs.fetch_all().unwrap();
    let items = Repo::open(&yours).unwrap().backlog().unwrap().items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "The footer breaks at 320px");

    assert_eq!(branch, mine.open().head_name());
    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&yours);
}

#[test]
fn taking_an_item_out_is_as_unshared_as_putting_one_in() {
    let mine = TempRepo::new("backlog-drop");
    first_commit(&mine);
    let bare = with_remote(&mine, "backlog-drop-bare");
    mine.open().push().unwrap();

    // Never having written one is the only state with nothing to send.
    assert_eq!(mine.open().backlog().unwrap().unshared, 0);

    mine.open().backlog_add("Dark mode", None).unwrap();
    mine.open().backlog_share().unwrap();

    let id = mine.open().backlog().unwrap().items[0].id.clone();
    let note = mine.open().backlog_remove(&id).unwrap();
    assert!(note.contains("Dark mode"), "{note}");

    // Empty here, but the remote still has it, so there is still something to
    // say. Counting this as nothing would leave the other side holding an item
    // nobody can see has gone.
    let backlog = mine.open().backlog().unwrap();
    assert!(backlog.items.is_empty());
    assert_eq!(backlog.unshared, 1);

    mine.open().backlog_share().unwrap();
    assert_eq!(mine.open().backlog().unwrap().unshared, 0);

    let err = mine.open().backlog_remove(&id).unwrap_err().to_string();
    assert!(err.contains("no longer in the backlog"), "{err}");

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn an_item_taken_out_here_is_gone_for_everybody() {
    let mine = TempRepo::new("backlog-gone-mine");
    first_commit(&mine);
    let bare = with_remote(&mine, "backlog-gone-bare");
    mine.open().push().unwrap();
    mine.open().backlog_add("Dark mode", None).unwrap();
    mine.open().backlog_add("Conflict editor", None).unwrap();
    mine.open().backlog_share().unwrap();

    let yours = temp_dir("backlog-gone-yours");
    let _ = std::fs::remove_dir_all(&yours);
    git2::Repository::clone(&bare.display().to_string(), &yours).unwrap();
    Repo::open(&yours).unwrap().fetch_all().unwrap();
    assert_eq!(Repo::open(&yours).unwrap().backlog().unwrap().items.len(), 2);

    let id = mine
        .open()
        .backlog()
        .unwrap()
        .items
        .iter()
        .find(|item| item.title == "Dark mode")
        .expect("it should still be there")
        .id
        .clone();
    mine.open().backlog_remove(&id).unwrap();
    mine.open().backlog_share().unwrap();

    Repo::open(&yours).unwrap().fetch_all().unwrap();
    let titles: Vec<String> = Repo::open(&yours)
        .unwrap()
        .backlog()
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.title)
        .collect();
    assert_eq!(titles, ["Conflict editor"]);

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&yours);
}

#[test]
fn two_people_writing_down_two_different_things_do_not_clash() {
    let mine = TempRepo::new("backlog-merge-mine");
    first_commit(&mine);
    let bare = with_remote(&mine, "backlog-merge-bare");
    mine.open().push().unwrap();
    mine.open().backlog_add("Dark mode", None).unwrap();
    mine.open().backlog_share().unwrap();

    let yours = temp_dir("backlog-merge-yours");
    let _ = std::fs::remove_dir_all(&yours);
    git2::Repository::clone(&bare.display().to_string(), &yours).unwrap();
    let theirs = git2::Repository::open(&yours).unwrap();
    let mut config = theirs.config().unwrap();
    config.set_str("user.name", "someone else").unwrap();
    config.set_str("user.email", "else@example.invalid").unwrap();
    Repo::open(&yours).unwrap().fetch_all().unwrap();

    // Both of us write something down from the same starting point.
    mine.open().backlog_add("Keyboard shortcuts", None).unwrap();
    Repo::open(&yours).unwrap().backlog_add("Printing", None).unwrap();

    // Yours goes up first, so mine has to join the two rather than overwrite.
    Repo::open(&yours).unwrap().backlog_share().unwrap();
    mine.open().backlog_share().unwrap();

    let mut titles: Vec<String> =
        mine.open().backlog().unwrap().items.into_iter().map(|item| item.title).collect();
    titles.sort();
    assert_eq!(titles, ["Dark mode", "Keyboard shortcuts", "Printing"]);

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&yours);
}
