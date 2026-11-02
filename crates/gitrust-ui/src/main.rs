//! gitrust: a desktop Git client.
//!
//! The window follows the layout of the usual graphical clients: references
//! on the left, the graph in the middle and the detail of what is picked on
//! the right, so that clicking in the graph shows the diff without changing
//! screen.

mod action_bar;
mod board_view;
mod changes_view;
mod detail_view;
mod diff_view;
mod graph_view;
mod refs_view;
mod theme;
mod toast;

use anyhow::Result;
use eframe::egui;
use gitrust_core::{
    compose, layout, layout_with_working, Move, Backlog, Board, CommitDetail, FileDiff, Graph, HunkId,
    Oid, PullMode, PullRequests, RefEntry, RefKind, Repo, StashEntry, Tracking, WorkingState,
    Worktree,
};

/// How wide the repository menu is, measured inside its frame.
///
/// Told, not asked for, for the same reason the action bar's menus are: a
/// menu that works its own width out from the room it is given answers a
/// little differently every frame, and the answers drift. Wide enough for a
/// path you can read and for the folders listed under it.
const REPO_MENU: f32 = 520.0;

fn main() -> eframe::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 900.0])
            .with_min_inner_size([900.0, 480.0])
            .with_title("gitrust"),
        ..Default::default()
    };

    eframe::run_native(
        "gitrust",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, path)))),
    )
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    History,
    Changes,
    Board,
}

struct App {
    path: String,
    head: String,
    /// What is in the repository: it stays put in the bottom bar.
    summary: String,
    /// The last thing that happened. It is cleared on a reload.
    status: String,
    status_failed: bool,
    tab: Tab,

    graph: Graph,
    limit: usize,
    /// What is being looked for in the history, and where it is.
    needle: String,
    found: Vec<usize>,
    at: usize,
    /// The row to bring into view on the next frame.
    bring: Option<usize>,
    refs: Vec<RefEntry>,
    ref_filter: String,

    /// The work laid out in columns, and which project it is filtered by.
    board: Option<Board>,
    project: Option<String>,
    /// Where you are on the board: the trunk, then every branch you went
    /// into. The last one is what the columns are measured against.
    trail: Vec<String>,
    /// Every folder this repository is checked out in, the main one first.
    worktrees: Vec<Worktree>,
    /// The one column that is stored rather than worked out, and what is being
    /// typed into it.
    backlog: Backlog,
    backlog_draft: String,
    /// What the forge last said, and the asking of it.
    pulls: Option<PullRequests>,
    forge: Option<Forge>,
    /// The board being worked out, when one is.
    laying: Option<Laying>,
    /// Why the forge could not answer, if it could not. It is kept so the
    /// board can say why it is guessing instead of quietly guessing.
    forge_note: Option<String>,
    /// The fetch button's position: it repeats the last thing asked for.
    pull_mode: PullMode,

    // The action bar and what it needs to know.
    tracking: Option<Tracking>,
    stashes: Vec<StashEntry>,
    draft: action_bar::Draft,
    job: Option<Job>,

    // The commit picked in the graph.
    selected_commit: Option<Oid>,
    detail: Option<CommitDetail>,
    detail_file: Option<String>,
    detail_diff: Option<FileDiff>,

    // The working tree.
    work: WorkingState,
    selected: Option<(String, bool)>,
    diff: Option<FileDiff>,
    subject: String,
    body: String,
    /// Whether what is written will redo the last commit.
    amend: bool,
    /// What was being written before the switch went on, so it can be given
    /// back if the switch goes off.
    saved: Option<(String, String)>,
    /// The tag being written in the detail panel, if any.
    tag: detail_view::TagDraft,
    /// What is waiting for a yes before anything is destroyed.
    confirm: Option<Confirm>,
    /// The note being written, when one is.
    describing: Option<Describing>,
    /// The name being asked for, when one is.
    naming: Option<Naming>,
    /// The last thing that happened, said where it will be seen.
    toast: Option<toast::Toast>,
}

/// A slow operation, running off to one side.
///
/// Fetching or pushing takes as long as the network takes, and doing it on
/// the painting thread would leave the window hung with no word of why. The
/// thread opens its own repository -- `git2::Repository` does not travel
/// between threads -- and sends back one single thing: the sentence to put in
/// the bottom bar.
struct Job {
    done: std::sync::mpsc::Receiver<Result<String>>,
}

/// The asking of the forge, which is a job of its own.
///
/// It is kept apart from [`Job`] on purpose. `gh` goes out to the network, and
/// it can sit there for as long as the network lets it; hanging the toolbar on
/// that would mean a board nobody asked to open could stop you pushing. So
/// this one darkens nothing and holds nothing up: the board is already drawn
/// from the graph, and the answer only ever improves it.
struct Forge {
    done: std::sync::mpsc::Receiver<Result<PullRequests>>,
}

/// The board, worked out off to one side.
///
/// Laying a board out means weighing every branch against every other, and on
/// a repository with thirty of them that is well over a second. Spent on the
/// painting thread it is a window that does not move, after every operation.
/// So it goes the way fetching goes: its own thread, its own repository, and
/// the board that was already there stays up until the new one arrives.
///
/// Only the newest one is ever listened to. Starting another drops this one,
/// and an answer nobody is holding a receiver for is an answer nobody reads,
/// so a slow board can never land on top of a quick one.
struct Laying {
    done: std::sync::mpsc::Receiver<Result<Board>>,
}

/// A note being written about a branch.
///
/// It is held here and not in the view for the same reason the confirm is:
/// a view that drew it would have to remember it between frames, and the
/// views here remember nothing.
struct Describing {
    branch: String,
    title: String,
    body: String,
    /// Whether there was one already, which is the difference between
    /// cancelling and being able to take it away.
    had: bool,
}

/// A name being asked for before something is made or renamed.
///
/// The two that need one are the same shape -- a box, a verb, and what to do
/// with what was typed -- so they share a dialog rather than having one each.
struct Naming {
    title: String,
    hint: String,
    verb: String,
    draft: String,
    deed: Named,
}

/// What the name will be used for.
enum Named {
    BranchAt(Oid),
    Rename(String),
}

/// Something destructive that is waiting for a yes.
///
/// Everything that cannot be undone comes through here. The view asks for the
/// action like any other; it is the app that stops to ask, so that no view
/// has to remember to.
struct Confirm {
    title: String,
    detail: String,
    verb: String,
    deed: Deed,
}

