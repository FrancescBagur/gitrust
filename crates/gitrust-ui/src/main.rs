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

use anyhow::Result;
use eframe::egui;
use gitrust_core::{
    compose, layout, layout_with_working, Board, CommitDetail, FileDiff, Graph, HunkId, Oid,
    PullMode, RefEntry, RefKind, Repo, StashEntry, Tracking, WorkingState,
};

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
    Land(String),
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
        self.board = repo.board().ok();
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
                        self.status = note;
                        self.status_failed = false;
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

    /// Asks before bringing a branch into the main one, which may start by
    /// moving you somewhere else.
    fn ask_land(&mut self, branch: String) {
        let trunk = match &self.board {
            Some(board) => board.trunk.clone(),
            None => return,
        };

        let detail = if self.head == trunk {
            format!("{branch} is merged into {trunk}. If they clash, nothing is touched and it says where.")
        } else {
            format!(
                "You move to {trunk} first, and then {branch} is merged into it. \
                 If they clash, nothing is merged and you stay on {trunk}."
            )
        };

        self.ask(
            format!("Bring {branch} into {trunk}?"),
            &detail,
            "Merge",
            Deed::Land(branch),
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
            Deed::Land(branch) => {
                let doing = format!("merging {branch}");
                return self.spawn(&doing, move |repo| repo.land(&branch));
            }
            Deed::DeleteBranch { name, .. } => {
                return match self.repo().and_then(|repo| repo.delete_branch(&name)) {
                    Ok(()) => {
                        // The references change, so the graph and the sidebar
                        // have to be read again.
                        self.load();
                        self.status = format!("branch {name} deleted");
                        self.status_failed = false;
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                };
            }
        };

        if discarded {
            self.status = "changes discarded".to_string();
            self.status_failed = false;
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

    /// Leaves a note in the bottom bar, marked as a failure.
    fn note_error(&mut self, message: String) {
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
                        self.status = "last commit redone".to_string();
                        self.status_failed = false;
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
                        self.status = format!("commit created on {}", self.head);
                        self.status_failed = false;
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
                        self.status = format!("you are now on {name}");
                        self.status_failed = false;
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
                self.status = note;
                self.status_failed = false;
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.top_bar(ctx);
        self.confirm_dialog(ctx);
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
                    let picked = match &self.board {
                        Some(board) => board_view::show(
                            ui,
                            board,
                            &mut self.project,
                            self.job.is_some(),
                        ),
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
                        Some(board_view::Act::Publish(branch)) => {
                            let doing = format!("pushing {branch}");
                            self.spawn(&doing, move |repo| repo.push_branch(&branch));
                        }
                        Some(board_view::Act::Land(branch)) => self.ask_land(branch),
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

        let open = egui::popup::popup_below_widget(
            ui,
            id,
            &button,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(430.0);
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

                if go {
                    ui.memory_mut(|memory| memory.close_popup());
                }
                go
            },
        )
        .unwrap_or(false);

        if open {
            self.selected = None;
            self.diff = None;
            self.selected_commit = None;
            self.detail = None;
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
            Some(refs_view::Pick::DeleteTag(name)) => {
                match self.repo().and_then(|repo| repo.delete_tag(&name)) {
                    Ok(()) => {
                        self.load();
                        self.status = format!("tag {name} deleted");
                        self.status_failed = false;
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
                self.status = format!("tag {name} created");
                self.status_failed = false;
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }
}
