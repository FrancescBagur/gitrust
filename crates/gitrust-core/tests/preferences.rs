//! What the preferences screen changes, on the side of it that touches a
//! repository: whether a fetch prunes, what a new repository's branch is
//! called, the submodules, the `.orig` copies, and conflicts seen coming.
//!
//! The remotes are bare repositories in other temporary directories, as in
//! the tests of the action bar, and for the same reason.

mod common;

use std::path::{Path, PathBuf};

use common::{temp_dir, TempRepo};
use gitrust_core::{PullMode, Repo};

/// Leaves the repository with one commit and gives back its branch's name.
fn first_commit(temp: &TempRepo) -> String {
    temp.write("a.txt", "one\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: one").unwrap();
    repo.head_name()
}

/// A bare repository to push things to, set up as "origin".
fn with_remote(temp: &TempRepo, tag: &str) -> PathBuf {
    let bare = temp_dir(tag);
    git2::Repository::init_bare(&bare).unwrap();
    let repo = git2::Repository::open(temp.path()).unwrap();
    repo.remote("origin", &bare.display().to_string()).unwrap();
    bare
}

fn has_ref(path: &Path, name: &str) -> bool {
    git2::Repository::open(path).unwrap().find_reference(name).is_ok()
}

// --------------------------------------------------------------- pruning

/// An origin with a second branch on it, a copy that has fetched both, and
/// then the second branch deleted at the origin.
fn a_branch_gone_from_the_remote(tag: &str) -> (TempRepo, PathBuf, PathBuf) {
    let origin = TempRepo::new(&format!("{tag}-origin"));
    first_commit(&origin);
    let bare = with_remote(&origin, &format!("{tag}-bare"));
    origin.open().push().unwrap();
    origin.open().branch_here("gone").unwrap();
    origin.open().push().unwrap();

    let copy = temp_dir(&format!("{tag}-copy"));
    git2::Repository::clone(&bare.display().to_string(), &copy).unwrap();
    assert!(has_ref(&copy, "refs/remotes/origin/gone"));

    git2::Repository::open(&bare)
        .unwrap()
        .find_reference("refs/heads/gone")
        .unwrap()
        .delete()
        .unwrap();
    (origin, bare, copy)
}

#[test]
fn a_fetch_that_prunes_takes_away_the_branch_that_has_gone() {
    let (_origin, bare, copy) = a_branch_gone_from_the_remote("prune-on");

    Repo::open(&copy).unwrap().pruning(true).fetch_all().unwrap();
    assert!(!has_ref(&copy, "refs/remotes/origin/gone"));

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copy);
}

#[test]
fn a_fetch_that_does_not_prune_leaves_it_where_it_was() {
    let (_origin, bare, copy) = a_branch_gone_from_the_remote("prune-off");

    // Said here so that the test is about this program and not about
    // whether the machine running it has `fetch.prune` turned on.
    git2::Repository::open(&copy).unwrap().config().unwrap().set_bool("fetch.prune", false).unwrap();

    Repo::open(&copy).unwrap().fetch_all().unwrap();
    assert!(has_ref(&copy, "refs/remotes/origin/gone"));

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copy);
}

// ------------------------------------------------------- the quiet fetch

#[test]
fn the_quiet_fetch_says_whether_anything_came() {
    let origin = TempRepo::new("quiet-origin");
    first_commit(&origin);
    let bare = with_remote(&origin, "quiet-bare");
    origin.open().push().unwrap();

    let copy = temp_dir("quiet-copy");
    git2::Repository::clone(&bare.display().to_string(), &copy).unwrap();

    // Nothing has happened at the origin since the clone.
    assert!(!Repo::open(&copy).unwrap().fetch_quietly().unwrap());

    origin.write("a.txt", "one\ntwo\n");
    let repo = origin.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: two").unwrap();
    repo.push().unwrap();

    assert!(Repo::open(&copy).unwrap().fetch_quietly().unwrap());
    // And the second time round it has all arrived already.
    assert!(!Repo::open(&copy).unwrap().fetch_quietly().unwrap());

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copy);
}