/// What will be done if the answer is yes. It is deliberately a different
/// type from the views' actions: that way there is no looping back to asking.
enum Deed {
    Discard(String),
    DiscardHunk { path: String, hunk: HunkId },
    DiscardAll(Vec<String>),
    DeleteBranch { name: String, remote: bool },
    CloseWorktree { name: String },
    MoveTo { oid: Oid, how: Move },
    Land { base: String, branch: String },
    Review(String),
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, path: String) -> Self {
        theme::install(&cc.egui_ctx);

        let mut app = Self {
            path,
            head: String::new(),
            summary: String::new(),
            status: String::new(),
            status_failed: false,
            tab: Tab::History,
            graph: Graph::default(),
            limit: 5_000,
            needle: String::new(),
            found: Vec::new(),
            at: 0,
            bring: None,
            refs: Vec::new(),
            ref_filter: String::new(),
            board: None,
            project: None,
            trail: Vec::new(),
            worktrees: Vec::new(),
            backlog: Backlog::default(),
            backlog_draft: String::new(),
            pulls: None,
            forge: None,
            laying: None,
            forge_note: None,
            pull_mode: PullMode::Merge,
            tracking: None,
            stashes: Vec::new(),
            draft: action_bar::Draft::default(),
            job: None,
            selected_commit: None,
            detail: None,
            detail_file: None,
            detail_diff: None,
            work: WorkingState::default(),
            selected: None,
            diff: None,
            subject: String::new(),
            body: String::new(),
            amend: false,
            saved: None,
            tag: detail_view::TagDraft::default(),
            confirm: None,
            describing: None,
            naming: None,
            toast: None,
        };
        app.load();
        app
    }

    fn repo(&self) -> Result<Repo> {
        Repo::open(&self.path)
    }

    /// Reloads everything: history, references and the working tree's state.
    fn load(&mut self) {
        let started = std::time::Instant::now();
        // A reload changes which commit is the last one, so a message brought
        // in from it is no longer good for anything.
        self.drop_amend();

        let mut repo = match self.repo() {
            Ok(repo) => repo,
            Err(err) => return self.fail(err),
        };

        self.head = repo.head_name();
        self.path = repo.workdir().display().to_string();
        self.refs = repo.references().unwrap_or_default();
        self.tracking = repo.tracking();
        self.lay_out();
        self.backlog = repo.backlog().unwrap_or_default();
        self.worktrees = repo.worktrees().unwrap_or_default();
        self.stashes = repo.stashes().unwrap_or_default();

        // The tree's state goes before the graph: whether there is a row of
        // uncommitted changes above the history depends on it.
        self.read_work(&repo);
        if let Err(err) = self.rebuild_graph(&repo) {
            return self.fail(err);
        }
        self.preselect();

        self.recount();
        self.status = format!("read in {} ms", started.elapsed().as_millis());
        self.status_failed = false;
        self.reload_detail(&repo);
    }

    /// Lays out the graph, with the uncommitted-changes row if the working
    /// tree has something to say.
    fn rebuild_graph(&mut self, repo: &Repo) -> Result<()> {
        let commits = repo.commits(self.limit)?;
        let total = commits.len();

        self.graph = match repo.head_id() {
            Some(head) if !self.work.is_clean() => layout_with_working(commits, head),
            _ => layout(commits),
        };

        let refs = self.refs.len();
        self.summary = format!(
            "{total} {} across {refs} {}",
            if total == 1 { "commit" } else { "commits" },
            if refs == 1 { "ref" } else { "refs" },
        );
        Ok(())
    }

    /// On opening, the right panel shows where HEAD is instead of greeting you
    /// with a gap: the first thing anyone wants to know about a repository is
    /// what last happened in it.
    fn preselect(&mut self) {
        if self.selected_commit.is_some() {
            return;
        }
        self.selected_commit = self
            .graph
            .rows
            .iter()
            .find(|row| row.commit.refs.iter().any(|r| r.kind == RefKind::Head))
            .or_else(|| self.graph.rows.first())
            .map(|row| row.commit.id);
    }

    /// Counts again which rows match what is being looked for.
    ///
    /// It is redone on typing and on reloading: the indices are positions in
    /// the graph, and a new graph leaves them meaningless.
    fn recount(&mut self) {
        let needle = self.needle.trim().to_lowercase();
        self.found = if needle.is_empty() {
            Vec::new()
        } else {
            self.graph
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| row.commit.matches(&needle))
                .map(|(index, _)| index)
                .collect()
        };
        self.at = 0;
    }

    /// Jumps to the next or previous match, wrapping round at the end.
    fn jump(&mut self, forward: bool) {
        if self.found.is_empty() {
            return;
        }
        let last = self.found.len() - 1;
        self.at = match (forward, self.at) {
            (true, at) if at >= last => 0,
            (true, at) => at + 1,
            (false, 0) => last,
            (false, at) => at - 1,
        };
        self.land();
    }

    /// Brings the current match into view and picks it.
    fn land(&mut self) {
        let Some(&index) = self.found.get(self.at) else {
            return;
        };
        self.bring = Some(index);
        if let Some(row) = self.graph.rows.get(index) {
            let oid = row.commit.id;
            self.select_commit(oid);
        }
    }

    fn has_working_row(&self) -> bool {
        self.graph.rows.first().is_some_and(|row| row.commit.is_working())
    }

    /// Takes amend mode off and gives back whatever was being written.
    fn drop_amend(&mut self) {
        self.amend = false;
        if let Some((subject, body)) = self.saved.take() {
            self.subject = subject;
            self.body = body;
        }
    }

    /// Whether there is a commit to amend, and the remote that already has it.
    ///
    /// The remote's name comes back owned on purpose. Borrowed from `&self`,
    /// the borrow would last as long as the value, and the commit box could
    /// not take hold of what is being written at the same time.
    fn amend_state(&self) -> (bool, Option<String>) {
        let amendable = self.graph.rows.iter().any(|row| !row.commit.is_working());

        // With nothing ahead of the remote, the last commit is already there.
        let pushed = self
            .tracking
            .as_ref()
            .filter(|tracking| tracking.ahead == 0)
            .map(|tracking| tracking.remote.clone());

        (amendable, pushed)
    }

    /// Reloads only what changes when the index is touched.
    fn refresh(&mut self) {
        let mut repo = match self.repo() {
            Ok(repo) => repo,
            Err(err) => return self.fail(err),
        };

        self.head = repo.head_name();
        self.stashes = repo.stashes().unwrap_or_default();
        let had_working = self.has_working_row();
        self.read_work(&repo);

        // The changes row comes and goes from the graph with the working tree:
        // if the tree has just got dirty, or just got clean, it has to be laid
        // out again so the row appears or leaves.
        let wants_working = !self.work.is_clean();
        if had_working != wants_working {
            let _ = self.rebuild_graph(&repo);
            self.recount();
        }

        // And if what was picked was that row and it no longer exists, the
        // cursor goes to HEAD instead of staying on a gap.
        if !wants_working && self.selected_commit.is_some_and(|oid| oid.is_zero()) {
            self.selected_commit = None;
            self.preselect();
            self.reload_detail(&repo);
        }
    }

    fn read_work(&mut self, repo: &Repo) {
        match repo.status() {
            Ok(work) => self.work = work,
            Err(err) => return self.note_error(format!("could not read the status: {err}")),
        }

        // If the picked file is no longer on its side, we let the pick go.
        if let Some((path, staged)) = self.selected.clone() {
            let list = if staged { &self.work.staged } else { &self.work.unstaged };
            if list.iter().any(|file| file.path == path) {
                self.diff = repo.file_diff(&path, staged).ok();
            } else {
                self.selected = None;
                self.diff = None;
            }
        }
    }

    fn reload_detail(&mut self, repo: &Repo) {
        // The uncommitted-changes row has no detail to read: the right panel
        // shows it the working tree instead.
        let Some(oid) = self.selected_commit.filter(|oid| !oid.is_zero()) else {
            self.detail = None;
            self.detail_diff = None;
            return;
        };

        self.detail = repo.commit_detail(oid).ok();

        // With no file picked, the commit's first one is opened. A commit of
        // one single file should not force one more click to see the only
        // thing it carries.
        if self.detail_file.is_none() {
            self.detail_file = self
                .detail
                .as_ref()
                .and_then(|detail| detail.files.first())
                .map(|file| file.path.clone());
        }

        self.detail_diff = match &self.detail_file {
            Some(path) if self.detail.is_some() => repo.commit_file_diff(oid, path).ok(),
            _ => None,
        };
    }

    /// Picks a commit and brings it into the graph's view.
    ///
    /// Pressing a branch in the sidebar or a card on the board means you want
    /// to see it. If it falls outside the window, picking it goes unnoticed.
    fn reveal_commit(&mut self, oid: Oid) {
        self.bring = self.graph.rows.iter().position(|row| row.commit.id == oid);
        self.select_commit(oid);
    }

    fn select_commit(&mut self, oid: Oid) {
        self.selected_commit = Some(oid);
        self.detail_file = None;
        self.detail_diff = None;

        match self.repo() {
            Ok(repo) => self.reload_detail(&repo),
            Err(err) => self.fail(err),
        }
    }

    fn select_detail_file(&mut self, path: String) {
        let Some(oid) = self.selected_commit else {
            return;
        };
        self.detail_diff = self
            .repo()
            .and_then(|repo| repo.commit_file_diff(oid, &path))
            .ok();
        self.detail_file = Some(path);
    }

    /// Runs an operation on the repository and reloads the state.
    fn run(&mut self, op: impl FnOnce(&mut Repo) -> Result<()>) -> bool {
        match self.repo().and_then(|mut repo| op(&mut repo)) {
            Ok(()) => {
                self.refresh();
                true
            }
            Err(err) => {
                self.note_error(format!("{err:#}"));
                false
            }
        }
    }

    /// Sends a slow operation off to a thread of its own.
    ///
    /// While it lasts the action bar goes dark altogether: queueing a push
    /// behind a pull when neither has finished is asking for a mess somebody
    /// then has to explain.
    fn spawn(
        &mut self,
        doing: &str,
        work: impl FnOnce(&mut Repo) -> Result<String> + Send + 'static,
    ) {
        let (send, done) = std::sync::mpsc::channel();
        let path = self.path.clone();

        std::thread::spawn(move || {
            let _ = send.send(Repo::open(&path).and_then(|mut repo| work(&mut repo)));
        });

        self.status = format!("{doing}…");
        self.status_failed = false;
        self.job = Some(Job { done });
    }

    /// Looks at whether the background operation has finished.
    ///
    /// While it has not, it asks to be repainted: without that egui falls
    /// asleep as soon as nobody touches the mouse, and the result would sit
    /// there waiting for somebody to move the window.
    fn poll(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;

        let Some(job) = &self.job else {
            return;
        };

        match job.done.try_recv() {
            Ok(result) => {
                self.job = None;
                match result {
                    Ok(note) => {
                        self.load();
                        self.said(note);
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
            }
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.note_error("the operation was cut short".to_string());
            }
        }
    }

    /// Leaves something waiting for a yes instead of doing it.
    fn ask(&mut self, title: String, detail: &str, verb: &str, deed: Deed) {
        self.confirm = Some(Confirm {
            title,
            detail: detail.to_string(),
            verb: verb.to_string(),
            deed,
        });
    }

    /// Asks before deleting a branch, saying what it costs.
    fn ask_delete_branch(&mut self, name: String, remote: bool) {
        if remote {
            let server = name.split_once('/').map(|(server, _)| server).unwrap_or("the remote");
            let detail = format!(
                "It is deleted on {server}, not just here. Anyone who has it keeps seeing it until they fetch."
            );
            return self.ask(
                format!("Delete {name} on the remote?"),
                &detail,
                "Delete",
                Deed::DeleteBranch { name, remote },
            );
        }

        // How many commits go with it is the only question that matters.
        let alone = self.repo().and_then(|repo| repo.unmerged(&name)).unwrap_or(0);
        let detail = match alone {
            0 => "Everything it has is somewhere else too, so nothing is lost."
                .to_string(),
            1 => "It has one commit that is on no other branch, remote or tag. That goes with it."
                .to_string(),
            n => format!(
                "It has {n} commits that are on no other branch, remote or tag. Those go with it."
            ),
        };

        self.ask(
            format!("Delete the branch {name}?"),
            &detail,
            "Delete",
            Deed::DeleteBranch { name, remote },
        );
    }

    /// Goes and asks the forge which pull requests there are.
    ///
    /// It runs at most once at a time, and it never says anything in the
    /// bottom bar while it runs: nobody asked for it out loud, so it should
    /// not talk over whatever did.
    fn ask_forge(&mut self) {
        if self.forge.is_some() {
            return;
        }

        let (send, done) = std::sync::mpsc::channel();
        let path = self.path.clone();

        std::thread::spawn(move || {
            let _ = send.send(Repo::open(&path).and_then(|repo| repo.pull_requests()));
        });

        self.forge = Some(Forge { done });
    }

    /// Looks at whether the forge has answered, and redraws the board if so.
    fn poll_forge(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;

        let Some(forge) = &self.forge else {
            return;
        };

        match forge.done.try_recv() {
            Ok(Ok(pulls)) => {
                self.forge = None;
                self.forge_note = None;
                self.pulls = Some(pulls);
                self.rebuild_board();
            }
            // A forge that cannot answer is not a failure of the board; it is
            // the board going back to what the graph alone can tell. The
            // reason is kept for the header to show, not shouted in the bar.
            Ok(Err(err)) => {
                self.forge = None;
                self.forge_note = Some(format!("{err:#}"));
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
            }
            Err(TryRecvError::Disconnected) => {
                self.forge = None;
                self.forge_note = Some("the forge was not reached".to_string());
            }
        }
    }

    /// Hands an address to whatever the desktop opens addresses with.
    ///
    /// It is spawned and forgotten rather than waited on: the browser takes a
    /// second or two to appear and there is nothing to report when it does.
    /// What there is to report is it not being there at all.
    fn open_in_browser(&mut self, url: &str) {
        match std::process::Command::new("xdg-open").arg(url).spawn() {
            Ok(_) => {
                self.said(format!("opening {url}"));
            }
            Err(err) => self.note_error(format!("could not open a browser: {err}")),
        }
    }

    /// Writes something in the backlog, or takes something out of it.
    ///
    /// All of it is local and finishes before anybody notices: the sharing is
    /// a separate thing, asked for out loud. Everything is read again
    /// afterwards because starting an item makes a branch and moves HEAD,
    /// which changes the graph, the sidebar and the columns at once.
    fn note_down(&mut self, op: impl FnOnce(&Repo) -> Result<String>) {
        match self.repo().and_then(|repo| op(&repo)) {
            Ok(note) => {
                self.load();
                self.said(note);
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }

    /// Lays the columns out again over whatever the forge last said.
    fn rebuild_board(&mut self) {
        self.lay_out();
    }

    /// Sends the board off to be laid out against wherever the trail ends.
    fn lay_out(&mut self) {
        if self.trail.is_empty() {
            let trunk = self.repo().ok().and_then(|repo| repo.trunk().ok());
            match trunk {
                Some(trunk) => self.trail.push(trunk),
                None => {
                    self.board = None;
                    self.laying = None;
                    return;
                }
            }
        }

        let base = self.trail.last().cloned().unwrap_or_default();
        let path = self.path.clone();
        let pulls = self.pulls.clone();
        let (send, done) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let _ = send.send(
                Repo::open(&path).and_then(|repo| repo.board_against(&base, pulls.as_ref())),
            );
        });

        self.laying = Some(Laying { done });
    }

    /// Looks at whether the board has been worked out, and puts it up if so.
    ///
    /// A branch you went into can stop existing while you are standing in it:
    /// deleted, or merged and tidied away. Stepping back out is better than
    /// showing nothing and better than an error about a branch you did not
    /// ask about.
    fn poll_board(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;

        let Some(laying) = &self.laying else {
            return;
        };

        match laying.done.try_recv() {
            Ok(Ok(board)) => {
                self.laying = None;
                self.board = Some(board);
            }
            Ok(Err(_)) => {
                self.laying = None;

                // A step you went into has gone: step back out of it and lay
                // the one below out instead.
                if self.trail.len() > 1 {
                    self.trail.pop();
                    self.project = None;
                    self.lay_out();
                    return;
                }

                // The bottom step was never this repository's trunk, which is
                // what opening another repository leaves behind. Work it out
                // again -- but only if it comes back different, or this would
                // ask the same question forever.
                let trunk = self.repo().ok().and_then(|repo| repo.trunk().ok());
                match trunk {
                    Some(trunk) if self.trail.first() != Some(&trunk) => {
                        self.trail = vec![trunk];
                        self.project = None;
                        self.lay_out();
                    }
                    _ => {
                        self.trail.clear();
                        self.board = None;
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
            }
            Err(TryRecvError::Disconnected) => {
                self.laying = None;
            }
        }
    }

    /// Looks at one branch's own board, or at the one it came from.
    ///
    /// The project filter is dropped on the way: it was made of the names on
    /// the board you were looking at, and carrying it in would quietly empty
    /// the one you are arriving at.
    fn walk_to(&mut self, trail: Vec<String>) {
        self.trail = trail;
        self.project = None;
        self.rebuild_board();
    }

    /// Asks before opening a pull request, which everyone on the repository
    /// is going to see.
    fn ask_review(&mut self, branch: String) {
        let trunk = match &self.board {
            Some(board) => board.trunk.clone(),
            None => return,
        };

        self.ask(
            format!("Open a pull request for {branch}?"),
            &format!(
                "{branch} is pushed first, and then a pull request is opened against \
                 {trunk} with the title and body taken from its own commits. \
                 Everyone on the repository sees it."
            ),
            "Open",
            Deed::Review(branch),
        );
    }

    /// Looks at another checkout of the same repository.
    ///
    /// Same history, same branches, a different HEAD and a different working
    /// tree. Nothing is written and nothing is fetched: it is the window
    /// moving, not the repository.
    fn visit(&mut self, path: std::path::PathBuf) {
        self.path = path.display().to_string();
        // What was picked belonged to the other folder's working tree.
        self.selected_commit = None;
        self.detail = None;
        self.detail_file = None;
        self.detail_diff = None;
        self.selected = None;
        self.diff = None;

        self.load();
        self.said(format!("looking at {}", self.path));
    }

    /// Asks before taking a checkout away, saying what goes with it.
    fn ask_release(&mut self, name: String) {
        let where_it_is = self
            .worktrees
            .iter()
            .find(|tree| tree.name == name)
            .map(|tree| tree.path.display().to_string())
            .unwrap_or_else(|| name.clone());

        // What is uncommitted there is the only thing that cannot be had
        // back, so it is the number that goes in the question.
        let loose = self.repo().and_then(|repo| repo.worktree_changes(&name)).unwrap_or(0);
        let detail = match loose {
            0 => format!(
                "{where_it_is} is deleted. The branch stays where it is, and \
                 nothing is lost: there is nothing uncommitted in there."
            ),
            1 => format!(
                "{where_it_is} is deleted, and 1 uncommitted file in it goes too. \
                 The branch itself stays."
            ),
            n => format!(
                "{where_it_is} is deleted, and {n} uncommitted files in it go too. \
                 The branch itself stays."
            ),
        };

        self.ask(
            format!("Close the folder {name}?"),
            &detail,
            "Close",
            Deed::CloseWorktree { name },
        );
    }

    /// Moves the branch you are on to a commit, asking first when the move
    /// would take your files with it.
    ///
    /// Only the one that discards asks. The other two keep everything you had
    /// not committed -- staged or not -- so there is nothing to warn about
    /// and nothing a question would protect.
    fn ask_move(&mut self, oid: Oid, how: Move) {
        if how != Move::Discard {
            let doing = format!("moving to {}", &oid.to_string()[..7]);
            return self.spawn(&doing, move |repo| repo.move_branch(oid, how));
        }

        let loose = self.work.staged.len() + self.work.unstaged.len();
        let detail = match loose {
            0 => format!(
                "{} moves back to {}. The commits after it stop being shown, and \
                 nothing of yours goes with them: there is nothing uncommitted.",
                self.head,
                &oid.to_string()[..7]
            ),
            1 => format!(
                "{} moves back to {}, and the one file you have not committed is \
                 thrown away with it. No stash holds it and nothing gives it back.",
                self.head,
                &oid.to_string()[..7]
            ),
            n => format!(
                "{} moves back to {}, and the {n} files you have not committed are \
                 thrown away with it. No stash holds them and nothing gives them back.",
                self.head,
                &oid.to_string()[..7]
            ),
        };

        self.ask(
            format!("Move {} and throw the rest away?", self.head),
            &detail,
            "Move and discard",
            Deed::MoveTo { oid, how },
        );
    }

    /// Asks before bringing a branch into the one whose board you are on,
    /// which may start by moving you somewhere else.
    ///
    /// Not always the main branch. Inside a card it is that card's branch,
    /// because a subtask is finished when it goes into its parent, and
    /// merging it into the trunk instead would be doing something else
    /// entirely while looking like the same gesture.
    fn ask_land(&mut self, branch: String) {
        let base = match &self.board {
            Some(board) => board.trunk.clone(),
            None => return,
        };

        let detail = if self.head == base {
            format!("{branch} is merged into {base}. If they clash, nothing is touched and it says where.")
        } else {
            format!(
                "You move to {base} first, and then {branch} is merged into it. \
                 If they clash, nothing is merged and you stay on {base}."
            )
        };

        self.ask(
            format!("Bring {branch} into {base}?"),
            &detail,
            "Merge",
            Deed::Land { base, branch },
        );
    }

    /// Does what has already been answered yes to.
    fn commit_deed(&mut self, deed: Deed) {
        let discarded = match deed {
            Deed::Discard(path) => self.run(|repo| repo.discard(&path)),
            Deed::DiscardHunk { path, hunk } => self.run(|repo| repo.discard_hunk(&path, hunk)),
            Deed::DiscardAll(paths) => self.run(|repo| repo.discard_all(&paths)),

            // The remote one goes out on the network; the one here is instant.
            Deed::DeleteBranch { name, remote } if remote => {
                let doing = format!("deleting {name}");
                return self.spawn(&doing, move |repo| repo.delete_remote_branch(&name));
            }
            Deed::MoveTo { oid, how } => {
                let doing = format!("moving to {}", &oid.to_string()[..7]);
                return self.spawn(&doing, move |repo| repo.move_branch(oid, how));
            }
            Deed::CloseWorktree { name } => {
                let doing = format!("closing {name}");
                return self.spawn(&doing, move |repo| repo.remove_worktree(&name));
            }
            Deed::Land { base, branch } => {
                let doing = format!("merging {branch} into {base}");
                return self.spawn(&doing, move |repo| repo.land_into(&base, &branch));
            }
            Deed::Review(branch) => {
                // What the forge last said no longer holds once there is a new
                // pull request in it, so it gets asked again when this lands.
                self.pulls = None;
                let doing = format!("opening a pull request for {branch}");
                return self.spawn(&doing, move |repo| repo.open_pull_request(&branch));
            }
            Deed::DeleteBranch { name, .. } => {
                return match self.repo().and_then(|repo| repo.delete_branch(&name)) {
                    Ok(()) => {
                        // The references change, so the graph and the sidebar
                        // have to be read again.
                        self.load();
                        self.said(format!("branch {name} deleted"));
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                };
            }
        };

        if discarded {
            self.said("changes discarded".to_string());
        }
    }

    /// The warning about what cannot be undone.
    ///
    /// What destroys goes on the right and in red, and what does nothing on
    /// the left: the button that is easy to reach without reading has to be
    /// the one that breaks nothing.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm else {
            return;
        };

        let (title, detail, verb) =
            (confirm.title.clone(), confirm.detail.clone(), confirm.verb.clone());
        let mut go = false;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let modal = egui::Modal::new(egui::Id::new("confirm"))
            .frame(
                egui::Frame::NONE
                    .fill(theme::RAISED)
                    .inner_margin(egui::Margin::same(18))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .stroke(egui::Stroke::new(1.0, theme::EDGE)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(372.0);
                ui.label(
                    egui::RichText::new(&title)
                        .font(theme::strong(14.0))
                        .color(theme::TEXT),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&detail)
                            .font(theme::body(12.0))
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Cancel").min_size(egui::vec2(88.0, 28.0)))
                        .clicked()
                    {
                        close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let button = egui::Button::new(
                            egui::RichText::new(&verb)
                                .font(theme::medium(13.0))
                                .color(theme::REMOVED),
                        )
                        .fill(theme::tint(theme::REMOVED, 38))
                        .min_size(egui::vec2(104.0, 28.0));

                        if ui.add(button).clicked() {
                            go = true;
                        }
                    });
                });
            });

        if modal.backdrop_response.clicked() {
            close = true;
        }

        if go {
            if let Some(confirm) = self.confirm.take() {
                self.commit_deed(confirm.deed);
            }
        } else if close {
            self.confirm = None;
        }
    }

    /// Opens the note for a branch, with whatever is already written in it.
    fn describe(&mut self, branch: String) {
        let found = self.repo().ok().and_then(|repo| repo.note(&branch).ok().flatten());
        self.describing = Some(Describing {
            had: found.is_some(),
            title: found.as_ref().map(|note| note.title.clone()).unwrap_or_default(),
            body: found.as_ref().map(|note| note.body.clone()).unwrap_or_default(),
            branch,
        });
    }

    /// What a branch is for, in the words somebody would use out loud.
    ///
    /// Two boxes and not one: a line that has to be read at a glance on a
    /// card, and underneath it as much as anybody wants to write, which the
    /// card never shows.
    fn note_dialog(&mut self, ctx: &egui::Context) {
        let Some(note) = &mut self.describing else {
            return;
        };

        let branch = note.branch.clone();
        let had = note.had;
        let (mut save, mut clear) = (false, false);
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let modal = egui::Modal::new(egui::Id::new("note"))
            .frame(
                egui::Frame::NONE
                    .fill(theme::RAISED)
                    .inner_margin(egui::Margin::same(18))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .stroke(egui::Stroke::new(1.0, theme::EDGE)),
            )
            .show(ctx, |ui| {
                ui.set_width(452.0);
                ui.label(
                    egui::RichText::new("What is this branch for?")
                        .font(theme::strong(14.0))
                        .color(theme::TEXT),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(&branch).font(theme::mono(11.0)).color(theme::MUTED),
                );
                ui.add_space(12.0);

                ui.add(
                    egui::TextEdit::singleline(&mut note.title)
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(8.0, 6.0))
                        .font(theme::body(12.5))
                        .hint_text(
                            egui::RichText::new("in the words you would use out loud")
                                .color(theme::FAINT),
                        ),
                );
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut note.body)
                        .desired_width(f32::INFINITY)
                        .desired_rows(5)
                        .margin(egui::vec2(8.0, 6.0))
                        .font(theme::body(12.0))
                        .hint_text(
                            egui::RichText::new("anything else worth saying, or nothing")
                                .color(theme::FAINT),
                        ),
                );
                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Cancel").min_size(egui::vec2(88.0, 28.0)))
                        .clicked()
                    {
                        close = true;
                    }
                    // Only offered when there is one to take away, so the
                    // dialog never shows a button that would do nothing.
                    if had
                        && ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Take it away").color(theme::MUTED),
                                )
                                .min_size(egui::vec2(104.0, 28.0)),
                            )
                            .on_hover_text(
                                "the reference keeps its own history, so the words are still in it",
                            )
                            .clicked()
                    {
                        clear = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let button = egui::Button::new(
                            egui::RichText::new("Save")
                                .font(theme::medium(13.0))
                                .color(theme::TEXT),
                        )
                        .fill(theme::CHOSEN)
                        .min_size(egui::vec2(104.0, 28.0));

                        if ui.add(button).clicked() {
                            save = true;
                        }
                    });
                });
            });

        if modal.backdrop_response.clicked() {
            close = true;
        }

        if save || clear {
            let Some(note) = self.describing.take() else {
                return;
            };
            match clear {
                true => self.note_down(|repo| repo.note_clear(&note.branch)),
                false => {
                    self.note_down(|repo| repo.note_set(&note.branch, &note.title, &note.body))
                }
            }
        } else if close {
            self.describing = None;
        }
    }

    /// Asks for a name, for whatever is about to be given one.
    fn name_dialog(&mut self, ctx: &egui::Context) {
        let Some(naming) = &mut self.naming else {
            return;
        };

        let (title, hint, verb) =
            (naming.title.clone(), naming.hint.clone(), naming.verb.clone());
        let mut go = false;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let modal = egui::Modal::new(egui::Id::new("naming"))
            .frame(
                egui::Frame::NONE
                    .fill(theme::RAISED)
                    .inner_margin(egui::Margin::same(18))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .stroke(egui::Stroke::new(1.0, theme::EDGE)),
            )
            .show(ctx, |ui| {
                ui.set_width(372.0);
                ui.label(
                    egui::RichText::new(&title).font(theme::strong(14.0)).color(theme::TEXT),
                );
                ui.add_space(10.0);

                let entry = ui.add(
                    egui::TextEdit::singleline(&mut naming.draft)
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(8.0, 6.0))
                        .font(theme::mono(12.0))
                        .hint_text(egui::RichText::new(&hint).color(theme::FAINT)),
                );
                entry.request_focus();
                if entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = true;
                }

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Cancel").min_size(egui::vec2(88.0, 28.0)))
                        .clicked()
                    {
                        close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let button = egui::Button::new(
                            egui::RichText::new(&verb)
                                .font(theme::medium(13.0))
                                .color(theme::TEXT),
                        )
                        .fill(theme::CHOSEN)
                        .min_size(egui::vec2(104.0, 28.0));
                        if ui.add(button).clicked() {
                            go = true;
                        }
                    });
                });
            });

        if modal.backdrop_response.clicked() {
            close = true;
        }

        if go {
            let Some(naming) = self.naming.take() else {
                return;
            };
            let name = naming.draft.trim().to_string();
            if name.is_empty() {
                return self.note_error("it needs a name".to_string());
            }
            match naming.deed {
                Named::BranchAt(oid) => self.note_down(move |repo| repo.branch_at(&name, oid)),
                Named::Rename(old) => {
                    self.note_down(move |repo| repo.rename_branch(&old, &name))
                }
            }
        } else if close {
            self.naming = None;
        }
    }

    /// Says that something finished, in the bar and in a toast at once.
    ///
    /// Both, not one or the other: the bar is the record of the last thing
    /// that happened and stays until the next, and the toast is the part that
    /// gets seen. A sentence said only in the bar is a sentence nobody reads
    /// while they are looking at the work they just asked about.
    fn said(&mut self, text: String) {
        self.toast = Some(toast::Toast::new(text.clone(), false));
        self.status = text;
        self.status_failed = false;
    }

    /// The same, for something that went wrong: the bar marks it as a failure
    /// and the toast comes up in red and stays up for longer.
    fn note_error(&mut self, message: String) {
        self.toast = Some(toast::Toast::new(message.clone(), true));
        self.status = message;
        self.status_failed = true;
    }

    fn fail(&mut self, err: anyhow::Error) {
        self.graph = Graph::default();
        self.refs.clear();
        self.board = None;
        self.tracking = None;
        self.stashes.clear();
        self.work = WorkingState::default();
        self.selected = None;
        self.diff = None;
        self.detail = None;
        self.detail_diff = None;
        self.head.clear();
        self.summary.clear();
        self.note_error(format!("{err:#}"));
    }

    fn handle(&mut self, action: changes_view::Action) {
        use changes_view::Action;

        match action {
            Action::Select { path, staged } => {
                self.selected = Some((path.clone(), staged));
                self.diff = self.repo().and_then(|r| r.file_diff(&path, staged)).ok();
            }
            Action::Stage(path) => {
                self.run(|repo| repo.stage(&path));
            }
            Action::Unstage(path) => {
                self.run(|repo| repo.unstage(&path));
            }
            Action::StageHunk { path, hunk } => {
                self.run(|repo| repo.stage_hunk(&path, hunk));
            }
            Action::UnstageHunk { path, hunk } => {
                self.run(|repo| repo.unstage_hunk(&path, hunk));
            }
            Action::Discard { path, untracked } => {
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                let detail = if untracked {
                    "Git knows nothing about that file yet, so it goes from the disk and is left nowhere."
                } else {
                    "It goes back to what the index says. Whatever you have written since is saved nowhere."
                };
                self.ask(
                    format!("Discard the changes to {name}?"),
                    detail,
                    "Discard",
                    Deed::Discard(path),
                );
            }
            Action::DiscardHunk { path, hunk } => {
                self.ask(
                    "Discard this hunk?".to_string(),
                    "That hunk goes back to what the index says. The rest of the file stays as you have it.",
                    "Discard",
                    Deed::DiscardHunk { path, hunk },
                );
            }
            Action::DiscardAll => {
                let paths: Vec<String> =
                    self.work.unstaged.iter().map(|file| file.path.clone()).collect();
                self.ask(
                    format!("Discard {}?", theme::files_label(paths.len())),
                    "The ones Git knows go back to the index; the new ones go from the disk. None of it can be got back.",
                    "Discard",
                    Deed::DiscardAll(paths),
                );
            }
            Action::StageAll => {
                let paths: Vec<String> = self.work.unstaged.iter().map(|f| f.path.clone()).collect();
                self.run(|repo| repo.stage_all(&paths));
            }
            Action::UnstageAll => {
                let paths: Vec<String> = self.work.staged.iter().map(|f| f.path.clone()).collect();
                self.run(|repo| repo.unstage_all(&paths));
            }
            Action::AmendToggled => {
                if self.amend {
                    // The last commit's message comes in, and whatever was
                    // written is kept: taking the switch off has to give it
                    // back, not have lost it along the way.
                    self.saved = Some((self.subject.clone(), self.body.clone()));
                    if let Some((subject, body)) =
                        self.repo().ok().and_then(|repo| repo.head_message())
                    {
                        self.subject = subject;
                        self.body = body;
                    }
                } else {
                    self.drop_amend();
                }
            }
            Action::Amend => {
                let message = compose(&self.subject, &self.body);
                match self.repo().and_then(|repo| repo.amend(&message)) {
                    Ok(oid) => {
                        self.saved = None;
                        self.subject.clear();
                        self.body.clear();
                        // The old one no longer exists: the cursor goes to the
                        // one replacing it, not to a gap.
                        self.selected_commit = Some(oid);
                        self.detail_file = None;
                        self.load();
                        self.said("last commit redone".to_string());
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            Action::Commit => {
                let message = compose(&self.subject, &self.body);
                match self.repo().and_then(|repo| repo.commit(&message)) {
                    Ok(oid) => {
                        self.subject.clear();
                        self.body.clear();
                        // The cursor stays on what has just been written: the
                        // changes row it was on has just stopped existing.
                        self.selected_commit = Some(oid);
                        self.detail_file = None;
                        // The commit changes the history: redraw the graph.
                        self.load();
                        self.said(format!("commit created on {}", self.head));
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
        }
    }
}

impl App {
    fn act(&mut self, action: action_bar::Action) {
        use action_bar::Action;

        match action {
            // The two that go out on the network leave for a thread of their
            // own. The rest are local and finish before anyone notices.
            Action::Pull(mode) => self.spawn("pulling", move |repo| repo.pull(mode)),
            Action::Fetch => self.spawn("fetching", |repo| repo.fetch_all()),
            Action::Push => self.spawn("pushing", |repo| repo.push()),
            Action::PushForce => self.spawn("forcing", |repo| repo.push_force()),
            Action::PushTags => self.spawn("sending tags", |repo| repo.push_tags()),

            Action::Branch(name) => {
                match self.repo().and_then(|repo| repo.branch_here(&name)) {
                    Ok(()) => {
                        self.load();
                        self.said(format!("you are now on {name}"));
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }

            Action::StashSave(message) => self.local(|repo| repo.stash_save(&message)),
            Action::StashPop(index) => self.local(|repo| repo.stash_pop(index)),
            Action::StashApply(index) => self.local(|repo| repo.stash_apply(index)),
            Action::StashDrop(index) => self.local(|repo| repo.stash_drop(index)),
        }
    }

    /// A local operation that leaves word of how it went.
    fn local(&mut self, op: impl FnOnce(&mut Repo) -> Result<String>) {
        match self.repo().and_then(|mut repo| op(&mut repo)) {
            Ok(note) => {
                self.refresh();
                self.said(note);
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.poll_forge(ctx);
        self.poll_board(ctx);
        self.top_bar(ctx);
        self.confirm_dialog(ctx);
        self.note_dialog(ctx);
        self.name_dialog(ctx);
        self.status_bar(ctx);
        self.refs_panel(ctx);

        if self.tab == Tab::History {
            self.detail_panel(ctx);
        }

        let frame = match self.tab {
            // The graph is painted on the deepest background in the window so
            // that the lanes are the brightest thing on screen.
            Tab::History => egui::Frame::NONE.fill(theme::CANVAS),
            Tab::Changes => egui::Frame::NONE.fill(theme::PANEL),
            // The board is painted on the canvas so that the cards, which wear
            // the panels' color, lift off the background.
            Tab::Board => egui::Frame::NONE
                .fill(theme::CANVAS)
                .inner_margin(egui::Margin::symmetric(12, 0)),
        };

        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| match self.tab {
                Tab::History => {
                    match graph_view::finder(ui, &mut self.needle, self.found.len(), self.at) {
                        graph_view::Found::Changed => {
                            self.recount();
                            self.land();
                        }
                        graph_view::Found::Next => self.jump(true),
                        graph_view::Found::Prev => self.jump(false),
                        graph_view::Found::Nothing => {}
                    }

                    let needle = self.needle.trim().to_lowercase();
                    let clicked = graph_view::show(
                        ui,
                        &self.graph,
                        self.selected_commit,
                        &self.work,
                        &needle,
                        self.bring.take(),
                    );
                    if let Some(oid) = clicked {
                        self.select_commit(oid);
                    }
                }
                Tab::Board => {
                    // The forge is asked the first time the board is looked
                    // at, not when the window opens: somebody who never leaves
                    // the history should not be making network calls.
                    if self.pulls.is_none() && self.forge_note.is_none() {
                        self.ask_forge();
                    }

                    let state = board_view::Forge {
                        asking: self.forge.is_some(),
                        note: self.forge_note.as_deref(),
                    };

                    // The checkout you are looking at is not somewhere else,
                    // so a card for a branch open here says nothing about
                    // folders: you are standing in it.
                    let here = std::path::Path::new(&self.path);
                    let elsewhere: Vec<Worktree> = self
                        .worktrees
                        .iter()
                        .filter(|tree| tree.path != here)
                        .cloned()
                        .collect();
                    let picked = match &self.board {
                        Some(board) => board_view::show(
                            ui,
                            board,
                            &mut self.project,
                            self.job.is_some(),
                            state,
                            board_view::Waiting {
                                backlog: &self.backlog,
                                draft: &mut self.backlog_draft,
                            },
                            &self.trail,
                            &elsewhere,
                        ),
                        None if self.laying.is_some() => {
                            theme::empty(ui, "Working the board out…");
                            None
                        }
                        None => {
                            theme::empty(ui, "Open a repository to see its work");
                            None
                        }
                    };

                    match picked {
                        Some(board_view::Act::Pick(refs_view::Pick::Show(oid))) => {
                            self.reveal_commit(oid);
                            self.tab = Tab::History;
                        }
                        Some(board_view::Act::Pick(refs_view::Pick::Switch { name, remote })) => {
                            self.switch(name, remote)
                        }
                        // Pushing is what moves a card into review only while
                        // nobody has asked the forge. Once it has answered,
                        // what moves it is a pull request, so that is what the
                        // drop does.
                        Some(board_view::Act::Publish(branch)) => {
                            let doing = format!("pushing {branch}");
                            self.spawn(&doing, move |repo| repo.push_branch(&branch));
                        }
                        Some(board_view::Act::Review(branch)) => self.ask_review(branch),
                        Some(board_view::Act::Land(branch)) => self.ask_land(branch),
                        Some(board_view::Act::Reload) => {
                            self.pulls = None;
                            self.forge_note = None;
                            self.ask_forge();
                        }
                        Some(board_view::Act::Open(url)) => self.open_in_browser(&url),

                        // Going in and coming back out. Neither touches the
                        // repository: they measure the same board against a
                        // different branch.
                        Some(board_view::Act::Enter(branch)) => {
                            let mut trail = self.trail.clone();
                            trail.push(branch);
                            self.walk_to(trail);
                        }
                        Some(board_view::Act::Up(depth)) => {
                            let mut trail = self.trail.clone();
                            trail.truncate(depth + 1);
                            self.walk_to(trail);
                        }

                        // Opening a branch somewhere else writes a whole
                        // working tree, so it goes off to a thread like the
                        // rest of what takes a moment.
                        Some(board_view::Act::Describe(branch)) => self.describe(branch),
                        Some(board_view::Act::Isolate(branch)) => {
                            let doing = format!("opening {branch} in a folder of its own");
                            self.spawn(&doing, move |repo| repo.add_worktree(&branch));
                        }
                        Some(board_view::Act::Visit(path)) => self.visit(path),
                        Some(board_view::Act::Release(name)) => self.ask_release(name),

                        // The backlog. Writing is instant and local; sending
                        // it out is the only part that goes near a network.
                        Some(board_view::Act::Note(title)) => {
                            let project = self.project.clone();
                            self.note_down(move |repo| {
                                repo.backlog_add(&title, project.as_deref())
                            });
                        }
                        Some(board_view::Act::Start(id)) => {
                            self.note_down(move |repo| repo.backlog_start(&id))
                        }
                        Some(board_view::Act::Drop(id)) => {
                            self.note_down(move |repo| repo.backlog_remove(&id))
                        }
                        Some(board_view::Act::Share) => {
                            self.spawn("sharing the backlog", |repo| repo.backlog_share())
                        }
                        _ => {}
                    }
                }
                Tab::Changes => {
                    let (amendable, pushed) = self.amend_state();
                    let selected = self
                        .selected
                        .as_ref()
                        .map(|(path, staged)| (path.as_str(), *staged));

                    let mut draft = changes_view::Draft {
                        subject: &mut self.subject,
                        body: &mut self.body,
                        amend: &mut self.amend,
                        amendable,
                        pushed: pushed.as_deref(),
                    };

                    let action = changes_view::show(
                        ui,
                        &self.work,
                        selected,
                        self.diff.as_ref(),
                        &mut draft,
                    );

                    if let Some(action) = action {
                        self.handle(action);
                    }
                }
            });

        // Last, over the panels and after whatever has just been pressed, so
        // that a toast asked for this very frame is up before the frame ends.
        // It takes itself away when its time is up.
        if let Some(toast) = &self.toast {
            if !toast::show(ctx, toast) {
                self.toast = None;
            }
        }
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::RAISED)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.repo_menu(ui);
                    theme::vrule(ui);
                    self.tabs(ui);
                    theme::vrule(ui);

                    // Where HEAD is, in the window's one and only amber. It
                    // sits by the actions because they all end up there.
                    if !self.head.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.head)
                                .font(theme::medium(12.0))
                                .color(theme::HEAD)
                                .background_color(theme::tint(theme::HEAD, 28)),
                        );
                        // A breath before the actions: HEAD's amber and the
                        // counts' amber are almost the same, and side by side
                        // they would read as one single stain of color.
                        ui.add_space(11.0);
                    }

                    let action = action_bar::show(
                        ui,
                        self.tracking.as_ref(),
                        &self.stashes,
                        &mut self.pull_mode,
                        self.job.is_some(),
                        &mut self.draft,
                    );

                    let mut reload = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        reload = ui
                            .add_enabled(self.job.is_none(), egui::Button::new("↻"))
                            .on_hover_text("Reload")
                            .clicked();
                    });

                    if let Some(action) = action {
                        self.act(action);
                    } else if reload {
                        self.load();
                    }
                });
            });
    }

    /// Which repository is open, and how to change it.
    ///
    /// Day to day the folder's name is enough. The whole path, which ate a
    /// third of the bar to say almost nothing, has gone to the popup where it
    /// really is used: when opening another repository.
    fn repo_menu(&mut self, ui: &mut egui::Ui) {
        let name = std::path::Path::new(self.path.trim_end_matches('/'))
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".to_string());

        let button =
            ui.button(egui::RichText::new(format!("{name}     ")).font(theme::medium(12.5)));
        theme::caret(ui, &button);
        let hint = self.path.clone();
        let button = button.on_hover_text(hint);
        let id = ui.make_persistent_id("menu-repo");

        if button.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }

        // Cloned out of the way: the path below is being edited through
        // `&mut self`, and the list cannot be borrowed at the same time.
        let trees = self.worktrees.clone();
        let here = std::path::PathBuf::from(&self.path);
        let mut visit = None;

        let open = egui::popup::popup_below_widget(
            ui,
            id,
            &button,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_width(REPO_MENU);
                let mut go = false;

                ui.horizontal(|ui| {
                    let entry = ui.add(
                        egui::TextEdit::singleline(&mut self.path)
                            .desired_width(ui.available_width() - 66.0)
                            .margin(egui::vec2(8.0, 5.0))
                            .font(theme::mono(11.5))
                            .hint_text(
                                egui::RichText::new("path to a repository")
                                    .color(theme::FAINT),
                            ),
                    );
                    go = ui.button("Open").clicked()
                        || (entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                });

                // The other folders this same repository is checked out in.
                // They belong here rather than anywhere else, because this is
                // the control that answers "where am I", and going to one of
                // them is the same move as opening a repository: the window
                // looks somewhere else and nothing is written.
                if trees.len() > 1 {
                    theme::rule(ui);
                    for tree in &trees {
                        if checkout_row(ui, tree, &here) {
                            visit = Some(tree.path.clone());
                        }
                    }
                }

                if go || visit.is_some() {
                    ui.memory_mut(|memory| memory.close_popup());
                }
                go
            },
        )
        .unwrap_or(false);

        if let Some(path) = visit {
            return self.visit(path);
        }
        if open {
            self.selected = None;
            self.diff = None;
            self.selected_commit = None;
            self.detail = None;
            // Another repository is somewhere you have not gone into yet,
            // even when it happens to have a branch of the same name.
            self.trail.clear();
            self.project = None;
            self.load();
        }
    }

    /// The two views, flush together like a two-position control.
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let pending = self.work.staged.len() + self.work.unstaged.len();

        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.selectable_value(&mut self.tab, Tab::History, "History");

            let changes = ui.selectable_value(&mut self.tab, Tab::Changes, "Changes");
            ui.add_space(2.0);
            ui.selectable_value(&mut self.tab, Tab::Board, "Board");
            ui.add_space(2.0);
            if pending > 0 {
                ui.label(
                    egui::RichText::new(pending.to_string())
                        .font(theme::mono(10.5))
                        .color(theme::CHANGED)
                        .background_color(theme::tint(theme::CHANGED, 32)),
                )
                .on_hover_text("changes not committed");
            }
            let _ = changes;
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(theme::RAISED)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&self.summary)
                            .font(theme::body(11.5))
                            .color(theme::MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if self.status_failed {
                            theme::REMOVED
                        } else {
                            theme::FAINT
                        };
                        ui.label(
                            egui::RichText::new(&self.status)
                                .font(theme::body(11.5))
                                .color(color),
                        );
                    });
                });
            });
    }

    /// The references come along always, whether you look at the history or
    /// at the changes.
    fn refs_panel(&mut self, ctx: &egui::Context) {
        let mut picked = None;
        // With a detached HEAD there is no branch to bring anything into.
        let on_branch = self.refs.iter().any(|entry| entry.is_head);

        egui::SidePanel::left("refs-panel")
            .resizable(true)
            .default_width(222.0)
            .width_range(170.0..=360.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ctx, |ui| {
                picked = refs_view::show(
                    ui,
                    &self.refs,
                    &mut self.ref_filter,
                    self.selected_commit,
                    on_branch.then_some(self.head.as_str()),
                    self.job.is_some(),
                );
            });

        match picked {
            Some(refs_view::Pick::Show(oid)) => {
                self.reveal_commit(oid);
                self.tab = Tab::History;
            }
            Some(refs_view::Pick::Switch { name, remote }) => self.switch(name, remote),
            Some(refs_view::Pick::Merge(name)) => {
                // It can rewrite the whole tree, so it goes apart.
                let doing = format!("merging {name}");
                self.spawn(&doing, move |repo| repo.merge(&name));
            }
            Some(refs_view::Pick::DeleteBranch { name, remote }) => {
                self.ask_delete_branch(name, remote)
            }
            Some(refs_view::Pick::FastForward(name)) => {
                self.note_down(move |repo| repo.fast_forward_to(&name))
            }
            Some(refs_view::Pick::Rebase(name)) => {
                let doing = format!("rebasing onto {name}");
                self.spawn(&doing, move |repo| repo.rebase_onto(&name));
            }
            Some(refs_view::Pick::Pull) => {
                self.spawn("pulling", move |repo| repo.pull(PullMode::Merge));
            }
            Some(refs_view::Pick::Push(name)) => {
                let doing = format!("pushing {name}");
                self.spawn(&doing, move |repo| repo.push_branch(&name));
            }
            Some(refs_view::Pick::Follow { branch, upstream }) => {
                self.note_down(move |repo| repo.set_upstream(&branch, upstream.as_deref()))
            }
            Some(refs_view::Pick::Isolate(name)) => {
                let doing = format!("opening {name} in a folder of its own");
                self.spawn(&doing, move |repo| repo.add_worktree(&name));
            }
            Some(refs_view::Pick::GoTo(oid)) => {
                let doing = format!("going to {}", &oid.to_string()[..7]);
                self.spawn(&doing, move |repo| repo.checkout_commit(oid));
            }
            Some(refs_view::Pick::BranchAt(oid)) => {
                self.naming = Some(Naming {
                    title: format!("A branch at {}", &oid.to_string()[..7]),
                    hint: "feat/something".to_string(),
                    verb: "Make it".to_string(),
                    draft: String::new(),
                    deed: Named::BranchAt(oid),
                });
            }
            Some(refs_view::Pick::Rename(name)) => {
                self.naming = Some(Naming {
                    title: format!("Rename {name}"),
                    hint: name.clone(),
                    verb: "Rename".to_string(),
                    draft: name.clone(),
                    deed: Named::Rename(name),
                });
            }
            Some(refs_view::Pick::CherryPick(oid)) => {
                let doing = format!("taking {} along", &oid.to_string()[..7]);
                self.spawn(&doing, move |repo| repo.cherry_pick(oid));
            }
            Some(refs_view::Pick::Revert(oid)) => {
                let doing = format!("undoing {}", &oid.to_string()[..7]);
                self.spawn(&doing, move |repo| repo.revert(oid));
            }
            Some(refs_view::Pick::MoveTo(oid, how)) => self.ask_move(oid, how),
            Some(refs_view::Pick::Copy(text)) => {
                ctx.copy_text(text.clone());
                self.said(format!("{text} copied"));
            }
            Some(refs_view::Pick::DeleteTag(name)) => {
                match self.repo().and_then(|repo| repo.delete_tag(&name)) {
                    Ok(()) => {
                        self.load();
                        self.said(format!("tag {name} deleted"));
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            None => {}
        }
    }

    /// Switches branch on a thread of its own.
    ///
    /// It is local, but it is the operation that can write the most files of
    /// all the ones here: between two distant branches half the tree is
    /// rewritten, and with the window frozen that reads as the app having died.
    fn switch(&mut self, name: String, remote: bool) {
        // The tab stays where it is: if you were in the changes you were
        // working on files, and switching branch is no reason to send you to
        // another screen.
        //
        // The cursor is let go before leaving, though: on reaching the new
        // branch what you want to see is its last commit, not the place in the
        // old branch where you had been looking.
        self.selected_commit = None;
        self.detail_file = None;

        let doing = format!("switching to {name}");
        self.spawn(&doing, move |repo| {
            if remote {
                repo.checkout_remote(&name)
            } else {
                repo.checkout_branch(&name)
            }
        });
    }

    fn detail_panel(&mut self, ctx: &egui::Context) {
        // With the changes row picked, the right panel becomes the whole
        // working tree: lists, commit box and diff. That way you stage and
        // commit without leaving the graph, which is what makes the two views
        // feel like one.
        let working = self.selected_commit.is_some_and(|oid| oid.is_zero());
        let (amendable, pushed) = self.amend_state();
        let mut clicked = None;
        let mut action = None;

        egui::SidePanel::right("detail")
            .resizable(true)
            .default_width(420.0)
            // Capped: without it, a commit with a long message widens the
            // panel until the graph's column is nothing.
            .width_range(300.0..=520.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ctx, |ui| {
                if working {
                    let selected = self
                        .selected
                        .as_ref()
                        .map(|(path, staged)| (path.as_str(), *staged));

                    let mut draft = changes_view::Draft {
                        subject: &mut self.subject,
                        body: &mut self.body,
                        amend: &mut self.amend,
                        amendable,
                        pushed: pushed.as_deref(),
                    };

                    action = changes_view::compact(
                        ui,
                        &self.work,
                        selected,
                        self.diff.as_ref(),
                        &mut draft,
                    );
                } else {
                    clicked = detail_view::show(
                        ui,
                        self.detail.as_ref(),
                        self.detail_file.as_deref(),
                        self.detail_diff.as_ref(),
                        &mut self.tag,
                    );
                }
            });

        if let Some(action) = action {
            self.handle(action);
        }
        match clicked {
            Some(detail_view::Pick::File(path)) => self.select_detail_file(path),
            Some(detail_view::Pick::Tag { name, message }) => self.create_tag(name, message),
            None => {}
        }
    }

    /// Tags the commit being looked at.
    fn create_tag(&mut self, name: String, message: String) {
        let Some(oid) = self.selected_commit.filter(|oid| !oid.is_zero()) else {
            return self.note_error("no commit is selected".to_string());
        };

        match self.repo().and_then(|repo| repo.tag(&name, oid, &message)) {
            Ok(()) => {
                // The new tag has to show up in the graph and in the sidebar,
                // and that only happens by reading the references again.
                self.load();
                self.said(format!("tag {name} created"));
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }
}

/// One checkout in the repository menu: where it is and what is open there.
///
/// The one you are looking at wears HEAD's amber, the same as the branch does
/// in the bar, because it is the same question answered in the same color:
/// this is where you are.
fn checkout_row(ui: &mut egui::Ui, tree: &Worktree, here: &std::path::Path) -> bool {
    let current = tree.path == here;
    let name = match tree.main {
        true => format!("{} (main)", tree.name),
        false => tree.name.clone(),
    };

    let mut go = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            egui::RichText::new(name)
                .font(theme::medium(12.0))
                .color(if current { theme::HEAD } else { theme::TEXT }),
        );

        match (&tree.branch, tree.missing) {
            // Git kept the bookkeeping and the folder is gone, which is what
            // somebody deleting it by hand leaves behind.
            (_, true) => ui.label(
                egui::RichText::new("folder is gone")
                    .font(theme::body(11.0))
                    .color(theme::REMOVED),
            ),
            (Some(branch), _) => ui.label(
                egui::RichText::new(branch).font(theme::mono(11.0)).color(theme::MUTED),
            ),
            (None, _) => ui.label(
                egui::RichText::new("detached").font(theme::body(11.0)).color(theme::FAINT),
            ),
        };

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if current {
                ui.label(
                    egui::RichText::new("you are here")
                        .font(theme::body(11.0))
                        .color(theme::FAINT),
                );
                return;
            }
            if tree.missing {
                return;
            }
            go = ui
                .button(egui::RichText::new("Go").font(theme::body(11.0)))
                .on_hover_text(tree.path.display().to_string())
                .clicked();
        });
    });
    ui.add_space(3.0);

    go
}