#[test]
fn the_quiet_fetch_of_a_repository_with_no_remote_is_not_a_failure() {
    let temp = TempRepo::new("quiet-alone");
    first_commit(&temp);
    assert!(!temp.open().fetch_quietly().unwrap());
}

// ------------------------------------------------- the first branch's name

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("gitrust-prefs-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    path
}

#[test]
fn a_new_repository_starts_on_the_branch_the_preferences_name() {
    let path = scratch("trunk");
    gitrust_core::start::init(&path, Some("trunk")).unwrap();

    let head = std::fs::read_to_string(path.join(".git/HEAD")).unwrap();
    assert_eq!(head.trim(), "ref: refs/heads/trunk");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn an_empty_name_is_no_name_and_git_decides() {
    let named = scratch("named-by-git");
    gitrust_core::start::init(&named, None).unwrap();
    let blank = scratch("blank");
    gitrust_core::start::init(&blank, Some("  ")).unwrap();

    let head = |path: &Path| std::fs::read_to_string(path.join(".git/HEAD")).unwrap();
    assert_eq!(head(&named), head(&blank));
    let _ = std::fs::remove_dir_all(&named);
    let _ = std::fs::remove_dir_all(&blank);
}

#[test]
fn a_name_no_branch_can_have_is_refused_before_anything_is_made() {
    let path = scratch("bad-name");
    let err = gitrust_core::start::init(&path, Some("no..good")).unwrap_err().to_string();
    assert!(err.contains("not a name a branch can have"), "{err}");
    assert!(!path.exists(), "nothing was made");
}

#[test]
fn git_is_asked_what_a_branch_can_be_called() {
    use gitrust_core::start::is_branch_name;
    for good in ["main", "trunk", "release/2.0", "feat-login"] {
        assert!(is_branch_name(good), "{good}");
    }
    for bad in ["", "two words", "no..good", "ends.lock", "-flag", "a~b", "with:colon"] {
        assert!(!is_branch_name(bad), "{bad}");
    }
}

// ------------------------------------------------------------ .orig copies

#[test]
fn the_orig_copy_beside_a_file_goes_and_the_file_stays() {
    let temp = TempRepo::new("orig");
    first_commit(&temp);
    temp.write("a.txt.orig", "<<<<<<< ours\n");

    assert!(temp.open().drop_orig("a.txt").unwrap());
    assert!(!temp.path().join("a.txt.orig").exists());
    assert!(temp.path().join("a.txt").exists());

    // With none there it says so and does nothing.
    assert!(!temp.open().drop_orig("a.txt").unwrap());
}

#[test]
fn an_orig_file_the_repository_keeps_is_kept() {
    let temp = TempRepo::new("orig-tracked");
    first_commit(&temp);
    temp.write("a.txt.orig", "theirs to keep\n");
    let repo = temp.open();
    repo.stage("a.txt.orig").unwrap();
    repo.commit("chore: a file that happens to be called that").unwrap();

    assert!(!temp.open().drop_orig("a.txt").unwrap());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt.orig")).unwrap(),
        "theirs to keep\n"
    );
}

// ------------------------------------------------------ conflicts foreseen

/// A repository on `main` with one commit, whatever Git calls a first
/// branch on this machine.
fn on_main(tag: &str) -> TempRepo {
    let temp = TempRepo::new(tag);
    let branch = first_commit(&temp);
    if branch != "main" {
        temp.open().rename_branch(&branch, "main").unwrap();
    }
    temp
}

fn change(temp: &TempRepo, name: &str, contents: &str, message: &str) {
    temp.write(name, contents);
    let repo = temp.open();
    repo.stage(name).unwrap();
    repo.commit(message).unwrap();
}

#[test]
fn a_branch_that_has_everything_main_has_has_nothing_to_foresee() {
    let temp = on_main("foresee-ahead");
    temp.open().branch_here("feat").unwrap();
    change(&temp, "a.txt", "one\ntwo\n", "feat: two");

    assert_eq!(temp.open().target().map(|(name, _)| name).as_deref(), Some("main"));
    assert_eq!(temp.open().foresee().unwrap(), None);
}

#[test]
fn two_changes_that_fit_together_are_foreseen_as_clean() {
    let temp = on_main("foresee-clean");
    temp.open().branch_here("feat").unwrap();
    change(&temp, "b.txt", "theirs\n", "feat: another file");
    temp.open().checkout_branch("main").unwrap();
    change(&temp, "c.txt", "ours\n", "feat: a third file");
    temp.open().checkout_branch("feat").unwrap();

    let seen = temp.open().foresee().unwrap().expect("there is something to merge");
    assert_eq!(seen.target, "main");
    assert!(seen.files.is_empty());
    assert_eq!(seen.says(), None);
}

#[test]
fn the_same_line_changed_on_both_sides_is_foreseen_and_nothing_is_touched() {
    let temp = on_main("foresee-clash");
    temp.open().branch_here("feat").unwrap();
    change(&temp, "a.txt", "feat's\n", "feat: mine");
    temp.open().checkout_branch("main").unwrap();
    change(&temp, "a.txt", "main's\n", "feat: theirs");
    temp.open().checkout_branch("feat").unwrap();

    let before = temp.open().head_id();
    let seen = temp.open().foresee().unwrap().expect("there is something to merge");
    assert_eq!(seen.files, vec!["a.txt".to_string()]);
    assert_eq!(seen.says().unwrap(), "1 file would conflict with main");

    // It was a question. The branch, the tree and the index are where they
    // were, and nothing is half-done.
    let repo = temp.open();
    assert_eq!(repo.head_id(), before);
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(repo.half_done(), gitrust_core::conflict::Half::None);
    assert_eq!(std::fs::read_to_string(temp.path().join("a.txt")).unwrap(), "feat's\n");
}

#[test]
fn what_the_remote_calls_its_main_branch_is_the_target() {
    let origin = TempRepo::new("foresee-origin");
    let branch = first_commit(&origin);
    let bare = with_remote(&origin, "foresee-bare");
    origin.open().push().unwrap();
    // A bare repository made empty points at a branch that may not be the
    // one that was pushed. Say which it is, as a host does.
    git2::Repository::open(&bare).unwrap().set_head(&format!("refs/heads/{branch}")).unwrap();

    let copy = temp_dir("foresee-copy");
    git2::Repository::clone(&bare.display().to_string(), &copy).unwrap();
    let here = Repo::open(&copy).unwrap();
    here.branch_here("feat").unwrap();

    let (target, _) = Repo::open(&copy).unwrap().target().expect("origin has a main branch");
    assert_eq!(target, format!("origin/{branch}"));

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copy);
}

// -------------------------------------------------------------- submodules

/// A library, a project that has it as a submodule at `lib`, the bare
/// repository the project is pushed to, and a clone of that made the way a
/// plain `git clone` makes one: with the submodule recorded and not there.
struct WithSubmodule {
    library: TempRepo,
    project: TempRepo,
    bare: PathBuf,
    copy: PathBuf,
}

impl WithSubmodule {
    fn new(tag: &str) -> Self {
        let library = TempRepo::new(&format!("{tag}-library"));
        library.write("lib.txt", "one\n");
        let repo = library.open();
        repo.stage("lib.txt").unwrap();
        repo.commit("feat: a library").unwrap();

        let project = TempRepo::new(&format!("{tag}-project"));
        first_commit(&project);
        {
            let raw = git2::Repository::open(project.path()).unwrap();
            let mut sub = raw
                .submodule(&library.path().display().to_string(), Path::new("lib"), true)
                .unwrap();
            sub.clone(None).unwrap();
            sub.add_finalize().unwrap();
        }
        project.open().commit("feat: use the library").unwrap();

        let bare = with_remote(&project, &format!("{tag}-bare"));
        project.open().push().unwrap();

        let copy = temp_dir(&format!("{tag}-copy"));
        git2::Repository::clone(&bare.display().to_string(), &copy).unwrap();

        Self { library, project, bare, copy }
    }

    /// Moves the library on by a commit and records that in the project.
    fn move_the_library_on(&self) -> git2::Oid {
        self.library.write("lib.txt", "one\ntwo\n");
        let repo = self.library.open();
        repo.stage("lib.txt").unwrap();
        let moved = repo.commit("feat: more library").unwrap();

        let inside = git2::Repository::open(self.project.path().join("lib")).unwrap();
        inside.find_remote("origin").unwrap().fetch(&[] as &[&str], None, None).unwrap();
        inside.set_head_detached(moved).unwrap();
        inside
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();

        let project = self.project.open();
        project.stage("lib").unwrap();
        project.commit("chore: the newer library").unwrap();
        project.push().unwrap();
        moved
    }
}

impl Drop for WithSubmodule {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.bare);
        let _ = std::fs::remove_dir_all(&self.copy);
    }
}

#[test]
fn a_repository_with_no_submodules_has_none() {
    let temp = TempRepo::new("no-submodules");
    first_commit(&temp);
    assert!(temp.open().submodules().is_empty());
}

#[test]
fn a_submodule_that_was_never_brought_down_is_brought_down() {
    let set = WithSubmodule::new("sub-missing");

    let subs = Repo::open(&set.copy).unwrap().submodules();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].path, "lib");
    assert!(subs[0].missing() && subs[0].behind());
    assert!(!set.copy.join("lib/lib.txt").exists());

    let note = Repo::open(&set.copy).unwrap().update_submodules(&["lib".to_string()]).unwrap();
    assert_eq!(note, "1 submodule updated");
    assert_eq!(std::fs::read_to_string(set.copy.join("lib/lib.txt")).unwrap(), "one\n");

    let subs = Repo::open(&set.copy).unwrap().submodules();
    assert!(!subs[0].behind());
    assert_eq!(subs[0].checked_out, subs[0].recorded);
}

#[test]
fn a_pull_that_moves_the_record_leaves_the_submodule_behind_until_it_is_updated() {
    let set = WithSubmodule::new("sub-behind");
    Repo::open(&set.copy).unwrap().update_submodules(&["lib".to_string()]).unwrap();

    let moved = set.move_the_library_on();
    Repo::open(&set.copy).unwrap().pull(PullMode::FastForward).unwrap();

    // The record has moved and the folder has not: this is the state the
    // preference exists to end.
    let subs = Repo::open(&set.copy).unwrap().submodules();
    assert_eq!(subs[0].recorded, Some(moved));
    assert!(subs[0].behind() && !subs[0].missing());
    assert_eq!(std::fs::read_to_string(set.copy.join("lib/lib.txt")).unwrap(), "one\n");

    Repo::open(&set.copy).unwrap().update_submodules(&["lib".to_string()]).unwrap();
    assert_eq!(std::fs::read_to_string(set.copy.join("lib/lib.txt")).unwrap(), "one\ntwo\n");
    assert!(!Repo::open(&set.copy).unwrap().submodules()[0].behind());
}

#[test]
fn a_submodule_whose_removal_is_staged_is_not_brought_back() {
    let set = WithSubmodule::new("sub-removed");

    // `git rm lib`, as far as the index goes: the last commit still has it,
    // and the folder was never there in this clone.
    let raw = git2::Repository::open(&set.copy).unwrap();
    let mut index = raw.index().unwrap();
    index.remove_path(Path::new("lib")).unwrap();
    index.write().unwrap();

    let subs = Repo::open(&set.copy).unwrap().submodules();
    assert!(subs.iter().all(|sub| !sub.behind() && !sub.missing()), "{subs:?}");

    let note = Repo::open(&set.copy).unwrap().update_submodules(&["lib".to_string()]).unwrap();
    assert_eq!(note, "the submodules were already up to date");
    assert!(!set.copy.join("lib/lib.txt").exists());
}

#[test]
fn updating_submodules_that_are_where_they_should_be_says_so() {
    let set = WithSubmodule::new("sub-uptodate");
    Repo::open(&set.copy).unwrap().update_submodules(&["lib".to_string()]).unwrap();

    let note = Repo::open(&set.copy).unwrap().update_submodules(&["lib".to_string()]).unwrap();
    assert_eq!(note, "the submodules were already up to date");
    // And one that was not asked for is not touched either way.
    let note = Repo::open(&set.copy).unwrap().update_submodules(&[]).unwrap();
    assert_eq!(note, "the submodules were already up to date");
}
