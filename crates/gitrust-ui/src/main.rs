//! gitrust: a desktop Git client.
//!
//! The window follows the layout of the usual graphical clients: references
//! on the left, the graph in the middle and the detail of what is picked on
//! the right, so that clicking in the graph shows the diff without changing
//! screen.

// On Windows a program is either a console program or a window program, and
// the default is the console one: a black terminal opens behind the window
// and sits there for as long as the window is up. Released builds say they
// are a window; a debug build keeps the console, because that is where a
// panic goes and losing it is losing the one thing worth reading. Ignored
// everywhere else.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod action_bar;
mod board_view;
mod changes_view;
mod chooser;
mod conflict_view;
mod detail_view;
mod diff_view;
mod file_view;
mod graph_view;
mod picker;
mod prefs_view;
mod profiles_view;
mod refs_view;
mod rewrite_view;
mod ripple;
mod syntax;
mod terminal;
mod terminal_view;
mod theme;
mod toast;
mod updates;
mod watch;

use anyhow::Result;
use eframe::egui;
use gitrust_core::{
    compose, layout, layout_with_working, likely_target, Appearance, Backlog, Board, CommitDetail, FileDiff,
    Foreseen, General, Graph, HunkId, Join, Move, Oid, Panel, Profile, Profiles, PullMode, PullRequests, Reader,
    Pulse, RefEntry, RefKind, RemoteRepo, Repo, Settings, StashEntry, Tracking, WorkingState, Worktree, Writer,
};

/// How wide the repository menu is, measured inside its frame.
///
/// Told, not asked for, for the same reason the action bar's menus are: a
/// menu that works its own width out from the room it is given answers a
/// little differently every frame, and the answers drift. Wide enough for a
/// path you can read and for the folders listed under it.
const REPO_MENU: f32 = 520.0;

/// How wide the menu behind the profile in the tab strip is. Told, not asked
/// for, for the reason the repository menu above is: a menu that works its
/// own width out drifts a notch wider every time it is opened.
const PROFILE_MENU: f32 = 240.0;

/// How wide the activity log opens.
const LOG_MENU: f32 = 560.0;

/// How many lines the activity log keeps. It is a memory of this sitting
/// and not a file, and past a few hundred lines nobody scrolls back.
const LOG_LINES: usize = 400;

/// How wide the menu a drop opens is.
///
/// Set, not merely asked for: a minimum leaves egui free to remember a wider
/// rectangle from last time and hand it back a notch wider on every opening,
/// which is exactly what the repository menu above used to do.
const JOIN_MENU: f32 = 300.0;

/// How many commits are walked looking for the ones that touched a file.
///
/// Git keeps no history for a file, so this is found by reading commits one
/// by one. The number is a guess at where somebody stops caring, and the view
/// says out loud when it was reached rather than handing back a short answer
/// that looks complete.
const FILE_HISTORY: usize = 4_000;

/// The window's icon, carried inside the binary so that the program run
/// straight out of `target/release` looks like itself and not like a
/// nameless window. 256 is the largest any title bar or switcher asks for.
const ICON: &[u8] = include_bytes!("../../../assets/icon-256.png");

/// The line the project goes by, where somebody reads it before anything
/// else: the title bar, and whatever the desktop's switcher puts under the
/// icon.
///
/// It says what the two halves of the name are for. Trust is git's own
/// promise -- a hash that cannot be quietly changed -- and this program is
/// here to let somebody see it rather than take it on faith. The README
/// says the same at the length a README has.
const SLOGAN: &str = "Trust, protected. Code, eternal.";

/// A compact three-state appearance control with icons drawn by us rather
/// than borrowed from whatever glyphs the platform happens to provide.
fn appearance_control(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let before = settings.appearance;
    ui.allocate_ui_with_layout(
        egui::vec2(82.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            egui::Frame::NONE
                .fill(theme::sunken())
                .stroke(egui::Stroke::new(1.0, theme::edge()))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(2, 2))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if appearance_icon(ui, Appearance::Light, settings.appearance == Appearance::Light)
                        .on_hover_text("Sol — tema claro")
                        .clicked()
                    {
                        settings.appearance = Appearance::Light;
                    }
                    if appearance_icon(ui, Appearance::Dark, settings.appearance == Appearance::Dark)
                        .on_hover_text("Luna — tema oscuro")
                        .clicked()
                    {
                        settings.appearance = Appearance::Dark;
                    }
                    if appearance_icon(ui, Appearance::Auto, settings.appearance == Appearance::Auto)
                        .on_hover_text("Auto — seguir el sistema")
                        .clicked()
                    {
                        settings.appearance = Appearance::Auto;
                    }
                });
        },
    );
    settings.appearance != before
}

fn appearance_icon(ui: &mut egui::Ui, appearance: Appearance, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 18.0), egui::Sense::click());
    let painter = ui.painter();
    let selected_fill = ui.visuals().selection.bg_fill;
    let background = if selected { selected_fill } else { theme::sunken() };
    if selected || response.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(3), if selected { selected_fill } else { theme::hover() });
    }

    let center = rect.center();
    let ink = theme::text();
    let stroke = egui::Stroke::new(1.35, ink);
    match appearance {
        Appearance::Light => {
            painter.circle_stroke(center, 3.3, stroke);
            for step in 0..8 {
                let angle = std::f32::consts::TAU * step as f32 / 8.0;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment([center + direction * 5.2, center + direction * 7.0], stroke);
            }
        }
        Appearance::Dark => {
            painter.circle_filled(center, 5.3, ink);
            painter.circle_filled(center + egui::vec2(2.4, -1.7), 5.3, background);
        }
        Appearance::Auto => {
            let radius = 5.3;
            let mut half = vec![center];
            for step in 0..=10 {
                let angle = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * step as f32 / 10.0;
                half.push(center + egui::vec2(angle.cos(), angle.sin()) * radius);
            }
            painter.add(egui::Shape::convex_polygon(half, ink, egui::Stroke::NONE));
            painter.circle_stroke(center, radius, stroke);
        }
    }
    response
}

/// What `--help` prints, and what `--version` prints the first line of. A
/// graphical program still gets run from a terminal once -- by whoever is
/// about to report a bug and is asked which version they have -- and a
/// window that opens on a repository called `--version` is the wrong answer
/// to that question.
fn tell_and_stop(arg: &str) -> bool {
    match arg {
        "-V" | "--version" => {
            println!("gitrust {}", env!("CARGO_PKG_VERSION"));
            true
        }
        "-h" | "--help" => {
            println!(
                "gitrust {} \u{2014} {}\n\n\
                 Usage: gitrust [REPOSITORY]\n\n\
                 With no argument it opens the tabs that were open last time,\n\
                 and the repository in the current directory if there is one.\n\
                 A folder dragged onto the window opens too.\n\n\
                 \x20 -V, --version   print the version and stop\n\
                 \x20 -h, --help      print this and stop\n\n\
                 {}\n",
                env!("CARGO_PKG_VERSION"),
                SLOGAN,
                env!("CARGO_PKG_REPOSITORY"),
            );
            true
        }
        _ => false,
    }
}

fn main() -> eframe::Result<()> {
    // Kept as "none was given" rather than turned into the current folder
    // here: with the tabs remembered, a window started from an icon has
    // somewhere better to open than wherever the desktop happened to be.
    let path = std::env::args().nth(1);

    if path.as_deref().is_some_and(tell_and_stop) {
        return Ok(());
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1500.0, 900.0])
        .with_min_inner_size([900.0, 480.0])
        // Capitalised here and nowhere else. This is the name read by a
        // person, in a title bar and a window switcher, among the names of
        // every other program they have open -- all of which are
        // capitalised. Everywhere a machine reads it instead -- the binary,
        // the crate, the app id below -- it stays lowercase, because those
        // are not names, they are identifiers that have to match.
        .with_title(format!("Gitrust \u{2014} {SLOGAN}"))
        // What the desktop files this window under. It has to be the name of
        // the .desktop file in packaging/ word for word: on Wayland the icon
        // comes from that file and never from the window, so the one below
        // is for X11 and for Windows, and this line is what gets an icon on
        // the dock everywhere else.
        .with_app_id("gitrust");

    // On macOS the native traffic lights stay in place, while the content
    // continues behind the transparent title bar. That gives them the same
    // application colour as the tab strip instead of a separate white band.
    #[cfg(target_os = "macos")]
    {
        viewport = viewport
            // The native title would be painted above our tab strip. The app
            // id still identifies the window in the switcher and dock.
            .with_title("")
            .with_titlebar_shown(false)
            .with_fullsize_content_view(true);
    }

    // Reading it costs no new dependency: the decoder is already in here,
    // pulled in by eframe. If it ever fails to read, the window goes up
    // without an icon -- the wrong picture on a title bar is not a reason to
    // refuse to open a repository.
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON) {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "gitrust",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, path)))),
    )
}

/// The window: the repositories open in it, and which one is being looked at.
///
/// Everything else lives in [`Open`], and that is not a judgement call -- when
/// the time came to split this in two, every single thing the window
/// remembered turned out to be about a repository: the graph, the working
/// tree, what is half-typed into the commit box, the operation running off to
/// one side. Nothing was left over to be shared. So tabs are a list of those
/// and a finger on one of them, and not a layer over something common.
struct App {
    open: Vec<Open>,
    /// Which one is on top. Kept inside the list by `close`, the only thing
    /// that can make the list shorter.
    at: usize,
    /// How many tabs have ever been opened. It is where each one's egui salt
    /// comes from, so it counts up and never counts down.
    opened: u64,
    /// The screen for choosing a repository, while it is up. The plus opens
    /// it; it is the one screen here that is not about a repository.
    picking: Option<picker::Picker>,
    /// The repositories this machine knows, read when the screen opens so the
    /// rows are not asking the disk sixty times a second.
    known: Vec<gitrust_core::Known>,
    /// What each of them was at, worked out at the same time.
    glance: std::collections::HashMap<std::path::PathBuf, String>,
    /// A clone on its way down, when one is.
    cloning: Option<Cloning>,
    /// Something being chosen in the desktop's own dialog, and what it was
    /// opened for.
    browsing: Option<(Browsing, std::sync::mpsc::Receiver<Result<Option<std::path::PathBuf>>>)>,
    /// Who this machine works as, and who it is working as right now. It is
    /// held here for the reason the settings are: it is about the machine
    /// and not about a repository, and every tab works under the same one.
    profiles: Profiles,
    /// The screen for the profiles, while it is up.
    managing: Option<profiles_view::Managing>,
    /// The screen for the preferences, while it is up.
    preferring: Option<prefs_view::Preferring>,
    /// When the window last fetched without being asked. The next one is
    /// counted from here, so it starts at "just now": a window that fetched
    /// the moment it opened would be a window that asks for a passphrase
    /// before it has drawn anything.
    fetched: std::time::Instant,
    /// What looks at the repository in front while nobody is pressing
    /// anything. Started with the first frame, which is the first time
    /// there is a window to wake.
    watch: Option<watch::Watch>,
    /// Whether there is a newer version than this one, and the file for
    /// it if somebody has asked for that.
    updates: updates::Updates,
    /// What one of the screens that is not a repository has to say. A tab's
    /// own bottom bar cannot say it: the tab is not on screen.
    note: Option<Note>,
    /// What somebody has said they prefer. It is about this computer, not
    /// about a repository, so it is held here and lent to whichever tab is
    /// on top rather than being a copy per tab that would drift apart.
    settings: Settings,
}

/// Waits for a repository's turn, and gives up waiting after a while.
///
/// What comes back is the turn itself, held until it is dropped, or nothing
/// when the wait ran out -- and the caller goes ahead either way. A lock
/// somebody panicked under is still a lock nobody holds.
fn wait_for(
    turn: &std::sync::Mutex<()>,
    longest: std::time::Duration,
) -> Option<std::sync::MutexGuard<'_, ()>> {
    let started = std::time::Instant::now();
    loop {
        match turn.try_lock() {
            Ok(held) => return Some(held),
            Err(std::sync::TryLockError::Poisoned(held)) => return Some(held.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) if started.elapsed() >= longest => return None,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

/// Whether two profiles are the same person, whatever is filed under them.
fn same_person(one: Option<&Profile>, other: Option<&Profile>) -> bool {
    match (one, other) {
        (None, None) => true,
        (Some(one), Some(other)) => {
            one.name == other.name
                && one.author == other.author
                && one.email == other.email
                && one.key == other.key
                && one.accounts == other.accounts
        }
        _ => false,
    }
}

/// The key a profile would open a connection with, when it has one that is
/// really there. A path to a file that has gone is no key, and handing it
/// over would turn "the agent would have worked" into a failure.
fn profile_key(profile: Option<&Profile>) -> Option<std::path::PathBuf> {
    profile
        .and_then(Profile::key_file)
        .filter(|key| key.exists())
}

/// A line for a screen that has no bottom bar of its own.
///
/// Two kinds, because they are read differently: a complaint is something to
/// act on and is red, and a word about something that worked is grey and is
/// only there because the thing that worked left nothing on screen to show
/// for it -- a key written into a file, a line put on the clipboard.
#[derive(Clone)]
enum Note {
    Said(String),
    Wrong(String),
}

/// What the desktop's dialog was opened for, so that the answer knows where
/// to go when it comes back minutes later.
#[derive(Clone, Copy)]
enum Browsing {
    /// A folder for the repository screen. `beside` picks which box: the one
    /// a clone lands in, or the one above it.
    Folder { beside: bool },
    /// A private key for a profile.
    Key { which: usize },
    /// A private key for the remote checkout being connected to.
    RemoteKey,
}

/// A repository being brought down, and where it is going.
struct Cloning {
    into: std::path::PathBuf,
    done: std::sync::mpsc::Receiver<Result<String>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, asked: Option<String>) -> Self {
        let settings = Settings::load();
        theme::install(&cc.egui_ctx, settings.appearance);
        let mut app = Self {
            open: Vec::new(),
            at: 0,
            opened: 0,
            picking: None,
            known: Vec::new(),
            glance: std::collections::HashMap::new(),
            cloning: None,
            browsing: None,
            settings,
            profiles: Profiles::load(),
            managing: None,
            preferring: None,
            fetched: std::time::Instant::now(),
            watch: None,
            updates: updates::Updates::new(),
            note: None,
        };
        app.come_back(asked);
        app
    }

    /// Opens what was open last time, and then what was asked for.
    ///
    /// What was asked for on the command line goes in front, in a tab of
    /// its own if it was not one of them. With nothing asked for, the
    /// repository in the current folder does the same when there is one --
    /// `gitrust` typed in a terminal still opens where it was typed -- and
    /// when there is not, the tabs come back as they were left. Only a
    /// window with nothing at all to show falls back to opening the current
    /// folder and saying what is wrong with it, which is what it always did.
    fn come_back(&mut self, asked: Option<String>) {
        if self.settings.general.remember_tabs {
            for place in self.settings.tabs.clone() {
                self.reopen(&place);
            }
            // Found again by what it was, not by its position: a folder that
            // has gone since is left out, and every tab after it has moved
            // one to the left.
            let front = self.settings.tabs.get(self.settings.front);
            self.at = self
                .open
                .iter()
                .position(|open| open.place.as_ref() == front)
                .unwrap_or(0);
        }

        match asked {
            Some(path) => self.open_asked(path),
            None if self.open.is_empty() => self.add(".".to_string()),
            None => {
                if let Ok(here) = Repo::open(".") {
                    self.open_asked(here.workdir().display().to_string());
                }
            }
        }
    }

    /// Puts back one tab that was open last time.
    ///
    /// Quietly, both ways. A folder that is no longer a repository is left
    /// out rather than brought back as a tab that says so: it was deleted
    /// on purpose, by somebody who does not need telling at every start.
    /// And one that is still there is not written down as opened just now,
    /// nor filed under whichever profile is in use: nobody opened it.
    fn reopen(&mut self, place: &str) {
        let open = match self.settings.remote_at(place) {
            // Not connected, and not connecting: that is a wait of up to
            // fifteen seconds for each one, with the window not yet drawn.
            Some(remote) => Some(Open::waiting(remote.clone(), self.opened, &self.settings.general)),
            None if place.starts_with("ssh://") => None,
            None => Repo::open(place)
                .ok()
                .map(|_| Open::new(place.to_string(), self.opened, &self.settings.general)),
        };
        if let Some(open) = open {
            self.open.push(open);
            self.opened += 1;
        }
    }

    /// Opens a path given on the command line, or goes to its tab.
    fn open_asked(&mut self, path: String) {
        let place = gitrust_core::place_of(std::path::Path::new(&path));
        let already = place.as_ref().and_then(|place| {
            self.open
                .iter()
                .position(|open| open.place.as_ref() == Some(place))
        });
        match already {
            Some(which) => self.at = which,
            None => self.add(path),
        }
    }

    /// Writes down which tabs are open, when that has changed.
    ///
    /// Asked every frame and answered almost never: the list is compared
    /// with the one already kept and the file is touched only when they
    /// differ. With the preference off the list is emptied once, so that
    /// what was open the day it was turned off is not what comes back the
    /// day it is turned on again.
    fn note_tabs(&mut self) {
        let (tabs, front) = match self.settings.general.remember_tabs {
            true => {
                let front = self.open[..self.at.min(self.open.len())]
                    .iter()
                    .filter(|open| open.place.is_some())
                    .count();
                let tabs: Vec<String> =
                    self.open.iter().filter_map(|open| open.place.clone()).collect();
                (tabs, front)
            }
            false => (Vec::new(), 0),
        };
        if self.settings.note_tabs(tabs, front) {
            // Not said when it fails. It is tried again at the next change,
            // and a line about a list of tabs is not worth a toast over
            // whatever somebody is in the middle of.
            let _ = self.settings.save();
        }
    }

    /// Fetches the repository in front, when it has been long enough.
    ///
    /// The one in front and not all of them: it is the one being looked at,
    /// and a window with a dozen tabs would otherwise be a dozen connections
    /// a minute to keep up to date what nobody is reading.
    fn auto_fetch(&mut self, ctx: &egui::Context) {
        let minutes = u64::from(self.settings.general.auto_fetch);
        if minutes == 0 {
            return;
        }
        let every = std::time::Duration::from_secs(60 * minutes);
        let waited = self.fetched.elapsed();
        if waited < every {
            // egui sleeps when nothing moves. Without this the minute would
            // be up whenever somebody next touched the mouse.
            ctx.request_repaint_after(every - waited);
            return;
        }
        self.fetched = std::time::Instant::now();
        if let Some(open) = self.open.get_mut(self.at) {
            open.fetch_quietly();
        }
        ctx.request_repaint_after(every);
    }

    /// Does what was pressed in the menu under the word for a newer
    /// version.
    ///
    /// Opening is handing something to the desktop, and the one thing
    /// that can go wrong with it is that there is nothing to hand it to.
    /// That is said on whichever screen is up.
    fn act_on_update(&mut self, ctx: &egui::Context, act: updates::Act) {
        let opened = match act {
            updates::Act::Fetch => {
                self.updates.fetch(ctx);
                return;
            }
            updates::Act::Skip(tag) => {
                self.settings.skipped = tag;
                if let Err(err) = self.settings.save() {
                    self.complain(err);
                }
                return;
            }
            updates::Act::Open(file) => updates::open_outside(file),
            updates::Act::Folder(folder) => updates::open_outside(folder),
            updates::Act::Page(address) => updates::open_outside(address),
        };
        if let Err(err) = opened {
            self.complain(anyhow::anyhow!("could not open it: {err}"));
        }
    }

    /// Keeps the repository in front up to date with what other programs
    /// do to it: an editor, a terminal, an agent.
    ///
    /// The looking is done off this thread, by [`watch::Watch`], which
    /// says so when the repository is not as it last found it. What it
    /// says goes to the tab the folder belongs to, and the tab decides
    /// whether that is news.
    ///
    /// Only the one in front, like the fetch nobody asked for and for the
    /// same reason. The others are looked at the moment they are brought
    /// forward, which is the moment being out of date starts to matter.
    /// Not a checkout on another machine, and not a window that has been
    /// put away, which has nobody to be up to date for.
    fn keep_up(&mut self, ctx: &egui::Context) {
        let (near, away) = ctx.input(|input| (input.focused, input.viewport().minimized == Some(true)));
        let front = self
            .open
            .get(self.at)
            .filter(|open| !open.is_remote() && !away)
            .map(|open| std::path::PathBuf::from(&open.path));
        let watch = self.watch.get_or_insert_with(|| watch::Watch::start(ctx.clone()));
        watch.look_at(front.as_deref(), near);
        for (folder, seen) in watch.heard() {
            let found = self
                .open
                .iter_mut()
                .find(|open| !open.is_remote() && std::path::Path::new(&open.path) == folder);
            if let Some(open) = found {
                open.heard(seen);
            }
        }
    }

    /// Hands every tab the preferences, once the screen for them is away.
    ///
    /// Not while it is up: a number being dragged is forty numbers on the
    /// way to the one that was wanted, and each of them would be every tab
    /// reading its history again.
    fn hand_preferences(&mut self) {
        if self.preferring.is_some() {
            return;
        }
        for open in &mut self.open {
            open.prefer(&self.settings.general);
        }
    }

    /// Opens a folder in a tab of its own, or goes to the tab already on it.
    ///
    /// Two tabs onto one working tree would be two windows onto one set of
    /// files with nothing to tell them apart, and the second one would
    /// quietly disagree with the first as soon as anything was staged.
    fn open_tab(&mut self, path: std::path::PathBuf) {
        match self
            .open
            .iter()
            .position(|open| std::path::Path::new(&open.path) == path)
        {
            Some(which) => {
                self.at = which;
                self.picking = None;
            }
            None => self.add(path.display().to_string()),
        }
    }

    fn add(&mut self, path: String) {
        self.open.push(Open::new(path, self.opened, &self.settings.general));
        self.opened += 1;
        self.at = self.open.len() - 1;
        self.picking = None;
        self.remember(self.at);
        self.file_under_profile();
    }

    fn add_remote(&mut self, session: gitrust_core::ssh::Session, path: String, key: Option<String>) {
        match RemoteRepo::new(session, path) {
            Ok(mut repo) => {
                repo.keyed(key);
                self.settings.remember_remote(repo.descriptor());
                let saved = self.settings.save();
                self.open.push(Open::new_remote(repo, self.opened, &self.settings.general));
                self.opened += 1;
                self.at = self.open.len() - 1;
                self.picking = None;
                self.file_under_profile();
                if let Err(err) = saved {
                    self.complain(err);
                }
            }
            Err(err) => self.complain(err),
        }
    }

    /// Writes down that a repository was opened, so the screen can offer it
    /// again next time.
    ///
    /// A path that is not a repository is not remembered and does not
    /// complain: the tab it was opened in will say what is wrong with it,
    /// and saying it twice in two places helps nobody.
    fn remember(&mut self, which: usize) {
        let Some(open) = self.open.get(which) else {
            return;
        };
        let path = std::path::PathBuf::from(&open.path);
        if Repo::open(&open.path).is_err() {
            return;
        }
        let now = chrono::Local::now().timestamp();
        let _ = gitrust_core::known::seen(&path, now);
    }

    /// Opens the screen, reading the list and a line about each one.
    ///
    /// Done here, once, rather than in the view: a row that asked the disk
    /// what branch it was on would ask it again every frame, for every
    /// repository on the list.
    fn pick(&mut self) {
        self.known = gitrust_core::known::all();
        self.glance = self
            .known
            .iter()
            .filter(|known| known.there())
            .filter_map(|known| {
                let repo = Repo::open(&known.path.display().to_string()).ok()?;
                let head = repo.head_name();
                let line = match repo.tracking() {
                    Some(t) if t.ahead > 0 && t.behind > 0 => {
                        format!("{head}  \u{2191}{}  \u{2193}{}", t.ahead, t.behind)
                    }
                    Some(t) if t.ahead > 0 => format!("{head}  \u{2191}{}", t.ahead),
                    Some(t) if t.behind > 0 => format!("{head}  \u{2193}{}", t.behind),
                    _ => head,
                };
                Some((known.path.clone(), line))
            })
            .collect();
        self.picking = Some(picker::Picker::default());
        self.preferring = None;
    }

    /// Acts on what the screen asked for.
    fn act_on_pick(&mut self, act: picker::Act) {
        match act {
            picker::Act::Open(path) => self.open_tab(path),
            picker::Act::OpenRemote { session, path, key } => self.add_remote(session, path, key),
            picker::Act::Start(path) => match gitrust_core::start::init(
                &path,
                Some(&self.settings.general.default_branch),
            ) {
                Ok(_) => self.add(path.display().to_string()),
                Err(err) => self.complain(err),
            },
            // The only slow one, and the only one that goes near a network.
            // It runs where it will not hold the window still, and the tab it
            // lands in is opened when it is there.
            picker::Act::Clone { url, into } => self.start_clone(url, into),
            picker::Act::Star { path, starred } => {
                let _ = gitrust_core::known::star(&path, starred);
                self.known = gitrust_core::known::all();
            }
            picker::Act::Forget(path) => {
                let _ = gitrust_core::known::forget(&path);
                self.known = gitrust_core::known::all();
            }
            picker::Act::Close => self.picking = None,
            picker::Act::Browse { beside } => self.browse(Browsing::Folder { beside }),
            picker::Act::BrowseKey => self.browse(Browsing::RemoteKey),
        }
    }

    /// Asks the desktop to put its own folder chooser up.
    ///
    /// On a thread, because it does not come back until somebody has looked
    /// at the dialog and decided, and that can be minutes.
    /// Whether the screen is asking where to start an empty repository.
    fn starting(&self) -> bool {
        self.picking.as_ref().is_some_and(|state| state.starting())
    }

    fn browse(&mut self, what: Browsing) {
        if self.browsing.is_some() {
            return;
        }
        let (send, done) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(match what {
                Browsing::Folder { .. } => chooser::folder("Choose a repository folder"),
                Browsing::Key { .. } | Browsing::RemoteKey => chooser::file(
                    "Choose a private key",
                    gitrust_core::home::folder().map(|home| home.join(".ssh")),
                ),
            });
        });
        self.browsing = Some((what, done));
    }

    fn poll_browse(&mut self, ctx: &egui::Context) {
        let Some((what, done)) = &self.browsing else {
            return;
        };
        let what = *what;
        match done.try_recv() {
            Ok(found) => {
                self.browsing = None;
                match found {
                    // Cancelled. An ordinary answer, and nothing to say about it.
                    Ok(None) => {}
                    // A key is the one answer that is not about a repository:
                    // it goes straight into the profile that asked for it.
                    Ok(Some(path)) if matches!(what, Browsing::Key { .. }) => {
                        let Browsing::Key { which } = what else { return };
                        if let Some(profile) = self.profiles.list.get_mut(which) {
                            profile.key = path.display().to_string();
                            self.keep_profiles();
                        }
                    }
                    // And the other key goes into the box that asked: the
                    // connection is still to be made, by pressing Connect.
                    Ok(Some(path)) if matches!(what, Browsing::RemoteKey) => {
                        if let Some(state) = &mut self.picking {
                            state.take_key(&path);
                        }
                    }
                    // Choosing a folder in a dialog called "choose a
                    // repository folder" is the decision. Putting it in a box
                    // and waiting for a second Open was a step that asked
                    // somebody to say yes to what they had just said yes to.
                    //
                    // The two boxes that are not that are left filled in
                    // instead: a clone still needs its address, and starting
                    // an empty repository usually means a folder inside the
                    // one that was picked, with a name yet to be typed.
                    Ok(Some(path)) => match (matches!(what, Browsing::Folder { beside: true }), self.starting()) {
                        (false, false) => self.act_on_pick(picker::Act::Open(path)),
                        _ => {
                            if let Some(state) = &mut self.picking {
                                let box_ = match matches!(what, Browsing::Folder { beside: true }) {
                                    true => &mut state.beside,
                                    false => &mut state.draft,
                                };
                                *box_ = path.display().to_string();
                            }
                        }
                    },
                    Err(err) => self.complain(err),
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.browsing = None,
        }
    }

    /// A folder dragged onto the window from a file manager.
    ///
    /// Free, and it works everywhere: eframe hands over whatever was dropped
    /// without anybody asking a desktop for anything. It is the shortest road
    /// from "that folder there" to a tab, and on a machine with no portal it
    /// is the only one that is not typing.
    fn poll_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });

        for path in dropped {
            // A file inside a repository is somebody aiming at the repository
            // and hitting one of its files, which is easy to do and plainly
            // meant. A file on its own is not a repository and says so.
            let folder = match path.is_dir() {
                true => path,
                false => match path.parent() {
                    Some(parent) => parent.to_path_buf(),
                    None => continue,
                },
            };
            match Repo::open(&folder.display().to_string()) {
                Ok(_) => self.act_on_pick(picker::Act::Open(folder)),
                Err(err) => self.complain(err),
            }
        }
    }

    fn complain(&mut self, err: anyhow::Error) {
        if let Some(open) = self.open.get_mut(self.at) {
            open.note_error(format!("{err:#}"));
        }
    }

    /// Takes a tab away.
    ///
    /// The last one does not close. A window with no repository in it would
    /// have nothing to draw and nothing to say, and "quit" is a thing the
    /// window manager already does better.
    fn close(&mut self, which: usize) {
        if self.open.len() < 2 {
            return;
        }
        self.open.remove(which);
        // Standing on a tab to the right of the one that went, you were
        // looking at a repository that has now shifted one place left.
        if self.at >= which {
            self.at = self.at.saturating_sub(1);
        }
        self.at = self.at.min(self.open.len() - 1);
    }

    /// The strip of tabs, and the box for opening another.
    fn strip(&mut self, ctx: &egui::Context) {
        let mut go = None;
        let mut shut = None;
        let mut toggle_maximized = false;
        let mut newer = None;

        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::NONE
                    .fill(theme::canvas())
                    .inner_margin(egui::Margin {
                        // Full-size macOS content sits behind the traffic
                        // lights. Leave their hit area empty while retaining
                        // the tab strip's background behind them.
                        left: if cfg!(target_os = "macos") { 76 } else { 8 },
                        right: 8,
                        top: 5,
                        bottom: 0,
                    }),
            )
            .show(ctx, |ui| {
                // The transparent macOS titlebar belongs to this strip. Keep
                // the native double-click gesture even though it is our
                // content that is visible there now.
                let titlebar = ui.interact(
                    ui.max_rect(),
                    ui.id().with("titlebar-double-click"),
                    egui::Sense::click(),
                );
                toggle_maximized = titlebar.double_clicked();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    for (which, open) in self.open.iter().enumerate() {
                        let here = which == self.at;
                        // The folder's own name. The whole path is too long
                        // for a tab and the part that differs is the end.
                        let name = std::path::Path::new(&open.path)
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| open.path.clone());

                        let label = ui
                            .selectable_label(
                                here,
                                egui::RichText::new(name)
                                    .font(match here {
                                        true => theme::medium(12.5),
                                        false => theme::body(12.5),
                                    })
                                    .color(match here {
                                        true => theme::text(),
                                        false => theme::muted(),
                                    }),
                            )
                            .on_hover_text(&open.path);
                        if label.clicked() {
                            go = Some(which);
                        }

                        // The cross is on the tab you are on and on whatever
                        // the pointer is over, so the strip is not a row of
                        // crosses asking to be pressed by accident.
                        let shown = here || label.hovered();
                        if shown && self.open.len() > 1 {
                            let cross = ui.add(
                                egui::Button::new(
                                    egui::RichText::new("\u{00d7}")
                                        .font(theme::body(13.0))
                                        .color(theme::faint()),
                                )
                                .frame(false)
                                .min_size(egui::vec2(16.0, 16.0)),
                            );
                            if cross.on_hover_text("Close this tab").clicked() {
                                shut = Some(which);
                            }
                        }
                        ui.add_space(6.0);
                    }

                    self.plus(ui);

                    // The profile sits at the far end of the strip, where it
                    // is out of the way of the tabs and still on every
                    // screen -- including the one with no repository open,
                    // which is exactly where somebody changes client.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.chip(ui);
                        ui.add_space(6.0);
                        self.gear(ui);
                        // A newer version, when there is one, beside the
                        // other two things that are about the program and
                        // not about a repository. A word and not a dialog:
                        // it is there to be noticed, not to be answered.
                        ui.add_space(6.0);
                        newer = self.updates.word(ui, &self.settings.skipped);
                    });
                });
                ui.add_space(4.0);
            });

        if toggle_maximized {
            let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if let Some(act) = newer {
            self.act_on_update(ctx, act);
        }

        if let Some(which) = shut {
            self.close(which);
            return;
        }
        if let Some(which) = go {
            self.at = which;
        }
    }

    /// The button that opens the screen for choosing a repository.
    fn plus(&mut self, ui: &mut egui::Ui) {
        let button = ui.add(
            egui::Button::new(
                egui::RichText::new("+")
                    .font(theme::body(14.0))
                    .color(theme::muted()),
            )
            .frame(false)
            .min_size(egui::vec2(22.0, 20.0)),
        );
        if button.on_hover_text("Open another repository").clicked() {
            self.pick();
        }
    }

    /// The button that opens the preferences, and puts them away again.
    ///
    /// Beside the profile, at the end of the strip, for the reason the
    /// profile is there: it is not about a repository. The gear is drawn --
    /// the fonts this program ships have none -- as a ring with eight teeth,
    /// which is all a gear is at this size.
    fn gear(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 20.0), egui::Sense::click());
        let up = self.preferring.is_some();
        let colour = match (up, response.hovered()) {
            (true, _) => ui.visuals().selection.bg_fill,
            (_, true) => theme::text(),
            _ => theme::muted(),
        };
        let centre = rect.center();
        let painter = ui.painter();
        let stroke = egui::Stroke::new(1.3, colour);
        painter.circle_stroke(centre, 4.6, stroke);
        painter.circle_stroke(centre, 1.7, stroke);
        for tooth in 0..8 {
            let angle = std::f32::consts::TAU * tooth as f32 / 8.0;
            let (sin, cos) = angle.sin_cos();
            painter.line_segment(
                [
                    centre + egui::vec2(cos * 4.9, sin * 4.9),
                    centre + egui::vec2(cos * 7.0, sin * 7.0),
                ],
                egui::Stroke::new(1.9, colour),
            );
        }

        if response.on_hover_text("Preferences").clicked() {
            self.note = None;
            self.preferring = match up {
                true => None,
                false => {
                    self.managing = None;
                    Some(prefs_view::Preferring::open(&self.settings.general))
                }
            };
        }
    }

    /// Who the window is working as, and the menu for being somebody else.
    ///
    /// It is in the strip rather than in a repository's own bar because it is
    /// not about a repository: one profile is in use across every tab at
    /// once, and the moment somebody wants to change it is usually the moment
    /// before they open the next client's repository.
    fn chip(&mut self, ui: &mut egui::Ui) {
        let using = self.profiles.active();
        let name = match using {
            Some(profile) if !profile.name.trim().is_empty() => profile.name.clone(),
            Some(_) => "(no name)".to_string(),
            None => "Profiles".to_string(),
        };
        let hint = match using {
            Some(profile) => {
                let who = profile
                    .who()
                    .map(|(name, email)| format!("{name} <{email}>"))
                    .unwrap_or_else(|| "the name in your Git configuration".to_string());
                let key = match profile.key.trim().is_empty() {
                    true => "the SSH agent".to_string(),
                    false => profile.key.trim().to_string(),
                };
                format!("Working as {who}\nKey: {key}")
            }
            None => "No profile: Git's own settings, the SSH agent, and the forge \
                     as its tool is logged in"
                .to_string(),
        };

        let button = ui.add(
            egui::Button::new(
                egui::RichText::new(format!("{name}     "))
                    .font(match using.is_some() {
                        true => theme::medium(12.0),
                        false => theme::body(12.0),
                    })
                    .color(match using.is_some() {
                        true => theme::text(),
                        false => theme::muted(),
                    }),
            )
            .frame(false),
        );
        theme::caret(ui, &button);
        let button = button.on_hover_text(hint);
        let id = ui.make_persistent_id("menu-profiles");
        if button.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }

        let mut act = None;
        egui::popup::popup_below_widget(
            ui,
            id,
            &button,
            egui::PopupCloseBehavior::CloseOnClick,
            |ui| {
                ui.set_width(PROFILE_MENU);

                if ui
                    .selectable_label(
                        self.profiles.at().is_none(),
                        egui::RichText::new("Git's own settings")
                            .font(theme::body(12.0))
                            .color(theme::text()),
                    )
                    .clicked()
                {
                    act = Some(profiles_view::Act::Use(None));
                }

                for (which, profile) in self.profiles.list.iter().enumerate() {
                    let name = match profile.name.trim().is_empty() {
                        true => "(no name)",
                        false => profile.name.trim(),
                    };
                    if ui
                        .selectable_label(
                            self.profiles.at() == Some(which),
                            egui::RichText::new(name).font(theme::body(12.0)).color(theme::text()),
                        )
                        .clicked()
                    {
                        act = Some(profiles_view::Act::Use(Some(which)));
                    }
                }

                theme::rule(ui);
                // Making one is here and not only on the screen behind
                // "Manage", because the first thing anybody wants from this
                // menu the first time they open it is a profile, and this is
                // where they are already looking.
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("+  Add a profile")
                            .font(theme::body(12.0))
                            .color(theme::text()),
                    ))
                    .clicked()
                {
                    act = Some(profiles_view::Act::Add);
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Manage profiles\u{2026}")
                            .font(theme::body(12.0))
                            .color(theme::muted()),
                    ))
                    .clicked()
                {
                    act = Some(profiles_view::Act::Manage);
                }
            },
        );

        if let Some(act) = act {
            let ctx = ui.ctx().clone();
            self.act_on_profiles(&ctx, act);
        }
    }

    /// What a screen that is not a repository has to say, along the bottom.
    /// Gives back whether the cross on it was pressed.
    fn note_panel(ctx: &egui::Context, note: Option<&Note>) -> bool {
        // Along the bottom, and in a panel of its own, because the screen
        // above it fills whatever room there is: a line added after it is a
        // line below the edge of the window, which is a line nobody reads.
        // A profile that could not be written down is a profile that will
        // not be there tomorrow, and finding that out tomorrow is worse.
        let mut hush = false;
        if let Some(note) = note {
            let (text, colour) = match note {
                Note::Said(text) => (text, theme::muted()),
                Note::Wrong(text) => (text, theme::removed()),
            };
            egui::TopBottomPanel::bottom("profiles-note")
                .frame(
                    egui::Frame::NONE
                        .fill(theme::raised())
                        .inner_margin(egui::Margin::symmetric(18, 7)),
                )
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(text).font(theme::body(11.5)).color(colour));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("\u{00d7}")
                                            .font(theme::body(13.0))
                                            .color(theme::faint()),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                hush = true;
                            }
                        });
                    });
                });
        }
        hush
    }

    /// The preferences screen, drawn over everything instead of a repository.
    fn preferences(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.preferring else {
            return;
        };

        let hush = Self::note_panel(ctx, self.note.as_ref());
        let tokens = self.profiles.tokens();
        let mut changed = false;
        let mut act = None;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::canvas())
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ctx, |ui| {
                let newer = prefs_view::Newer {
                    running: self.updates.running(),
                    looking: self.updates.looking(),
                    answer: self.updates.answer(),
                };
                (changed, act) = prefs_view::show(ui, state, &mut self.settings.general, tokens, newer);
            });

        // Said only when it fails, as the switch for the waves is: a
        // preference that could not be written down is one that will be
        // back as it was tomorrow. And when it fails the screen stays up
        // whatever else was pressed, because the line saying so is on it.
        if changed {
            self.note = self.settings.save().err().map(|err| {
                Note::Wrong(format!("could not write the preferences down: {err:#}"))
            });
            if self.note.is_some() {
                return;
            }
        }

        match act {
            Some(prefs_view::Act::Forget) => {
                let had = self.profiles.forget_tokens();
                self.keep_profiles();
                // `keep_profiles` leaves a note only when the file could not
                // be written, and then that is the thing to say.
                if self.note.is_none() {
                    self.note = Some(Note::Said(match had {
                        1 => "1 token forgotten".to_string(),
                        n => format!("{n} tokens forgotten"),
                    }));
                }
            }
            Some(prefs_view::Act::Look) => {
                // Asking is asking about every version: what was said not
                // to be mentioned is mentioned again.
                self.settings.skipped.clear();
                let _ = self.settings.save();
                self.updates.look_now(ctx, self.settings.general.update_betas);
            }
            Some(prefs_view::Act::Close) => {
                self.preferring = None;
                self.note = None;
            }
            None if hush => self.note = None,
            None => {}
        }
    }

    /// The profiles screen, drawn over everything instead of a repository.
    fn profiles(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.managing else {
            return;
        };

        let mut act = None;
        let hush = Self::note_panel(ctx, self.note.as_ref());

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::canvas())
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ctx, |ui| {
                act = profiles_view::show(ui, state, &mut self.profiles);
            });

        if let Some(act) = act {
            self.act_on_profiles(ctx, act);
        } else if hush {
            self.note = None;
        }
    }

    /// Acts on what the screen, or the menu, asked for.
    fn act_on_profiles(&mut self, ctx: &egui::Context, act: profiles_view::Act) {
        match act {
            profiles_view::Act::Changed => self.keep_profiles(),
            // Choosing a profile with a repository in front is saying whose
            // that repository is, and it is written down as that: the next
            // time it is opened it is opened as this one, whatever was in
            // use a moment before. Choosing nobody takes it back off.
            profiles_view::Act::Use(which) => {
                self.profiles.use_at(which);
                if let Some(place) = self.in_front() {
                    self.profiles.claim(which, &place);
                }
                self.keep_profiles();
            }
            // Adding is asked for from the menu as well as from the screen,
            // and from the menu there is no screen yet. A profile made and
            // left somewhere the person who asked for it cannot see is a
            // profile they will make again, so the screen comes up on it.
            profiles_view::Act::Add => {
                let at = self.profiles.add(Profile::new("New profile"));
                self.open_profiles_at(at);
                self.keep_profiles();
            }
            profiles_view::Act::Duplicate(which) => {
                if let Some(at) = self.profiles.duplicate(which) {
                    self.open_profiles_at(at);
                }
                self.keep_profiles();
            }
            profiles_view::Act::Remove(which) => {
                self.profiles.remove(which);
                // Which one the screen was open on is now a number about a
                // profile that has gone; it picks another for itself.
                if let Some(state) = &mut self.managing {
                    state.at = None;
                    state.settled();
                }
                self.keep_profiles();
            }
            profiles_view::Act::Browse(which) => self.browse(Browsing::Key { which }),
            profiles_view::Act::MakeKey(which) => self.make_key(which),
            profiles_view::Act::CopyKey(which) => self.copy_key(ctx, which),
            profiles_view::Act::Manage => {
                self.managing = Some(profiles_view::Managing::default());
                self.preferring = None;
                self.note = None;
            }
            profiles_view::Act::Close => {
                self.managing = None;
                self.note = None;
            }
        }
    }

    /// Brings the profiles screen up on one profile in particular.
    fn open_profiles_at(&mut self, at: usize) {
        self.managing
            .get_or_insert_with(profiles_view::Managing::default)
            .open(at);
        self.preferring = None;
        self.note = None;
    }

    /// Makes a profile a key of its own and writes the path into it.
    ///
    /// On the thread that paints, which is a thing this program does not do
    /// with work that can be slow. This one is not: an ed25519 pair is a few
    /// milliseconds of arithmetic, and the alternative -- a thread, a
    /// channel and a state for "a key is being made" -- would be more
    /// machinery than the wait it saves.
    fn make_key(&mut self, which: usize) {
        let Some(profile) = self.profiles.list.get(which) else {
            return;
        };
        let Some(at) = profile.key_place() else {
            self.note = Some(Note::Wrong(
                "there is no home folder to put a key in".to_string(),
            ));
            return;
        };
        // The comment is what the forge shows beside the key in its list, so
        // it should say who this is, and the address says that best.
        let comment = match profile.email.trim().is_empty() {
            false => profile.email.trim(),
            true => match profile.name.trim().is_empty() {
                false => profile.name.trim(),
                true => "gitrust",
            },
        };

        match gitrust_core::make_key(&at, comment) {
            Ok(_) => {
                let written = gitrust_core::shorten(&at);
                self.profiles.list[which].key = written.clone();
                self.keep_profiles();
                if self.note.is_none() {
                    self.note = Some(Note::Said(format!(
                        "{written} made. Its public half goes in the client's forge: \
                         Copy the public key, then paste it there"
                    )));
                }
            }
            Err(err) => self.note = Some(Note::Wrong(format!("{err:#}"))),
        }
    }

    /// Puts the public half of a profile's key on the clipboard.
    fn copy_key(&mut self, ctx: &egui::Context, which: usize) {
        let Some(path) = self
            .profiles
            .list
            .get(which)
            .and_then(gitrust_core::Profile::public_key)
        else {
            return;
        };
        self.note = match std::fs::read_to_string(&path) {
            Ok(text) => {
                ctx.copy_text(text.trim().to_string());
                Some(Note::Said(format!(
                    "{} is on the clipboard",
                    gitrust_core::shorten(&path)
                )))
            }
            Err(err) => Some(Note::Wrong(format!("could not read {}: {err}", path.display()))),
        };
    }

    /// Writes the profiles down, and says so if it could not.
    fn keep_profiles(&mut self) {
        self.note = match self.profiles.save() {
            Ok(()) => None,
            Err(err) => Some(Note::Wrong(format!(
                "could not write the profiles down: {err:#}"
            ))),
        };
    }

    /// The repository being looked at, as a profile would file it.
    ///
    /// Nothing while the repository screen is up: what is in front then is a
    /// list to choose from, and a profile picked over it is picked for
    /// whatever is about to be opened, not for the tab left behind it.
    fn in_front(&self) -> Option<String> {
        if self.picking.is_some() {
            return None;
        }
        self.open.get(self.at).and_then(|open| open.place.clone())
    }

    /// Files a repository that has just been opened under the profile in
    /// use, if it is not filed under one already.
    ///
    /// Only on opening, never on merely coming back to a tab: a repository
    /// is opened as somebody on purpose, and it is looked at on the way past
    /// by accident.
    fn file_under_profile(&mut self) {
        let Some(place) = self.open.get(self.at).and_then(|open| open.place.clone()) else {
            return;
        };
        if self.profiles.owner_of(&place).is_some() {
            return;
        }
        if let Some(using) = self.profiles.at() {
            self.profiles.claim(Some(using), &place);
            self.keep_profiles();
        }
    }

    /// Puts the profile of the repository in front in use.
    ///
    /// This is what stops a commit to one client's repository going out
    /// under another client's name: which profile is in use is no longer
    /// whichever was chosen last, it is whose repository this is. One that
    /// is nobody's works as nobody -- Git's own settings -- rather than as
    /// the last profile that happened to be on screen.
    fn follow_front(&mut self) {
        let Some(place) = self.in_front() else {
            return;
        };
        let owner = self.profiles.owner_of(&place);
        if self.profiles.at() != owner {
            self.profiles.use_at(owner);
            self.keep_profiles();
        }
    }

    /// Hands every tab its profile: the one its repository is filed under.
    ///
    /// Compared before it is handed over, because handing it over is a
    /// handful of copied strings on every frame -- and because the
    /// comparison is also what tells a tab that its person has changed,
    /// which is the moment what the forge said stops being true.
    fn adopt_profile(&mut self) {
        for open in &mut self.open {
            let mine = match &open.place {
                Some(place) => self
                    .profiles
                    .owner_of(place)
                    .and_then(|at| self.profiles.list.get(at)),
                // A tab onto nothing in particular has nothing to be filed
                // under, and works as whoever is in use.
                None => self.profiles.active(),
            };
            if same_person(open.profile.as_ref(), mine) {
                continue;
            }
            // Without the list of repositories: a tab has no use for it, and
            // carrying it would make every tab of a profile think its person
            // had changed each time another repository was filed under them.
            open.adopt(mine.map(|profile| Profile {
                repos: Vec::new(),
                ..profile.clone()
            }));
        }
    }

    /// Brings a repository down, off to one side.
    ///
    /// The only thing in this window that can take minutes, and the only one
    /// that goes near a network without a repository behind it. The tab is
    /// opened when the clone is there, not when it is asked for: a tab onto a
    /// folder that is still filling up has nothing to show and no way to say
    /// why.
    fn start_clone(&mut self, url: String, into: std::path::PathBuf) {
        let (send, done) = std::sync::mpsc::channel();
        let where_to = into.clone();
        // The profile goes with it: a client's private repository is exactly
        // the kind that cannot be cloned without their key.
        let profile = self.profiles.active().cloned();
        std::thread::spawn(move || {
            let _ = send.send(gitrust_core::start::clone(&url, &where_to, profile));
        });
        self.cloning = Some(Cloning { into, done });
    }

    /// Looks at whether the clone has finished.
    fn poll_clone(&mut self, ctx: &egui::Context) {
        let Some(cloning) = &self.cloning else {
            return;
        };
        match cloning.done.try_recv() {
            Ok(result) => {
                let into = cloning.into.clone();
                self.cloning = None;
                match result {
                    Ok(_) => {
                        self.add(into.display().to_string());
                        if let Some(open) = self.open.get_mut(self.at) {
                            open.after_clone();
                        }
                    }
                    Err(err) => self.complain(err),
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Without this egui goes to sleep as soon as nobody touches
                // the mouse, and a clone that finished would sit there unseen
                // until something else woke the window.
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.cloning = None,
        }
    }

    /// The screen, drawn over everything instead of a repository.
    fn picker(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.picking else {
            return;
        };

        let open_now: Vec<std::path::PathBuf> = self
            .open
            .iter()
            .map(|open| std::path::PathBuf::from(&open.path))
            .collect();
        let known = std::mem::take(&mut self.known);
        let glance = std::mem::take(&mut self.glance);

        let mut act = None;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::canvas())
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ctx, |ui| {
                act = picker::show(
                    ui,
                    state,
                    &known,
                    &glance,
                    &open_now,
                    &self.settings.remotes,
                    &self.profiles,
                );

                if let Some(cloning) = &self.cloning {
                    ui.add_space(4.0);
                    theme::empty(
                        ui,
                        &format!("Cloning into {}\u{2026}", cloning.into.display()),
                    );
                }
            });

        self.known = known;
        self.glance = glance;

        if let Some(act) = act {
            self.act_on_pick(act);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::sync(ctx, self.settings.appearance);
        // Every tab is polled, not only the one on top. An operation left
        // running in another one has to finish whether or not you are looking
        // at it; otherwise coming back to it would find it still saying
        // "pushing…" with the push long since done.
        self.follow_front();
        self.adopt_profile();
        self.hand_preferences();
        self.auto_fetch(ctx);
        let mut ended = false;
        for (which, open) in self.open.iter_mut().enumerate() {
            open.poll(ctx);
            open.poll_forge(ctx);
            open.poll_board(ctx);
            open.poll_fetch(ctx);
            open.poll_foresee(ctx);
            ended |= open.poll_terminal(ctx) && which == self.at;
        }
        // A shell that has exited takes its panel with it: an open panel
        // with nothing in it would only start another, and whoever typed
        // `exit` did not ask for one. Only for the tab in front -- the panel
        // is the window's, and another tab's shell ending is not a reason
        // to take it from this one.
        if ended && self.settings.terminal == Panel::Expanded {
            self.settings.terminal = Panel::Hidden;
            let _ = self.settings.save();
        }
        self.note_tabs();
        self.keep_up(ctx);
        self.updates.tick(ctx, self.settings.general.updates, self.settings.general.update_betas);

        // Always, even with one repository open: the plus lives in it, and a
        // strip that appears only once you have two tabs is a way of opening
        // a second tab that you can only reach by having one.
        self.strip(ctx);
        self.poll_clone(ctx);
        self.poll_browse(ctx);
        self.poll_dropped(ctx);

        // One of the four, never two: a screen here is not a dialog over a
        // repository, it is what is there instead of one.
        match (self.preferring.is_some(), self.managing.is_some(), self.picking.is_some()) {
            (true, _, _) => self.preferences(ctx),
            (_, true, _) => self.profiles(ctx),
            (_, _, true) => self.picker(ctx),
            _ => {
                let mut elsewhere = None;
                if let Some(open) = self.open.get_mut(self.at) {
                    open.draw(ctx, &mut self.settings);
                    elsewhere = open.elsewhere.take();
                }
                if let Some(path) = elsewhere {
                    self.open_tab(path);
                }
            }
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum View {
    History,
    Changes,
    Board,
}

struct Open {
    /// What keeps this tab's egui state apart from the other tabs'. Scroll
    /// positions, folded sections and open popups are remembered by widget
    /// id, and without something to tell two tabs apart they would all
    /// remember one window between them.
    ///
    /// It is handed out on creation and never reused, rather than being the
    /// tab's position: closing a tab would otherwise shift every tab after it
    /// onto somebody else's remembered state.
    salt: u64,
    path: String,
    /// A remote tab owns the authenticated session for its whole lifetime.
    remote: Option<RemoteRepo>,
    /// Retained without a password so a dropped SSH session can be reopened
    /// deliberately through the SSH agent.
    remote_descriptor: Option<gitrust_core::ssh::RemoteDescriptor>,
    remote_disconnected: Option<String>,
    /// A folder this tab has asked for a tab of its own.
    ///
    /// A tab cannot make a tab: the list of them belongs to the window, and
    /// this is the whole of what one tab can say about it. Taken by the
    /// window on the frame it is set and never read twice.
    elsewhere: Option<std::path::PathBuf>,
    head: String,
    /// What is in the repository: it stays put in the bottom bar.
    summary: String,
    /// The last thing that happened. It is cleared on a reload.
    status: String,
    status_failed: bool,
    view: View,

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
    /// Who this tab is working as: the profile its repository is filed
    /// under. Copied in rather than borrowed, because every repository this
    /// tab opens is opened on a thread that outlives the frame that started
    /// it.
    profile: Option<Profile>,
    /// Where this tab's repository is, as the line a profile files it under:
    /// a folder, or `ssh://...` for a checkout on another machine. Worked
    /// out once, here, because the frame asks for it sixty times a second
    /// and the answer is a walk through every link in a path. `None` is a
    /// tab onto something that is not a repository, which is nobody's.
    place: Option<String>,

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

    /// The file being looked at across the window, when one is. It takes the
    /// middle of the window from the graph while it is up.
    looking: Option<file_view::Looking>,
    /// What is half-done here, read on every load. It is the one thing that
    /// changes what the whole window is for, so it is not asked for: it is
    /// known.
    half: gitrust_core::Half,
    clashes: Vec<gitrust_core::Clash>,
    /// The conflict screen, while it is up.
    deciding: Option<conflict_view::Deciding>,
    /// The plan for rewriting a run of commits, while one is being arranged.
    /// Nothing in it has happened: it is a list of intentions until the
    /// button at the top of that screen is pressed.
    rewriting: Option<gitrust_core::Plan>,

    // The working tree.
    work: WorkingState,
    /// The repository's pulse as of what is on screen: read in the same
    /// walk as `work`, every time `work` is. What a look from outside is
    /// compared with, to know whether it found anything this tab has not
    /// already shown.
    pulse: Option<Pulse>,
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
    /// The two branches one was dropped on the other, waiting to be told
    /// which of the three things that meant.
    joining: Option<Joining>,
    /// The last thing that happened, said where it will be seen.
    toast: Option<toast::Toast>,

    /// The preferences, as last handed over by the window.
    general: General,
    /// What has happened in this tab, oldest first.
    log: Vec<Logged>,
    /// A fetch nobody asked for, while one is out.
    fetching: Option<std::sync::mpsc::Receiver<Result<bool>>>,
    /// Held by whichever thread is working on this repository, so that the
    /// fetch nobody asked for and the pull somebody did are never both
    /// writing the remote's branches.
    turn: std::sync::Arc<std::sync::Mutex<()>>,
    /// What merging into the target would run into, when that is known.
    foreseen: Option<Foreseen>,
    /// Where the branch and its target were when that was last asked, so it
    /// is asked again only when one of them has moved.
    foresaw: Option<(Oid, Oid)>,
    /// The asking, while it is out.
    foreseeing: Option<std::sync::mpsc::Receiver<Result<Option<Foreseen>>>>,
    /// Where HEAD was at the last reload, and whether there has been one:
    /// a reload that finds it somewhere else is how the window knows a Git
    /// action has happened, whichever one it was.
    head_at: Option<Oid>,
    looked: bool,
    /// The commit the submodules were last brought up to date at, so that a
    /// submodule that will not update is tried once and not on every reload.
    subs_tried: Option<Oid>,
    /// The shell in this tab's terminal, once its panel has been opened.
    /// It outlives the panel being put away, and goes when it is killed,
    /// when it exits, or when the tab does.
    terminal: Option<terminal::Session>,
}

/// One line of the activity log.
struct Logged {
    at: chrono::DateTime<chrono::Local>,
    text: String,
    failed: bool,
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

/// One branch dropped on another, with the question not yet put.
///
/// The gesture says which two branches and which way round. It does not say
/// whether that meant a merge, a rebase or a move up, and guessing would be
/// guessing at the one part that cannot be taken back.
struct Joining {
    /// The branch dragged: where the commits come from.
    from: String,
    /// The branch dropped on: the one that ends up somewhere else.
    onto: String,
    /// Whether `onto` could simply move up. Worked out once, when the gesture
    /// lands, rather than every frame the menu is on screen.
    room: bool,
    /// Where it was dropped, which is where the menu goes. A menu that opens
    /// in the middle of the window makes you look away from the two branches
    /// you were just aiming at.
    at: egui::Pos2,
    /// Whether it has been on screen for a frame. Until it has, a click
    /// cannot be meant for anything outside it -- and the release that
    /// dropped the branch is still in the same frame.
    armed: bool,
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
    DiscardHunk {
        path: String,
        hunk: HunkId,
    },
    DiscardAll(Vec<String>),
    DeleteBranch {
        name: String,
        remote: bool,
    },
    CloseWorktree {
        name: String,
    },
    /// Throw away whatever is half-done.
    Abandon,
    MoveTo {
        oid: Oid,
        how: Move,
    },
    Land {
        base: String,
        branch: String,
    },
    Review(String),
}

impl Open {
    fn new(path: String, salt: u64, general: &General) -> Self {
        Self::with_backend(path, salt, None, None, general)
    }

    fn new_remote(mut remote: RemoteRepo, salt: u64, general: &General) -> Self {
        let path = remote.location();
        remote.pruning(general.auto_prune);
        Self::with_backend(path, salt, Some(remote), None, general)
    }

    /// A tab onto a checkout on another machine that is not connected yet.
    ///
    /// What a remembered tab comes back as. Connecting needs the agent or a
    /// key and can take as long as the network lets it, so it waits for
    /// somebody to press Reconnect rather than being done for them, one tab
    /// after another, before the window has drawn.
    fn waiting(descriptor: gitrust_core::ssh::RemoteDescriptor, salt: u64, general: &General) -> Self {
        let path = format!(
            "{}@{}:{}",
            descriptor.login.user, descriptor.login.host, descriptor.path
        );
        let mut open = Self::with_backend(path, salt, None, Some(descriptor), general);
        open.status = "not connected: Reconnect opens it again".to_string();
        open
    }

    fn with_backend(
        path: String,
        salt: u64,
        remote: Option<RemoteRepo>,
        waiting: Option<gitrust_core::ssh::RemoteDescriptor>,
        general: &General,
    ) -> Self {
        let remote_descriptor = remote.as_ref().map(RemoteRepo::descriptor).or(waiting);
        let place = match &remote_descriptor {
            Some(remote) => Some(remote.place()),
            None => Repo::open(&path)
                .ok()
                .and_then(|_| gitrust_core::place_of(std::path::Path::new(&path))),
        };
        let mut app = Self {
            salt,
            path,
            place,
            remote,
            remote_descriptor,
            remote_disconnected: None,
            elsewhere: None,
            head: String::new(),
            summary: String::new(),
            status: String::new(),
            status_failed: false,
            view: View::History,
            graph: Graph::default(),
            limit: general.commits(),
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
            profile: None,
            tracking: None,
            stashes: Vec::new(),
            draft: action_bar::Draft::default(),
            job: None,
            looking: None,
            half: gitrust_core::Half::None,
            clashes: Vec::new(),
            deciding: None,
            rewriting: None,
            selected_commit: None,
            detail: None,
            detail_file: None,
            detail_diff: None,
            work: WorkingState::default(),
            pulse: None,
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
            joining: None,
            toast: None,
            general: general.clone(),
            log: Vec::new(),
            fetching: None,
            turn: std::sync::Arc::default(),
            foreseen: None,
            foresaw: None,
            foreseeing: None,
            head_at: None,
            looked: false,
            subs_tried: None,
            terminal: None,
        };
        app.load();
        if let Ok(shot) = std::env::var("GITRUST_SHOT") {
            app.pose(&shot);
        }
        app
    }

    /// Whether this tab is onto a checkout on another machine -- connected
    /// or not. A remembered one comes back with the address and no
    /// connection, and it is still not a folder on this disk.
    fn is_remote(&self) -> bool {
        self.remote_descriptor.is_some()
    }

    /// Opens the window straight onto one screen, for photographing it.
    ///
    /// `GITRUST_SHOT=board`, `changes:<path>`, `diff:<path>`, `whole:<path>`,
    /// `blame:<path>` or `clash`.
    ///
    /// It is here because the pictures in the README are the first thing
    /// anybody sees of a program with a window, and they go stale every time
    /// the window changes. Taking them again means getting to each screen,
    /// and the machines that take them have no way to move a pointer: a
    /// screen you can only reach by clicking is a screen that cannot be
    /// photographed. So each one can be opened by name instead.
    ///
    /// It puts the window where a click would have put it and does nothing
    /// else -- no state that clicking could not reach, and nothing written.
    fn pose(&mut self, shot: &str) {
        let (what, rest) = shot.split_once(':').unwrap_or((shot, ""));
        match what {
            "board" => self.view = View::Board,
            "changes" => {
                self.view = View::Changes;
                if !rest.is_empty() {
                    self.handle(changes_view::Action::Select {
                        path: rest.to_string(),
                        staged: false,
                    });
                }
            }
            "diff" | "whole" | "blame" => {
                self.look_at(rest.to_string());
                if let Some(looking) = self.looking.as_mut() {
                    looking.how = match what {
                        "whole" => file_view::Look::Whole,
                        "blame" => file_view::Look::Blame,
                        _ => file_view::Look::Diff,
                    };
                }
                self.read_file();
            }
            "clash" => self.decide(),
            _ => {}
        }
    }

    fn repo(&self) -> Result<Repo> {
        Ok(Repo::open(&self.path)?
            .using(self.profile.clone())
            .pruning(self.general.auto_prune))
    }

    /// Takes on a profile: the one its repository is filed under.
    ///
    /// What the forge last said is thrown away with it: it answered as
    /// somebody else, and a column that says "in review" because another
    /// account has a pull request open is a column that is lying.
    fn adopt(&mut self, profile: Option<Profile>) {
        if self.profile == profile {
            return;
        }
        self.profile = profile;
        self.name_the_remote();
        self.pulls = None;
        self.forge_note = None;
    }

    /// Tells a checkout on another machine whose name to write.
    ///
    /// Asked for apart from `adopt` because a reconnection makes a new
    /// connection that has been told nothing, under a profile that has not
    /// changed -- which is exactly the case `adopt` returns early on.
    fn name_the_remote(&mut self) {
        let who = self.profile.as_ref().and_then(Profile::who);
        if let Some(remote) = &mut self.remote {
            remote.working_as(who);
        }
    }

    /// Reloads everything: history, references and the working tree's state.
    fn load(&mut self) {
        if self.is_remote() {
            self.load_remote();
            return;
        }
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
        // Before anything else that reads the tree: what is half-done here
        // changes what every other reading means.
        self.half = repo.half_done();
        self.clashes = repo.clashes().unwrap_or_default();
        if self.half == gitrust_core::Half::None {
            self.deciding = None;
        }
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
        self.record(self.status.clone(), false, true);
        self.reload_detail(&repo);
        self.look_ahead(&repo);
        // Last, because it may send something off that says so in the bar,
        // and the line about how long the reading took would write over it.
        self.mind_submodules(&repo);
    }

    /// Reads again what a fetch can have changed, and nothing else.
    ///
    /// For the fetch nobody asked for. A whole reload would also put away a
    /// commit message being amended and move what is selected, and somebody
    /// in the middle of writing one did not ask for anything to happen.
    fn reload_refs(&mut self) {
        let Ok(repo) = self.repo() else {
            return;
        };
        self.refs = repo.references().unwrap_or_default();
        self.tracking = repo.tracking();
        self.backlog = repo.backlog().unwrap_or_default();
        self.lay_out();
        if self.rebuild_graph(&repo).is_ok() {
            // Counting again starts the search from its first match, which
            // is right for a reload somebody asked for and wrong for this:
            // they were on the seventh.
            let at = self.at;
            self.recount();
            self.at = at.min(self.found.len().saturating_sub(1));
        }
        self.look_ahead(&repo);
    }

    /// Takes on the preferences, and acts on the ones that change what is
    /// already on screen.
    fn prefer(&mut self, general: &General) {
        if self.general == *general {
            return;
        }
        let before = std::mem::replace(&mut self.general, general.clone());
        if let Some(remote) = &mut self.remote {
            remote.pruning(general.auto_prune);
        }

        if before.commits() != general.commits() {
            // How much history to read has changed, and what is on screen
            // was read under the old answer.
            self.limit = general.commits();
            if self.job.is_none() {
                self.load();
            }
        } else if before.foresee_conflicts != general.foresee_conflicts {
            self.foresaw = None;
            self.foreseen = None;
            match self.is_remote() {
                true => self.look_ahead_remote(),
                false => {
                    if let Ok(repo) = self.repo() {
                        self.look_ahead(&repo);
                    }
                }
            }
        }
    }

    /// Reads further back, once somebody has reached the end of what was.
    ///
    /// Only when the end they reached is where the reading stopped and not
    /// where the history does: a repository with fewer commits than were
    /// asked for has shown all it has.
    fn more(&mut self) {
        if !self.general.lazy_load || self.general.show_all || self.job.is_some() {
            return;
        }
        let read = self.graph.rows.iter().filter(|row| !row.commit.is_working()).count();
        if read < self.limit {
            return;
        }
        self.limit = self
            .limit
            .saturating_add(self.general.initial_commits.max(gitrust_core::FEWEST_COMMITS));

        if self.is_remote() {
            self.load();
        } else if let Ok(repo) = self.repo() {
            if self.rebuild_graph(&repo).is_ok() {
                self.recount();
            }
        }
        let now = self.graph.rows.iter().filter(|row| !row.commit.is_working()).count();
        self.record(format!("read further back: {now} commits now"), false, true);
    }

    /// Writes a line in the activity log.
    ///
    /// `unasked` marks what the window did by itself -- a reload, a fetch on
    /// the timer -- which is kept only when the preferences ask for the
    /// extended log. Without it the log is what somebody did and how it
    /// went, which is the question it is usually opened to answer.
    fn record(&mut self, text: impl Into<String>, failed: bool, unasked: bool) {
        if unasked && !self.general.extended_log {
            return;
        }
        if self.log.len() >= LOG_LINES {
            self.log.remove(0);
        }
        self.log.push(Logged {
            at: chrono::Local::now(),
            text: text.into(),
            failed,
        });
    }

    /// Fetches every remote off to one side, saying nothing.
    ///
    /// Not while something else is running: a fetch under a push is two
    /// things writing the same references. And not a checkout on another
    /// machine, whose commands run on the thread that paints -- a window
    /// that stops for a moment every minute, by itself, is a broken window.
    fn fetch_quietly(&mut self) {
        if self.is_remote() || self.place.is_none() || self.job.is_some() || self.fetching.is_some() {
            return;
        }
        let (send, done) = std::sync::mpsc::channel();
        let path = self.path.clone();
        let profile = self.profile.clone();
        let prune = self.general.auto_prune;
        let turn = self.turn.clone();

        std::thread::spawn(move || {
            // Something asked for got there first, between the look above
            // and here. It goes ahead and this one waits for the next time.
            let Ok(_held) = turn.try_lock() else {
                let _ = send.send(Ok(false));
                return;
            };
            let _ = send.send(
                Repo::open(&path)
                    .map(|repo| repo.using(profile).pruning(prune))
                    .and_then(|repo| repo.fetch_quietly()),
            );
        });

        self.fetching = Some(done);
        self.record("auto-fetch\u{2026}", false, true);
    }

    /// Looks at whether that fetch is back.
    ///
    /// One that failed is a line in the extended log and nothing else: no
    /// toast and no red in the bar. Nobody asked for it, it will be tried
    /// again in a minute, and a laptop with no network would otherwise be
    /// told so sixty times an hour.
    fn poll_fetch(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;

        let Some(done) = &self.fetching else {
            return;
        };
        match done.try_recv() {
            Ok(result) => {
                self.fetching = None;
                match result {
                    Ok(true) => {
                        self.record("auto-fetch brought something new", false, true);
                        self.reload_refs();
                    }
                    Ok(false) => self.record("auto-fetch: nothing new", false, true),
                    Err(err) => self.record(format!("auto-fetch failed: {err:#}"), true, true),
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(TryRecvError::Disconnected) => self.fetching = None,
        }
    }

    /// Asks, off to one side, whether this branch would go into its target
    /// cleanly -- when either of them has moved since it was last asked.
    fn look_ahead(&mut self, repo: &Repo) {
        if !self.general.foresee_conflicts || self.half != gitrust_core::Half::None {
            self.foreseen = None;
            self.foresaw = None;
            self.foreseeing = None;
            return;
        }
        let key = repo.head_id().zip(repo.target().map(|(_, at)| at));
        if key == self.foresaw {
            return;
        }
        self.foresaw = key;
        self.foreseen = None;
        self.foreseeing = None;
        if key.is_none() {
            return;
        }

        let (send, done) = std::sync::mpsc::channel();
        let path = self.path.clone();
        // A merge of two large trees is not instant, and this is asked on a
        // reload: it goes where it will not hold the reload up.
        std::thread::spawn(move || {
            let _ = send.send(Repo::open(&path).and_then(|repo| repo.foresee()));
        });
        self.foreseeing = Some(done);
    }

    /// The same question for a checkout on another machine, asked of the
    /// Git over there and waited for.
    fn look_ahead_remote(&mut self) {
        if !self.general.foresee_conflicts || self.half != gitrust_core::Half::None {
            self.foreseen = None;
            self.foresaw = None;
            return;
        }
        let head = self.refs.iter().find(|entry| entry.is_head).map(|entry| entry.target);
        let target = likely_target(&self.refs, self.tracking.as_ref().map(|t| t.remote.as_str()))
            .map(|entry| (entry.name.clone(), entry.target));
        let key = head.zip(target.as_ref().map(|(_, at)| *at));
        if key == self.foresaw {
            return;
        }
        self.foresaw = key;
        self.foreseen = None;

        let (Some((ours, theirs)), Some((name, _)), Some(remote)) = (key, target, self.remote.as_ref())
        else {
            return;
        };
        if ours == theirs {
            return;
        }
        match remote.foresee(&name) {
            Ok(seen) => {
                if let Some(text) = seen.as_ref().and_then(Foreseen::says) {
                    self.record(text, false, true);
                }
                self.foreseen = seen;
            }
            // An older Git on the server cannot answer. That is nothing
            // somebody at this end can do anything about.
            Err(err) => self.record(format!("could not look ahead for conflicts: {err:#}"), true, true),
        }
    }

    /// Looks at whether the answer is back.
    fn poll_foresee(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;

        let Some(done) = &self.foreseeing else {
            return;
        };
        match done.try_recv() {
            Ok(result) => {
                self.foreseeing = None;
                match result {
                    Ok(seen) => {
                        if let Some(text) = seen.as_ref().and_then(Foreseen::says) {
                            self.record(text, false, true);
                        }
                        self.foreseen = seen;
                    }
                    Err(err) => {
                        self.record(format!("could not look ahead for conflicts: {err:#}"), true, true)
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
            Err(TryRecvError::Disconnected) => self.foreseeing = None,
        }
    }

    /// Notes where HEAD is, and says whether it has moved since last time.
    ///
    /// The first look is not a move: opening a repository is not a Git
    /// action, and nothing in it should be put somewhere else for having
    /// been opened. Neither is a commit written from this window, which
    /// says where HEAD is itself before the reload looks: somebody trying a
    /// newer library in a submodule and committing something else has not
    /// asked for the library to be put back.
    fn head_moved(&mut self, head: Option<Oid>) -> bool {
        let moved = self.looked && head != self.head_at;
        self.looked = true;
        self.head_at = head;
        if moved {
            self.subs_tried = None;
        }
        moved
    }

    /// Brings the submodules up to date after a Git action, when the
    /// preferences say to.
    ///
    /// "After a Git action" is "HEAD is not where it was": a pull, a switch,
    /// a merge, a reset. Merely opening a repository is not one, and nothing
    /// is fetched for having been looked at: where a submodule comes from
    /// is whatever a file in the repository says, and a folder somebody was
    /// sent is not a reason to go there.
    fn mind_submodules(&mut self, repo: &Repo) {
        if self.head_moved(repo.head_id()) {
            self.bring_submodules(repo);
        }
    }

    /// Moves every submodule that is not at its recorded commit to it, and
    /// brings down the ones that were never brought down.
    fn bring_submodules(&mut self, repo: &Repo) {
        let head = repo.head_id();
        if !self.general.submodules
            || self.job.is_some()
            || head.is_none()
            || self.subs_tried == head
            || self.half != gitrust_core::Half::None
        {
            return;
        }
        let stale: Vec<String> = repo
            .submodules()
            .into_iter()
            .filter(|sub| sub.behind())
            .map(|sub| sub.path)
            .collect();
        if stale.is_empty() {
            return;
        }
        self.subs_tried = head;
        self.spawn("updating submodules", move |repo| repo.update_submodules(&stale));
    }

    /// Brings down the submodules of a repository this window has just
    /// cloned: the clone was the Git action, and it was asked for.
    fn after_clone(&mut self) {
        if let Ok(repo) = self.repo() {
            self.bring_submodules(&repo);
        }
    }

    /// The same for a checkout on another machine, with that machine's Git.
    ///
    /// Only after a move, never on sight: it is a command on the server that
    /// may go out to the network from there, on the thread that paints.
    fn mind_remote_submodules(&mut self) {
        let head = self.refs.iter().find(|entry| entry.is_head).map(|entry| entry.target);
        let moved = self.head_moved(head);
        if !moved
            || !self.general.submodules
            || head.is_none()
            || self.subs_tried == head
            || self.half != gitrust_core::Half::None
        {
            return;
        }
        self.subs_tried = head;
        let Some(remote) = self.remote.as_ref() else {
            return;
        };
        if !remote.has_submodules() {
            return;
        }
        match remote.update_submodules() {
            Ok(note) => {
                self.record(note, false, true);
                // What was modified a moment ago no longer is.
                self.load_remote();
            }
            Err(err) => self.note_error(format!("could not update the submodules: {err:#}")),
        }
    }

    /// Takes away the `.orig` copy beside a file whose conflict has just
    /// been settled, when the preferences say to.
    fn drop_orig(&mut self, path: &str) {
        if !self.general.delete_orig || self.clashes.iter().any(|clash| clash.path == path) {
            return;
        }
        let gone = match &self.remote {
            Some(remote) => remote.drop_orig(path),
            None => self.repo().and_then(|repo| repo.drop_orig(path)),
        };
        match gone {
            Ok(true) => {
                self.record(format!("deleted {path}.orig"), false, true);
                // It was in the list of untracked files a moment ago.
                match self.is_remote() {
                    true => self.load(),
                    false => self.refresh(),
                }
            }
            Ok(false) => {}
            Err(err) => self.record(format!("could not delete {path}.orig: {err:#}"), true, false),
        }
    }

    fn load_remote(&mut self) {
        let started = std::time::Instant::now();
        let Some(repo) = self.remote.as_ref() else {
            return;
        };
        self.head = repo.head_name();
        self.path = repo.location();
        self.half = repo.half_done();
        self.clashes = match repo.clashes() {
            Ok(value) => value,
            Err(err) => return self.remote_fail(err),
        };
        if self.half == gitrust_core::Half::None {
            self.deciding = None;
        }
        self.tracking = repo.tracking();
        self.worktrees.clear();
        self.stashes = repo.stashes().unwrap_or_default();
        self.refs = match repo.references() {
            Ok(value) => value,
            Err(err) => return self.remote_fail(err),
        };
        self.work = match repo.status() {
            Ok(value) => value,
            Err(err) => return self.remote_fail(err),
        };
        if let Some((path, staged)) = self.selected.clone() {
            self.diff = repo.file_diff(&path, staged).ok();
        }
        let commits = match repo.commits(self.limit) {
            Ok(value) => value,
            Err(err) => return self.remote_fail(err),
        };
        let total = commits.len();
        let head = self
            .refs
            .iter()
            .find(|reference| reference.is_head)
            .map(|reference| reference.target);
        self.graph = match head.filter(|_| !self.work.is_clean()) {
            Some(head) => layout_with_working(commits, head),
            None => layout(commits),
        };
        let refs = self.refs.len();
        self.summary = format!(
            "{total} {} across {refs} {} · remote checkout",
            if total == 1 { "commit" } else { "commits" },
            if refs == 1 { "ref" } else { "refs" }
        );
        self.preselect();
        self.status = format!("read remotely in {} ms", started.elapsed().as_millis());
        self.status_failed = false;
        self.record(self.status.clone(), false, true);
        self.remote_disconnected = None;
        self.reload_remote_detail();
        self.look_ahead_remote();
        self.mind_remote_submodules();
    }

    fn reload_remote_detail(&mut self) {
        let Some(oid) = self.selected_commit.filter(|oid| !oid.is_zero()) else {
            self.detail = None;
            self.detail_diff = None;
            return;
        };
        let Some(repo) = self.remote.as_ref() else {
            return;
        };
        self.detail = repo.commit_detail(oid).ok();
        if self.detail_file.is_none() {
            self.detail_file = self
                .detail
                .as_ref()
                .and_then(|detail| detail.files.first())
                .map(|file| file.path.clone());
        }
        self.detail_diff = self
            .detail_file
            .as_deref()
            .and_then(|path| repo.commit_file_diff(oid, path).ok());
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
        self.graph
            .rows
            .first()
            .is_some_and(|row| row.commit.is_working())
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
        match repo.status_and_pulse() {
            Ok((work, pulse)) => {
                self.work = work;
                self.pulse = Some(pulse);
            }
            Err(err) => return self.note_error(format!("could not read the status: {err}")),
        }

        // If the picked file is no longer on its side, we let the pick go.
        if let Some((path, staged)) = self.selected.clone() {
            let list = if staged {
                &self.work.staged
            } else {
                &self.work.unstaged
            };
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

        if self.is_remote() {
            self.reload_remote_detail();
            return;
        }
        match self.repo() {
            Ok(repo) => self.reload_detail(&repo),
            Err(err) => self.fail(err),
        }
    }

    fn select_detail_file(&mut self, path: String) {
        let Some(oid) = self.selected_commit else {
            return;
        };
        self.detail_diff = match self.remote.as_ref() {
            Some(repo) => repo.commit_file_diff(oid, &path).ok(),
            None => self
                .repo()
                .and_then(|repo| repo.commit_file_diff(oid, &path))
                .ok(),
        };
        self.detail_file = Some(path);
    }

    /// Opens a file across the window.
    fn look_at(&mut self, path: String) {
        let Some(at) = self.selected_commit else {
            return;
        };
        self.looking = Some(file_view::Looking {
            path,
            at,
            how: file_view::Look::Diff,
            read: file_view::Read::default(),
            trouble: None,
            editing: false,
            draft: String::new(),
        });
        self.read_file();
    }

    /// Reads whatever is needed and not already read.
    ///
    /// Each of the three questions is read the first time it is asked for and
    /// then kept, so going back and forth between them costs nothing. The
    /// history of the file is different: it is the column on the left, so it
    /// is always needed, and it belongs to the file rather than to the commit
    /// picked -- which is why moving between commits does not throw it away.
    fn read_file(&mut self) {
        let Some(looking) = &self.looking else {
            return;
        };
        let (path, at, how) = (looking.path.clone(), looking.at, looking.how);

        let needed = match how {
            file_view::Look::Diff => looking.read.diff.is_none(),
            file_view::Look::Whole => looking.read.whole.is_none(),
            file_view::Look::Blame => looking.read.blame.is_none(),
        };
        let walking = looking.read.story.is_none();
        if !needed && !walking {
            return;
        }

        if let Some(remote) = self.remote.as_ref() {
            if how == file_view::Look::Whole {
                let read = remote.read_working_file(&path).map(|bytes| match String::from_utf8(bytes) { Ok(text) => gitrust_core::Contents::Text(text), Err(err) => gitrust_core::Contents::Binary(err.into_bytes().len()) });
                if let Some(looking) = &mut self.looking { match read { Ok(whole) => looking.read.whole = Some(whole), Err(err) => looking.trouble = Some(format!("{err:#}")) } }
            }
            return;
        }
        let Ok(repo) = self.repo() else {
            return;
        };

        // On this thread. Reading a file out of a commit is instant, blame is
        // libgit2's own work, and the history of one file is the only slow
        // one -- bounded by the limit it is given. If that limit ever has to
        // go up, this is the line that has to move off here first.
        let mut trouble = None;

        if walking {
            let read = repo.file_history(&path, FILE_HISTORY);
            if let Some(looking) = &mut self.looking {
                match read {
                    Ok(story) => looking.read.story = Some(story),
                    Err(err) => trouble = Some(format!("{err:#}")),
                }
            }
        }

        if !needed {
            if let Some(looking) = &mut self.looking {
                looking.trouble = trouble;
            }
            return;
        }

        match how {
            file_view::Look::Diff => {
                let read = repo.commit_file_diff(at, &path);
                if let Some(looking) = &mut self.looking {
                    match read {
                        Ok(diff) => looking.read.diff = Some(diff),
                        Err(err) => trouble = Some(format!("{err:#}")),
                    }
                }
            }
            file_view::Look::Whole => {
                let read = repo.file_at(at, &path);
                if let Some(looking) = &mut self.looking {
                    match read {
                        Ok(whole) => {
                            if let gitrust_core::Contents::Text(text) = &whole {
                                looking.read.painted = Some(syntax::read(text, &path));
                            }
                            looking.read.whole = Some(whole);
                        }
                        Err(err) => trouble = Some(format!("{err:#}")),
                    }
                }
            }
            file_view::Look::Blame => {
                let read = repo.blame(&path, Some(at));
                if let Some(looking) = &mut self.looking {
                    match read {
                        Ok(lines) => {
                            // The same lines at the same commit as the file
                            // itself, so whichever of the two is asked for
                            // first is the one that pays for the colour.
                            if looking.read.painted.is_none() {
                                let text: Vec<&str> =
                                    lines.iter().map(|line| line.text.as_str()).collect();
                                looking.read.painted = Some(syntax::read_lines(&text, &path));
                            }
                            looking.read.blame = Some(lines);
                        }
                        Err(err) => trouble = Some(format!("{err:#}")),
                    }
                }
            }
        }

        if let Some(looking) = &mut self.looking {
            looking.trouble = trouble;
        }
    }

    /// Opens the screen for arranging what becomes of a run of commits.
    ///
    /// Reading the range can refuse -- a merge in the way, a commit that is
    /// not on this branch -- and that refusal is the answer, so it goes where
    /// every other refusal goes rather than opening an empty screen.
    fn start_rewrite(&mut self, oid: gitrust_core::Oid) {
        match self.repo().and_then(|repo| repo.plan_from(oid)) {
            Ok(plan) => {
                self.looking = None;
                self.deciding = None;
                self.rewriting = Some(plan);
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }

    fn act_on_rewrite(&mut self, act: rewrite_view::Act) {
        match act {
            rewrite_view::Act::Close => self.rewriting = None,
            rewrite_view::Act::Step { at, step } => {
                if let Some(plan) = &mut self.rewriting {
                    if let Some(line) = plan.lines.get_mut(at) {
                        line.step = step;
                    }
                }
            }
            rewrite_view::Act::Typed { at, text } => {
                if let Some(plan) = &mut self.rewriting {
                    if let Some(line) = plan.lines.get_mut(at) {
                        line.message = text;
                    }
                }
            }
            rewrite_view::Act::Move { at, up } => {
                if let Some(plan) = &mut self.rewriting {
                    let with = match up {
                        true => at.checked_sub(1),
                        false => (at + 1 < plan.lines.len()).then_some(at + 1),
                    };
                    if let Some(with) = with {
                        plan.lines.swap(at, with);
                    }
                }
            }
            rewrite_view::Act::Go => {
                let Some(plan) = self.rewriting.take() else {
                    return;
                };
                // Put back if it would not run, so the arranging is not lost
                // along with the attempt: a plan that clashes is a line or
                // two away from a plan that does not.
                match self.repo().and_then(|repo| repo.rewrite(&plan)) {
                    Ok(note) => {
                        self.load();
                        self.said(note);
                    }
                    Err(err) => {
                        self.rewriting = Some(plan);
                        self.note_error(format!("{err:#}"));
                    }
                }
            }
        }
    }

    /// Opens the screen for deciding what a merge or a rebase could not.
    fn decide(&mut self) {
        self.looking = None;
        let first = self.clashes.first().map(|clash| clash.path.clone());
        self.deciding = Some(conflict_view::Deciding {
            path: first,
            draft: None,
        });
    }

    fn act_on_conflict(&mut self, act: conflict_view::Act) {
        match act {
            conflict_view::Act::Close => self.deciding = None,
            conflict_view::Act::Pick(path) => {
                if let Some(state) = &mut self.deciding {
                    state.path = Some(path);
                    // Moving to another file puts away whatever was being
                    // typed into this one. It belonged to that file.
                    state.draft = None;
                }
            }
            conflict_view::Act::Typed(text) => {
                if let Some(state) = &mut self.deciding {
                    state.draft = Some(text);
                }
            }
            conflict_view::Act::Drop => {
                if let Some(state) = &mut self.deciding {
                    state.draft = None;
                }
            }
            conflict_view::Act::Edit(path) => {
                let read = if let Some(remote) = self.remote.as_ref() {
                    remote.read_working_file(&path).and_then(|bytes| {
                        String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("{path} is a binary file and cannot be edited here"))
                    })
                } else {
                    self.repo().and_then(|repo| repo.clashing_text(&path))
                };
                match read {
                    Ok(text) => {
                        if let Some(state) = &mut self.deciding {
                            state.draft = Some(text);
                        }
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            conflict_view::Act::Take { path, side } => {
                let settled = path.clone();
                if self.is_remote() {
                    let theirs = side == gitrust_core::Side::Theirs;
                    self.run_remote(|repo| repo.take_conflict_side(&path, theirs));
                } else {
                    self.note_down(move |repo| repo.take_side(&path, side));
                }
                self.drop_orig(&settled);
                self.after_deciding();
            }
            conflict_view::Act::Write { path, text } => {
                let settled = path.clone();
                if self.is_remote() {
                    self.run_remote(|repo| {
                        repo.write_working_file(&path, text.as_bytes())?;
                        repo.stage(&path)
                    });
                } else {
                    self.note_down(move |repo| repo.take_text(&path, &text));
                }
                self.drop_orig(&settled);
                if let Some(state) = &mut self.deciding {
                    state.draft = None;
                }
                self.after_deciding();
            }
            conflict_view::Act::Finish => {
                // One button, four things behind it, and which one is a fact
                // about the repository rather than about this screen -- so
                // the core picks. A rebase writes one commit and walks on
                // into the next, which is why the screen is pointed at
                // whatever is clashing now rather than put away.
                if self.is_remote() {
                    self.run_remote(|repo| repo.continue_operation().map(|_| ()));
                } else {
                    self.note_down(|repo| repo.see_it_through());
                }
                self.after_deciding();
            }
            conflict_view::Act::Abandon => self.ask_abandon(),
        }
    }

    /// Moves on to the next file still waiting, once one is settled.
    ///
    /// Staying on a file that is decided would show "that one is decided" and
    /// leave somebody to find the next one themselves, which on a merge with
    /// twenty clashing files is twenty pieces of pointless work.
    fn after_deciding(&mut self) {
        let next = self.clashes.first().map(|clash| clash.path.clone());
        if let Some(state) = &mut self.deciding {
            state.path = next;
        }
    }

    /// Throwing a half-done merge or rebase away loses work nothing gives
    /// back, so it is asked about by name.
    fn ask_abandon(&mut self) {
        let half = self.half;
        let files = self.clashes.len();
        // A rebase that has already replayed some commits loses those too,
        // and that is the part somebody would not think of: the files in
        // front of them are one commit of several.
        let also = match half {
            gitrust_core::Half::Rebase => {
                " Any commits this rebase had already replayed go back where they were."
            }
            gitrust_core::Half::CherryPick | gitrust_core::Half::Revert => {
                " The commit it came from is untouched either way."
            }
            _ => "",
        };
        self.ask(
            format!("Throw away {}?", half.says()),
            &format!(
                "Everything goes: the markers, both sides, and anything typed into \
                 those {files} files since it started. The branch goes back to where \
                 it was before, and nothing else is touched.{also}"
            ),
            "Throw it away",
            Deed::Abandon,
        );
    }

    fn act_on_file(&mut self, act: file_view::Act) {
        match act {
            file_view::Act::Close => self.looking = None,
            file_view::Act::Look(how) => {
                if let Some(looking) = &mut self.looking {
                    looking.how = how;
                    looking.trouble = None;
                }
                self.read_file();
            }
            // Another commit is another diff, another file and another
            // blame, so all three are thrown away rather than shown beside
            // the wrong hash. The history is not: it is about the file, it
            // is the list being clicked in, and walking it again on every
            // click would be the most expensive thing in the window
            // happening for no reason at all.
            file_view::Act::At(oid) => {
                if let Some(looking) = &mut self.looking {
                    looking.at = oid;
                    let story = looking.read.story.take();
                    looking.read = file_view::Read {
                        story,
                        ..Default::default()
                    };
                    looking.trouble = None;
                }
                self.read_file();
            }
            file_view::Act::Show(oid) => {
                self.looking = None;
                self.reveal_commit(oid);
                self.view = View::History;
            }
            file_view::Act::Revert(hunk) => {
                let Some(looking) = &self.looking else {
                    return;
                };
                let (path, at) = (looking.path.clone(), looking.at);
                self.note_down(move |repo| repo.revert_hunk(at, &path, hunk));
            }
            file_view::Act::Save(text) => {
                let Some(path) = self.looking.as_ref().map(|looking| looking.path.clone()) else { return };
                match self.remote.as_ref().ok_or_else(|| anyhow::anyhow!("not a remote checkout")).and_then(|repo| repo.write_working_file(&path, text.as_bytes())) {
                    Ok(()) => { if let Some(looking) = &mut self.looking { looking.editing = false; looking.read.whole = Some(gitrust_core::Contents::Text(text)); } self.load(); self.said(format!("{path} saved remotely")); }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
        }
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

    fn run_remote(&mut self, op: impl FnOnce(&RemoteRepo) -> Result<()>) -> bool {
        match self.remote.as_ref().ok_or_else(|| anyhow::anyhow!("this checkout is not connected: press Reconnect")).and_then(op) {
            Ok(()) => { self.load(); true }
            Err(err) => { self.note_error(format!("{err:#}")); false }
        }
    }

    /// Re-authenticates deliberately: with the key that was named for this
    /// checkout when one was, and otherwise with the profile's key when it
    /// has one and through the SSH agent after it.  The descriptor has no
    /// password and no passphrase, so a server that wants one asks the
    /// user to reopen it from the picker instead of retaining a secret for
    /// later.
    fn reconnect_remote(&mut self) {
        let Some(descriptor) = self.remote_descriptor.clone() else {
            return self.note_error("this tab has no remote connection details".to_string());
        };
        let login = descriptor.login.clone();
        let session = match descriptor.key_file() {
            Some(named) => gitrust_core::ssh::Session::connect_keyed(login, &named, None, false),
            None => {
                let key = profile_key(self.profile.as_ref());
                gitrust_core::ssh::Session::connect_as(login, None, key.as_deref(), false)
            }
        };
        match session.and_then(|session| RemoteRepo::new(session, descriptor.path.clone())) {
            Ok(mut repo) => {
                repo.keyed(descriptor.key.clone());
                repo.pruning(self.general.auto_prune);
                self.remote = Some(repo);
                self.name_the_remote();
                self.remote_disconnected = None;
                self.load();
                self.said("remote checkout reconnected".to_string());
            }
            Err(err) => self.note_error(format!("could not reconnect: {err:#}")),
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
        let profile = self.profile.clone();
        let prune = self.general.auto_prune;
        let turn = self.turn.clone();

        std::thread::spawn(move || {
            // What was asked for waits for the fetch that was not, which is
            // a second of somebody's time against a pull that fails on a
            // locked reference it had nothing to do with. Not for ever: a
            // quiet fetch hung on a dead network must not hold a push
            // hostage, and past ten seconds the two take their chances.
            let _held = wait_for(&turn, std::time::Duration::from_secs(10));
            let _ = send.send(
                Repo::open(&path)
                    .map(|repo| repo.using(profile).pruning(prune))
                    .and_then(|mut repo| work(&mut repo)),
            );
        });

        self.status = format!("{doing}…");
        self.status_failed = false;
        self.record(self.status.clone(), false, true);
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
        if self.is_remote() {
            let detail = if remote {
                "It is deleted from the checkout's upstream remote. Anyone who already has it keeps seeing it until they fetch."
            } else {
                "It is deleted from this remote checkout. Any commits only on it may become unreachable."
            };
            return self.ask(
                format!("Delete {name}?"),
                detail,
                "Delete",
                Deed::DeleteBranch { name, remote },
            );
        }
        if remote {
            let server = name
                .split_once('/')
                .map(|(server, _)| server)
                .unwrap_or("the remote");
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
        let alone = self
            .repo()
            .and_then(|repo| repo.unmerged(&name))
            .unwrap_or(0);
        let detail = match alone {
            0 => "Everything it has is somewhere else too, so nothing is lost.".to_string(),
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
        let profile = self.profile.clone();

        std::thread::spawn(move || {
            let _ = send.send(
                Repo::open(&path)
                    .map(|repo| repo.using(profile))
                    .and_then(|repo| repo.pull_requests()),
            );
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
        match updates::open_outside(url) {
            Ok(()) => {
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
        let profile = self.profile.clone();
        let (send, done) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let _ = send.send(
                Repo::open(&path)
                    .map(|repo| repo.using(profile))
                    .and_then(|repo| repo.board_against(&base, pulls.as_ref())),
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
        let loose = self
            .repo()
            .and_then(|repo| repo.worktree_changes(&name))
            .unwrap_or(0);
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
            Deed::Discard(path) if self.is_remote() => self.run_remote(|repo| repo.discard(&path)),
            Deed::Discard(path) => self.run(|repo| repo.discard(&path)),
            Deed::DiscardHunk { path, hunk } if self.is_remote() => self.run_remote(|repo| repo.discard_hunk(&path, hunk)),
            Deed::DiscardHunk { path, hunk } => self.run(|repo| repo.discard_hunk(&path, hunk)),
            Deed::DiscardAll(paths) if self.is_remote() => self.run_remote(|repo| repo.discard_all(&paths)),
            Deed::DiscardAll(paths) => self.run(|repo| repo.discard_all(&paths)),

            // The remote one goes out on the network; the one here is instant.
            Deed::DeleteBranch { name, remote } if remote && !self.is_remote() => {
                let doing = format!("deleting {name}");
                return self.spawn(&doing, move |repo| repo.delete_remote_branch(&name));
            }
            Deed::DeleteBranch { name, remote } if self.is_remote() => {
                if remote {
                    self.run_remote(|repo| repo.delete_remote_branch(&name).map(|_| ()));
                } else {
                    self.run_remote(|repo| repo.delete_branch(&name));
                }
                return;
            }
            Deed::MoveTo { oid, how } => {
                let doing = format!("moving to {}", &oid.to_string()[..7]);
                return self.spawn(&doing, move |repo| repo.move_branch(oid, how));
            }
            Deed::Abandon => {
                if self.is_remote() {
                    self.run_remote(|repo| repo.abort_operation().map(|_| ()));
                    return;
                }
                return self.spawn("throwing it away", |repo| repo.abandon());
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

        let (title, detail, verb) = (
            confirm.title.clone(),
            confirm.detail.clone(),
            confirm.verb.clone(),
        );
        let mut go = false;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let modal = egui::Modal::new(egui::Id::new("confirm"))
            .frame(
                egui::Frame::NONE
                    .fill(theme::raised())
                    .inner_margin(egui::Margin::same(18))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .stroke(egui::Stroke::new(1.0, theme::edge())),
            )
            .show(ctx, |ui| {
                ui.set_max_width(372.0);
                ui.label(
                    egui::RichText::new(&title)
                        .font(theme::strong(14.0))
                        .color(theme::text()),
                );
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&detail)
                            .font(theme::body(12.0))
                            .color(theme::muted()),
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
                                .color(theme::removed()),
                        )
                        .fill(theme::tint(theme::removed(), 38))
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
        let found = self
            .repo()
            .ok()
            .and_then(|repo| repo.note(&branch).ok().flatten());
        self.describing = Some(Describing {
            had: found.is_some(),
            title: found
                .as_ref()
                .map(|note| note.title.clone())
                .unwrap_or_default(),
            body: found
                .as_ref()
                .map(|note| note.body.clone())
                .unwrap_or_default(),
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
                    .fill(theme::raised())
                    .inner_margin(egui::Margin::same(18))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .stroke(egui::Stroke::new(1.0, theme::edge())),
            )
            .show(ctx, |ui| {
                ui.set_width(452.0);
                ui.label(
                    egui::RichText::new("What is this branch for?")
                        .font(theme::strong(14.0))
                        .color(theme::text()),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(&branch)
                        .font(theme::mono(11.0))
                        .color(theme::muted()),
                );
                ui.add_space(12.0);

                ui.add(
                    egui::TextEdit::singleline(&mut note.title)
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(8.0, 6.0))
                        .font(theme::body(12.5))
                        .hint_text(
                            egui::RichText::new("in the words you would use out loud")
                                .color(theme::faint()),
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
                                .color(theme::faint()),
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
                                    egui::RichText::new("Take it away").color(theme::muted()),
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
                                .color(theme::text()),
                        )
                        .fill(theme::chosen())
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
    /// Puts the question a drop asks but cannot answer.
    ///
    /// Dropping one branch on another is one gesture with three meanings, and
    /// they are not near-misses of each other: a merge keeps both histories,
    /// a rebase writes one of them again, and a move up writes nothing at
    /// all. Nothing in the gesture says which, so nothing here guesses.
    fn ask_join(&mut self, from: String, onto: String, ctx: &egui::Context) {
        // Asked now rather than while the menu is up: it is one question
        // about two branches that cannot change while somebody is reading.
        let room = self
            .repo()
            .and_then(|repo| repo.has_room(&onto, &from))
            .unwrap_or(false);
        let at = ctx.pointer_latest_pos().unwrap_or_default();
        self.joining = Some(Joining {
            from,
            onto,
            room,
            at,
            armed: false,
        });
    }

    /// The menu the drop opens, where the drop happened.
    ///
    /// It is the same menu as the one on a right click, and deliberately so:
    /// a drop and a right click are two ways of asking the same question, and
    /// two different-looking answers to it would be two things to learn.
    fn join_menu(&mut self, ctx: &egui::Context) {
        let Some(joining) = &self.joining else {
            return;
        };
        let (from, onto, room, at, armed) = (
            joining.from.clone(),
            joining.onto.clone(),
            joining.room,
            joining.at,
            joining.armed,
        );

        // What each one would really do. It is not on the face of the menu --
        // three sentences would not be a menu -- but it is one hover away,
        // including the reason the greyed-out one is greyed out.
        let options: Vec<(Join, String, bool)> = vec![
            (
                Join::Merge,
                format!(
                    "One commit holding both sides. Nothing already written changes, \
                     and you are left on {onto}."
                ),
                true,
            ),
            (
                Join::Rebase,
                format!(
                    "{onto}'s own commits are written again on top of {from}, so the \
                     history comes out as one line. They are new commits: anyone who \
                     already has the old ones ends up with both."
                ),
                true,
            ),
            (
                Join::FastForward,
                match room {
                    true => format!(
                        "{onto} has nothing of its own since the two parted, so it just \
                         moves. No commit is written."
                    ),
                    false => format!(
                        "{onto} has commits that {from} does not, so there is no moving \
                         up to be done here."
                    ),
                },
                room,
            ),
        ];

        let mut chosen = None;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let area = egui::Area::new(egui::Id::new("joining"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            // Dropped near the bottom of the window, the menu would otherwise
            // open below it and be unreachable.
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_width(JOIN_MENU);

                    // A row in a menu is not a button with a box round it.
                    // Inside an area of our own the ordinary widget style
                    // applies, so the two lines egui's own menus set are set
                    // here: no fill and no outline until the pointer is on it.
                    let widgets = &mut ui.style_mut().visuals.widgets;
                    widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                    widgets.inactive.bg_stroke = egui::Stroke::NONE;

                    for (how, why, can) in options {
                        // The one that writes nothing goes under a rule: it
                        // is a different kind of answer from the two above,
                        // which both leave a commit behind.
                        if how == Join::FastForward {
                            theme::rule(ui);
                        }

                        let button = egui::Button::new(
                            egui::RichText::new(how.says(&from, &onto)).font(theme::body(12.5)),
                        )
                        .min_size(egui::vec2(ui.available_width(), 24.0));

                        let button = ui
                            .add_enabled(can, button)
                            .on_hover_text(&why)
                            .on_disabled_hover_text(&why);
                        if button.clicked() {
                            chosen = Some(how);
                        }
                    }
                });
            });

        // Clicking anywhere else puts it away, the way a menu does. Not on
        // the frame it opened: the pointer release that dropped the branch
        // is still that frame's, and it would close before it was seen.
        if armed {
            let outside = ctx.input(|i| i.pointer.any_click())
                && !ctx
                    .pointer_latest_pos()
                    .is_some_and(|pos| area.response.rect.contains(pos));
            close |= outside;
        }

        if let Some(how) = chosen {
            self.joining = None;
            let doing = how.doing(&from, &onto);
            self.spawn(&doing, move |repo| repo.join(&from, &onto, how));
            return;
        }

        match close {
            true => self.joining = None,
            false => {
                if let Some(joining) = &mut self.joining {
                    joining.armed = true;
                }
            }
        }
    }

    fn name_dialog(&mut self, ctx: &egui::Context) {
        let Some(naming) = &mut self.naming else {
            return;
        };

        let (title, hint, verb) = (
            naming.title.clone(),
            naming.hint.clone(),
            naming.verb.clone(),
        );
        let mut go = false;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        let modal = egui::Modal::new(egui::Id::new("naming"))
            .frame(
                egui::Frame::NONE
                    .fill(theme::raised())
                    .inner_margin(egui::Margin::same(18))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .stroke(egui::Stroke::new(1.0, theme::edge())),
            )
            .show(ctx, |ui| {
                ui.set_width(372.0);
                ui.label(
                    egui::RichText::new(&title)
                        .font(theme::strong(14.0))
                        .color(theme::text()),
                );
                ui.add_space(10.0);

                let entry = ui.add(
                    egui::TextEdit::singleline(&mut naming.draft)
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(8.0, 6.0))
                        .font(theme::mono(12.0))
                        .hint_text(egui::RichText::new(&hint).color(theme::faint())),
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
                                .color(theme::text()),
                        )
                        .fill(theme::chosen())
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
                Named::BranchAt(oid) if self.is_remote() => { self.run_remote(|repo| repo.branch_at(&name, oid).map(|_| ())); }
                Named::BranchAt(oid) => self.note_down(move |repo| repo.branch_at(&name, oid)),
                Named::Rename(old) if self.is_remote() => { self.run_remote(|repo| repo.rename_branch(&old, &name).map(|_| ())); }
                Named::Rename(old) => self.note_down(move |repo| repo.rename_branch(&old, &name)),
            };
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
        self.record(text.clone(), false, false);
        self.toast = Some(toast::Toast::new(text.clone(), false));
        self.status = text;
        self.status_failed = false;
    }

    /// The same, for something that went wrong: the bar marks it as a failure
    /// and the toast comes up in red and stays up for longer.
    fn note_error(&mut self, message: String) {
        self.record(message.clone(), true, false);
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

    fn remote_fail(&mut self, err: anyhow::Error) {
        let message = format!("{err:#}");
        self.remote_disconnected = Some(message.clone());
        self.fail(anyhow::anyhow!(message));
    }

    fn handle(&mut self, action: changes_view::Action) {
        use changes_view::Action;

        match action {
            Action::Select { path, staged } => {
                self.selected = Some((path.clone(), staged));
                self.diff = match self.remote.as_ref() {
                    Some(repo) => repo.file_diff(&path, staged).ok(),
                    None => self
                        .repo()
                        .and_then(|repo| repo.file_diff(&path, staged))
                        .ok(),
                };
            }
            Action::Stage(path) if self.is_remote() => { self.run_remote(|repo| repo.stage(&path)); }
            Action::Stage(path) => {
                self.run(|repo| repo.stage(&path));
            }
            Action::Unstage(path) if self.is_remote() => { self.run_remote(|repo| repo.unstage(&path)); }
            Action::Unstage(path) => {
                self.run(|repo| repo.unstage(&path));
            }
            Action::StageHunk { path, hunk } if self.is_remote() => {
                self.run_remote(|repo| repo.stage_hunk(&path, hunk));
            }
            Action::StageHunk { path, hunk } => {
                self.run(|repo| repo.stage_hunk(&path, hunk));
            }
            Action::UnstageHunk { path, hunk } if self.is_remote() => {
                self.run_remote(|repo| repo.unstage_hunk(&path, hunk));
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
                let paths: Vec<String> = self
                    .work
                    .unstaged
                    .iter()
                    .map(|file| file.path.clone())
                    .collect();
                self.ask(
                    format!("Discard {}?", theme::files_label(paths.len())),
                    "The ones Git knows go back to the index; the new ones go from the disk. None of it can be got back.",
                    "Discard",
                    Deed::DiscardAll(paths),
                );
            }
            Action::StageAll if self.is_remote() => {
                let paths: Vec<String> = self.work.unstaged.iter().map(|file| file.path.clone()).collect();
                self.run_remote(|repo| repo.stage_all(&paths));
            }
            Action::StageAll => {
                let paths: Vec<String> =
                    self.work.unstaged.iter().map(|f| f.path.clone()).collect();
                self.run(|repo| repo.stage_all(&paths));
            }
            Action::UnstageAll if self.is_remote() => {
                let paths: Vec<String> = self.work.staged.iter().map(|file| file.path.clone()).collect();
                self.run_remote(|repo| repo.unstage_all(&paths));
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
                    let message = match self.remote.as_ref() {
                        Some(repo) => repo.head_message().ok(),
                        None => self.repo().ok().and_then(|repo| repo.head_message()),
                    };
                    if let Some((subject, body)) = message {
                        self.subject = subject;
                        self.body = body;
                    }
                } else {
                    self.drop_amend();
                }
            }
            Action::Amend if self.is_remote() => {
                let message = compose(&self.subject, &self.body);
                let result = self.remote.as_ref().ok_or_else(|| anyhow::anyhow!("this checkout is not connected: press Reconnect")).and_then(|repo| repo.amend(&message));
                match result {
                    Ok(oid) => { self.saved = None; self.subject.clear(); self.body.clear(); self.selected_commit = Some(oid); self.head_at = Some(oid); self.detail_file = None; self.load(); self.said("last remote commit redone".to_string()); }
                    Err(err) => self.note_error(format!("{err:#}")),
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
                        // Written here, a moment ago: not the repository having moved.
                        self.head_at = Some(oid);
                        self.detail_file = None;
                        self.load();
                        self.said("last commit redone".to_string());
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            Action::Commit if self.is_remote() => {
                let message = compose(&self.subject, &self.body);
                let result = self.remote.as_ref().ok_or_else(|| anyhow::anyhow!("this checkout is not connected: press Reconnect")).and_then(|repo| repo.commit(&message));
                match result {
                    Ok(oid) => { self.subject.clear(); self.body.clear(); self.selected_commit = Some(oid); self.head_at = Some(oid); self.detail_file = None; self.load(); self.said(format!("commit created on {}", self.head)); }
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
                        // Written here, a moment ago: not the repository having moved.
                        self.head_at = Some(oid);
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

impl Open {
    fn act(&mut self, action: action_bar::Action) {
        use action_bar::Action;

        if self.is_remote() {
            match action {
                Action::Pull(mode) => self.run_remote(|repo| repo.pull(mode).map(|_| ())),
                Action::Fetch => self.run_remote(|repo| repo.fetch_all().map(|_| ())),
                Action::Push => self.run_remote(|repo| repo.push().map(|_| ())),
                Action::PushForce => self.run_remote(|repo| repo.push_force().map(|_| ())),
                Action::PushTags => self.run_remote(|repo| repo.push_tags().map(|_| ())),
                Action::Branch(name) => self.run_remote(|repo| repo.branch_here(&name).map(|_| ())),
                Action::StashSave(message) => self.run_remote(|repo| repo.stash_save(&message).map(|_| ())),
                Action::StashPop(index) => self.run_remote(|repo| repo.stash_pop(index).map(|_| ())),
                Action::StashApply(index) => self.run_remote(|repo| repo.stash_apply(index).map(|_| ())),
                Action::StashDrop(index) => self.run_remote(|repo| repo.stash_drop(index).map(|_| ())),
            };
            return;
        }

        match action {
            // The two that go out on the network leave for a thread of their
            // own. The rest are local and finish before anyone notices.
            Action::Pull(mode) => self.spawn("pulling", move |repo| repo.pull(mode)),
            Action::Fetch => self.spawn("fetching", |repo| repo.fetch_all()),
            Action::Push => self.spawn("pushing", |repo| repo.push()),
            Action::PushForce => self.spawn("forcing", |repo| repo.push_force()),
            Action::PushTags => self.spawn("sending tags", |repo| repo.push_tags()),

            Action::Branch(name) => match self.repo().and_then(|repo| repo.branch_here(&name)) {
                Ok(()) => {
                    self.load();
                    self.said(format!("you are now on {name}"));
                }
                Err(err) => self.note_error(format!("{err:#}")),
            },

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

impl Open {
    /// Everything this repository draws, in the order it is drawn.
    fn draw(&mut self, ctx: &egui::Context, settings: &mut Settings) {
        self.top_bar(ctx, settings);
        self.confirm_dialog(ctx);
        self.note_dialog(ctx);
        self.name_dialog(ctx);
        self.join_menu(ctx);
        self.status_bar(ctx);
        self.refs_panel(ctx);

        if self.view == View::History
            && self.looking.is_none()
            && self.deciding.is_none()
            && self.rewriting.is_none()
        {
            self.detail_panel(ctx);
        }

        // Something half-done is the one thing in this window that cannot
        // wait. It is said in a band under the bar, on every view, until it
        // is seen through or thrown away.
        self.half_bar(ctx);

        // Under whatever has the middle of the window, and between the two
        // side panels rather than across them: it is the graph that gives
        // up room to it, and the branches and the commit stay whole.
        self.terminal_panel(ctx, settings);

        if self.deciding.is_some() {
            let mut act = None;
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme::canvas())
                        .inner_margin(egui::Margin::symmetric(14, 0)),
                )
                .show(ctx, |ui| {
                    ui.push_id(self.salt, |ui| {
                        if let Some(state) = &self.deciding {
                            act = conflict_view::show(
                                ui,
                                self.half,
                                &self.clashes,
                                state,
                                self.job.is_some(),
                            );
                        }
                    });
                });
            if let Some(act) = act {
                self.act_on_conflict(act);
            }
            self.toasting(ctx);
            return;
        }

        // Arranging a rewrite takes the middle of the window too, and for
        // the same reason: it is a list you read before you run it.
        if self.rewriting.is_some() {
            let mut act = None;
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme::canvas())
                        .inner_margin(egui::Margin::symmetric(14, 0)),
                )
                .show(ctx, |ui| {
                    ui.push_id(self.salt, |ui| {
                        if let Some(plan) = &self.rewriting {
                            act = rewrite_view::show(ui, plan, self.job.is_some());
                        }
                    });
                });
            if let Some(act) = act {
                self.act_on_rewrite(act);
            }
            self.toasting(ctx);
            return;
        }

        // A file open across the window takes the middle of it. The views
        // above stay where they are: pressing one is a way out of here, and
        // taking them away would leave only the Back button.
        if self.looking.is_some() {
            let mut act = None;
            let busy = self.job.is_some();
            let editable = self.is_remote();
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme::canvas())
                        .inner_margin(egui::Margin::symmetric(14, 0)),
                )
                .show(ctx, |ui| {
                    ui.push_id(self.salt, |ui| {
                        if let Some(looking) = &mut self.looking {
                            act = file_view::show(ui, looking, busy, editable);
                        }
                    });
                });
            if let Some(act) = act {
                self.act_on_file(act);
            }
            self.toasting(ctx);
            return;
        }

        let frame = match self.view {
            // The graph is painted on the deepest background in the window so
            // that the lanes are the brightest thing on screen.
            View::History => egui::Frame::NONE.fill(theme::canvas()),
            View::Changes => egui::Frame::NONE.fill(theme::panel()),
            // The board is painted on the canvas so that the cards, which wear
            // the panels' color, lift off the background.
            View::Board => egui::Frame::NONE
                .fill(theme::canvas())
                .inner_margin(egui::Margin::symmetric(12, 0)),
        };

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            // Each tab keeps its own scroll positions and folds. Without
            // this they are all one window as far as egui is concerned,
            // and scrolling one repository's history scrolls the rest.
            ui.push_id(self.salt, |ui| match self.view {
                View::History => {
                    // With a detached HEAD there is no branch for the menu to
                    // merge into or move up, the same as in the sidebar.
                    let on_branch = self.refs.iter().any(|entry| entry.is_head);
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
                    let asked = graph_view::show(
                        ui,
                        &self.graph,
                        &self.refs,
                        self.selected_commit,
                        &self.work,
                        &needle,
                        self.bring.take(),
                        on_branch.then_some(self.head.as_str()),
                        self.job.is_some(),
                        self.is_remote(),
                        settings.waves,
                    );
                    if let Some(oid) = asked.show {
                        self.select_commit(oid);
                    }
                    if let Some(picked) = asked.pick { self.act_on_ref(picked, ctx); }
                    if asked.bottom {
                        self.more();
                    }
                }
                View::Board => {
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
                            self.view = View::History;
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
                View::Changes => {
                    let read_only = false;
                    let (amendable, pushed) = if self.is_remote() {
                        (!self.graph.rows.is_empty(), None)
                    } else {
                        self.amend_state()
                    };
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

                    if read_only {
                        ui.label(
                            egui::RichText::new(
                                "Remote checkout — staging and commits arrive in deliverable 3.",
                            )
                            .color(theme::muted()),
                        );
                    }
                    let action = changes_view::show(
                        ui,
                        &self.work,
                        selected,
                        self.diff.as_ref(),
                        &mut draft,
                        read_only,
                    );

                    if let Some(action) = action {
                        if !read_only || matches!(action, changes_view::Action::Select { .. }) {
                            self.handle(action);
                        }
                    }
                }
            })
        });

        self.toasting(ctx);
    }

    /// The band that says something is half-done.
    ///
    /// It is not a toast and it does not go away on its own. A repository in
    /// the middle of a merge behaves differently from one that is not --
    /// committing writes a merge, switching branches refuses -- and anything
    /// that changes what the window means has to stay on the screen while it
    /// is true.
    fn half_bar(&mut self, ctx: &egui::Context) {
        if self.half == gitrust_core::Half::None || self.deciding.is_some() {
            return;
        }

        let mut decide = false;
        egui::TopBottomPanel::top(egui::Id::new(("half", self.salt)))
            .frame(
                egui::Frame::NONE
                    .fill(theme::raised())
                    .inner_margin(egui::Margin::symmetric(12, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} is half-done here.",
                            match self.half {
                                gitrust_core::Half::Merge => "A merge",
                                gitrust_core::Half::Rebase => "A rebase",
                                gitrust_core::Half::CherryPick => "A commit being taken along",
                                gitrust_core::Half::Revert => "A commit being undone",
                                _ => "Something",
                            }
                        ))
                        .font(theme::medium(12.5))
                        .color(theme::head()),
                    );
                    ui.label(
                        egui::RichText::new(match self.clashes.len() {
                            0 => "Nothing left to decide.".to_string(),
                            1 => "1 file still has both answers in it.".to_string(),
                            n => format!("{n} files still have both answers in them."),
                        })
                        .font(theme::body(12.0))
                        .color(theme::muted()),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("See it through").font(theme::medium(12.0)),
                                )
                                .fill(theme::chosen()),
                            )
                            .clicked()
                        {
                            decide = true;
                        }
                    });
                });
            });

        if decide {
            self.decide();
        }
    }

    /// Last, over the panels and after whatever has just been pressed, so
    /// that a toast asked for this very frame is up before the frame ends.
    /// It takes itself away when its time is up.
    fn toasting(&mut self, ctx: &egui::Context) {
        if let Some(toast) = &self.toast {
            if !toast::show(ctx, toast) {
                self.toast = None;
            }
        }
    }
}

/// The terminal: a shell in this tab's folder, in a panel under the graph.
///
/// The shell is the tab's and the panel is the window's. Each repository
/// has its own shell, started the first time the panel is opened on it and
/// left running when the panel is put away; whether the panel is open, and
/// how tall, is one answer for every tab, because it is about where
/// somebody likes their terminal and not about a repository.
impl Open {
    /// The button for it, at the end of the actions. Says whether it was
    /// pressed.
    fn terminal_button(ui: &mut egui::Ui, settings: &Settings) -> bool {
        theme::vrule(ui);
        terminal_view::toggle(ui, settings.terminal == Panel::Expanded)
            .on_hover_text("Show/Hide Terminal (Ctrl+`)")
            .clicked()
    }

    /// Opens the panel, or puts it away when it is open. Folded counts as
    /// not open: pressing the button on a folded panel unfolds it.
    fn toggle_terminal(&mut self, settings: &mut Settings) {
        self.set_terminal(
            settings,
            match settings.terminal {
                Panel::Expanded => Panel::Hidden,
                _ => Panel::Expanded,
            },
        );
    }

    /// Shows that much of the panel, and writes it down.
    fn set_terminal(&mut self, settings: &mut Settings, panel: Panel) {
        if panel == Panel::Expanded {
            // Opened to be typed into, so the keyboard goes with it.
            if let Some(session) = &mut self.terminal {
                session.view.wants_focus = true;
            }
        }
        if settings.terminal != panel {
            settings.terminal = panel;
            if let Err(err) = settings.save() {
                self.note_error(format!("could not remember that: {err:#}"));
            }
        }
    }

    /// Starts this tab's shell.
    ///
    /// In the repository's folder, as whoever this tab is working as: the
    /// profile's name on what is committed from it and the profile's key
    /// on what is fetched and pushed, for the reason the buttons use them.
    /// Both are read when the shell starts. A profile changed afterwards
    /// is taken on by the next shell, not by one already running, whose
    /// environment is its own.
    ///
    /// For a checkout on another machine the shell is on that machine, in
    /// that folder, reached with `ssh`.
    fn start_terminal(&self, ctx: &egui::Context) -> std::io::Result<terminal::Session> {
        let who = self.profile.as_ref().and_then(Profile::who);
        // The key that was named for a checkout on another machine goes
        // before the profile's, as it does when the checkout is opened.
        let key = self
            .remote_descriptor
            .as_ref()
            .and_then(gitrust_core::ssh::RemoteDescriptor::key_file)
            .or_else(|| profile_key(self.profile.as_ref()));
        let caret = self.general.terminal_caret;
        let start = match &self.remote_descriptor {
            Some(remote) => terminal::Start {
                program: Some(terminal::remote_shell(
                    &remote.login.host,
                    &remote.login.user,
                    remote.login.port,
                    &remote.path,
                    key.as_deref(),
                    who,
                )),
                folder: None,
                env: Vec::new(),
                caret,
            },
            None => {
                let shell = self.general.terminal_shell.trim();
                terminal::Start {
                    program: (!shell.is_empty()).then(|| (shell.to_string(), Vec::new())),
                    folder: Some(std::path::PathBuf::from(&self.path)),
                    env: terminal::profile_env(who, key.as_deref()),
                    caret,
                }
            }
        };
        terminal::Session::start(ctx, start)
    }

    /// Hears the shell out, whether or not its panel is on screen. Says
    /// whether the shell has just ended.
    ///
    /// Every tab's, every frame, like the rest of the polling: a program
    /// in a terminal asks its terminal questions and waits for the answers,
    /// and it does not stop asking because somebody is looking at another
    /// tab.
    fn poll_terminal(&mut self, ctx: &egui::Context) -> bool {
        let Some(session) = &mut self.terminal else {
            return false;
        };
        session.pump(ctx);
        if session.is_over() {
            self.terminal = None;
            return true;
        }
        match session.quiet() {
            terminal::Quiet::Nothing => {}
            terminal::Quiet::Wait(wait) => ctx.request_repaint_after(wait),
            terminal::Quiet::Now { asked } => self.after_terminal(asked),
        }
        false
    }

    /// Looks at the repository after the shell has had something to say.
    ///
    /// What is typed into a terminal changes the repository behind the
    /// window's back, and a graph that still shows the commit `git reset`
    /// has just taken away is worse than no graph. The window would find
    /// out by itself within a couple of seconds, from the thread that
    /// looks; this is so that it does not take a couple of seconds. It
    /// looks when the shell goes quiet: at the branches and HEAD every
    /// time, which is cheap, and at the working tree only after a line
    /// was sent in, which is a walk through it.
    ///
    /// Everything is read again only when a branch, HEAD, the stashes or
    /// what is half-done has actually changed. A reload puts away a commit
    /// message being amended and moves what is selected, and `ls` is not a
    /// reason for either.
    ///
    /// Not for a checkout on another machine: looking there is a round
    /// trip on the thread that paints, and one after every command would
    /// be a window that stutters while somebody types. The reload button
    /// is still where it was.
    fn after_terminal(&mut self, asked: bool) {
        if self.is_remote() || self.job.is_some() {
            return;
        }
        let Ok(mut repo) = self.repo() else {
            return;
        };
        let refs = repo.references().unwrap_or_default();
        let same = refs.len() == self.refs.len()
            && refs.iter().zip(&self.refs).all(|(now, then)| {
                now.name == then.name
                    && now.kind == then.kind
                    && now.target == then.target
                    && now.is_head == then.is_head
            })
            && repo.head_name() == self.head
            && (!self.looked || repo.head_id() == self.head_at)
            && repo.half_done() == self.half
            && repo.stashes().map(|stashes| stashes.len()).unwrap_or(0) == self.stashes.len();
        drop(repo);

        match (same, asked) {
            (false, _) => self.load(),
            (true, true) => self.refresh(),
            (true, false) => {}
        }
    }

    /// Is told what the repository looked like a moment ago, by the thread
    /// that looks while nobody is pressing anything.
    ///
    /// The same as what is on screen is the usual answer, and nothing
    /// happens: that is every look that found nothing, and every look that
    /// found what this tab has just done itself.
    ///
    /// Otherwise something else has been at the repository. What is read
    /// again is decided here and now and not by what was seen: a look is
    /// a moment old by the time it arrives, and a commit made from this
    /// window in that moment would otherwise look like one made behind its
    /// back. So Git's half is asked again, which is cheap. When it has
    /// moved, everything is read, as the reload button would; when it has
    /// not, only the working tree is, and a commit message being written
    /// stays where it is.
    ///
    /// Not while something slow is running, which reads everything itself
    /// when it ends, and not for a checkout on another machine.
    fn heard(&mut self, seen: Pulse) {
        if self.is_remote() || self.job.is_some() || self.pulse == Some(seen) {
            return;
        }
        let Ok(repo) = self.repo() else {
            return;
        };
        let moved = self.pulse.map(|shown| shown.git) != Some(repo.git_pulse());
        drop(repo);
        match moved {
            true => self.load(),
            false => self.refresh(),
        }
    }

    /// The panel itself: its bar, and under it the grid.
    fn terminal_panel(&mut self, ctx: &egui::Context, settings: &mut Settings) {
        // The key GitKraken and most editors answer to, wherever the
        // keyboard is -- including in the terminal, which is where it is
        // when somebody wants the panel out of the way.
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Backtick)) {
            self.toggle_terminal(settings);
        }

        let title = self
            .terminal
            .as_ref()
            .and_then(|session| session.title())
            .map(str::to_string);
        let mut act = None;

        match settings.terminal {
            Panel::Hidden => return,
            Panel::Minimized => {
                egui::TopBottomPanel::bottom("terminal-folded")
                    .exact_height(terminal_view::BAR)
                    .frame(egui::Frame::NONE.fill(theme::raised()))
                    .show(ctx, |ui| {
                        act = terminal_view::bar(ui, true, title.as_deref());
                    });
            }
            Panel::Expanded => {
                if self.terminal.is_none() {
                    match self.start_terminal(ctx) {
                        Ok(mut session) => {
                            session.view.wants_focus = true;
                            self.terminal = Some(session);
                        }
                        // Put away as well as said: an open panel asks for
                        // a shell on every frame, and this one has just
                        // been refused.
                        Err(err) => {
                            self.note_error(format!("Failed to start shell process: {err}"));
                            settings.terminal = Panel::Hidden;
                            let _ = settings.save();
                            return;
                        }
                    }
                }

                // Never so tall that what it is under cannot be seen.
                let shortest = gitrust_core::SHORTEST_TERMINAL as f32;
                let tallest = (ctx.screen_rect().height() * 0.75).max(shortest + 1.0);
                // One name for every tab, and not a tab's own: the height
                // is the window's, like whether the panel is open at all.
                let shown = egui::TopBottomPanel::bottom("terminal")
                    .resizable(true)
                    .default_height((settings.terminal_height as f32).clamp(shortest, tallest))
                    .height_range(shortest..=tallest)
                    .frame(egui::Frame::NONE.fill(theme::terminal_ground()))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let band = egui::Rect::from_min_size(
                            ui.max_rect().min,
                            egui::vec2(ui.max_rect().width(), terminal_view::BAR),
                        );
                        ui.painter().rect_filled(band, 0, theme::raised());
                        act = terminal_view::bar(ui, false, title.as_deref());
                        ui.push_id(self.salt, |ui| {
                            if let Some(session) = &mut self.terminal {
                                if let Some(asked) = terminal_view::show(ui, session, &self.general) {
                                    act = Some(asked);
                                }
                            }
                        });
                    });

                // Written down once the dragging is over, and not sixty
                // times a second while it is going on.
                let height = shown.response.rect.height().round() as u32;
                let dragging = ctx.input(|input| input.pointer.any_down());
                if height != settings.terminal_height && !dragging {
                    settings.terminal_height = height;
                    let _ = settings.save();
                }
            }
        }

        match act {
            Some(terminal_view::Act::Minimize) => self.set_terminal(settings, Panel::Minimized),
            Some(terminal_view::Act::Expand) => self.set_terminal(settings, Panel::Expanded),
            Some(terminal_view::Act::Hide) => self.set_terminal(settings, Panel::Hidden),
            // The shell is told to go by being let go of.
            Some(terminal_view::Act::Kill) => {
                self.terminal = None;
                self.set_terminal(settings, Panel::Hidden);
            }
            Some(terminal_view::Act::Open(address)) => self.open_in_browser(&address),
            None => {}
        }
    }
}

impl Open {
    fn top_bar(&mut self, ctx: &egui::Context, settings: &mut Settings) {
        if self.is_remote() {
            return self.remote_top_bar(ctx, settings);
        }
        egui::TopBottomPanel::top(egui::Id::new(("bar", self.salt)))
            .frame(
                egui::Frame::NONE
                    .fill(theme::raised())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.repo_menu(ui);
                    theme::vrule(ui);
                    self.views(ui, true);
                    theme::vrule(ui);

                    // Where HEAD is, in the window's one and only amber. It
                    // sits by the actions because they all end up there.
                    if !self.head.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.head)
                                .font(theme::medium(12.0))
                                .color(theme::head())
                                .background_color(theme::tint(theme::head(), 28)),
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
                    let terminal = Self::terminal_button(ui, settings);

                    let mut reload = false;
                    // Taken out and put back rather than lent to the closure:
                    // the closure already holds the rest of us.
                    let mut waves = settings.waves;
                    let mut appearance_changed = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        reload = ui
                            .add_enabled(self.job.is_none(), egui::Button::new("↻"))
                            .on_hover_text("Reload")
                            .clicked();
                        ui.add_space(6.0);
                        appearance_changed = appearance_control(ui, settings);
                        ui.add_space(6.0);
                        ui.toggle_value(&mut waves, "Waves")
                            .on_hover_text("The wave the pointer leaves in the graph");
                    });

                    if waves != settings.waves || appearance_changed {
                        settings.waves = waves;
                        // Said out loud only when it fails. A preference that
                        // cannot be written down is one that will be back on
                        // tomorrow, and finding that out tomorrow is worse
                        // than being told now.
                        if let Err(err) = settings.save() {
                            self.note_error(format!("could not remember that: {err:#}"));
                        }
                    }

                    if let Some(action) = action {
                        self.act(action);
                    } else if reload {
                        self.load();
                    } else if terminal {
                        self.toggle_terminal(settings);
                    }
                });
            });
    }

    fn remote_top_bar(&mut self, ctx: &egui::Context, settings: &mut Settings) {
        egui::TopBottomPanel::top(egui::Id::new(("bar", self.salt)))
            .frame(
                egui::Frame::NONE
                    .fill(theme::raised())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(self.remote_project_name())
                            .font(theme::medium(12.5))
                            .color(theme::head()),
                    );
                    ui.label(
                        egui::RichText::new("REMOTE")
                            .font(theme::medium(9.5))
                            .color(theme::text())
                            .background_color(theme::tint(theme::head(), 48)),
                    )
                    .on_hover_text("Remote checkout");
                    theme::vrule(ui);
                    self.views(ui, false);
                    theme::vrule(ui);
                    if self.remote.is_none() {
                        ui.label(egui::RichText::new("Not connected").color(theme::muted()));
                    } else if self.remote_disconnected.is_some() {
                        ui.label(egui::RichText::new("Connection lost").color(theme::removed()));
                    } else {
                        ui.label(egui::RichText::new("Connected").color(theme::added()));
                    }
                    if ui
                        .button("Reconnect")
                        .on_hover_text("authenticate again through the SSH agent")
                        .clicked()
                    {
                        self.reconnect_remote();
                    }
                    let action = action_bar::show(
                        ui,
                        self.tracking.as_ref(),
                        &self.stashes,
                        &mut self.pull_mode,
                        self.job.is_some(),
                        &mut self.draft,
                    );
                    let terminal = Self::terminal_button(ui, settings);
                    let mut appearance_changed = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("↻")
                            .on_hover_text("Reload remote checkout")
                            .clicked()
                        {
                            self.load();
                        }
                        ui.add_space(6.0);
                        appearance_changed = appearance_control(ui, settings);
                    });
                    if appearance_changed {
                        if let Err(err) = settings.save() {
                            self.note_error(format!("could not remember that: {err:#}"));
                        }
                    }
                    if let Some(action) = action {
                        self.act(action);
                    } else if terminal {
                        self.toggle_terminal(settings);
                    }
                })
            });
    }

    /// The remote's directory name is the project name; the connection
    /// address is useful context, but it is not the name people look for in
    /// the tab bar.
    fn remote_project_name(&self) -> String {
        self.remote_descriptor
            .as_ref()
            .and_then(|remote| {
                std::path::Path::new(remote.path.trim_end_matches('/'))
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "remote repository".to_string())
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
                ui.set_width(REPO_MENU);
                let mut go = false;

                ui.horizontal(|ui| {
                    let entry = ui.add(
                        egui::TextEdit::singleline(&mut self.path)
                            .desired_width(ui.available_width() - 66.0)
                            .margin(egui::vec2(8.0, 5.0))
                            .font(theme::mono(11.5))
                            .hint_text(
                                egui::RichText::new("path to a repository").color(theme::faint()),
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
            // Another repository is somewhere you have not gone into yet,
            // even when it happens to have a branch of the same name.
            self.trail.clear();
            self.project = None;
            self.load();
        }
    }

    /// The views share a single outlined control, so they read as a mode
    /// switch instead of three unrelated buttons.
    fn views(&mut self, ui: &mut egui::Ui, board: bool) {
        let pending = self.work.staged.len() + self.work.unstaged.len();

        egui::Frame::NONE
            .fill(theme::sunken())
            .stroke(egui::Stroke::new(1.0, theme::edge()))
            .corner_radius(egui::CornerRadius::same(theme::RADIUS))
            .inner_margin(egui::Margin::symmetric(2, 2))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.selectable_value(&mut self.view, View::History, "History");

                let changes = ui.selectable_value(&mut self.view, View::Changes, "Changes");
                if board {
                    ui.selectable_value(&mut self.view, View::Board, "Board");
                }
                if pending > 0 {
                    ui.label(
                        egui::RichText::new(pending.to_string())
                            .font(theme::mono(10.5))
                            .color(theme::changed())
                            .background_color(theme::tint(theme::changed(), 32)),
                    )
                    .on_hover_text("changes not committed");
                }
                let _ = changes;
            });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom(egui::Id::new(("status", self.salt)))
            .frame(
                egui::Frame::NONE
                    .fill(theme::raised())
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&self.summary)
                            .font(theme::body(11.5))
                            .color(theme::muted()),
                    );
                    // A conflict that has not happened yet, in the amber the
                    // window keeps for where you are: it is about this
                    // branch, and it is not yet anything that has gone wrong.
                    if let Some(seen) = &self.foreseen {
                        if let Some(text) = seen.says() {
                            ui.add_space(12.0);
                            let shown: Vec<&str> =
                                seen.files.iter().take(12).map(String::as_str).collect();
                            let mut files = shown.join("\n");
                            if seen.files.len() > shown.len() {
                                files.push_str(&format!(
                                    "\n\u{2026}and {} more",
                                    seen.files.len() - shown.len()
                                ));
                            }
                            ui.label(
                                egui::RichText::new(text)
                                    .font(theme::body(11.5))
                                    .color(theme::head()),
                            )
                            .on_hover_text(files);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if self.status_failed {
                            theme::removed()
                        } else {
                            theme::faint()
                        };
                        // The last thing that happened is the last line of
                        // the log, so it is also the way into the rest.
                        let last = match self.status.is_empty() {
                            true => "Activity log",
                            false => self.status.as_str(),
                        };
                        let line = ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(last).font(theme::body(11.5)).color(color),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Activity log");
                        let id = egui::Id::new(("activity", self.salt));
                        if line.clicked() {
                            ui.memory_mut(|memory| memory.toggle_popup(id));
                        }
                        egui::popup::popup_above_or_below_widget(
                            ui,
                            id,
                            &line,
                            egui::AboveOrBelow::Above,
                            egui::PopupCloseBehavior::CloseOnClickOutside,
                            |ui| self.activity(ui),
                        );
                    });
                });
            });
    }

    /// The activity log: what has happened in this tab, newest first.
    fn activity(&self, ui: &mut egui::Ui) {
        ui.set_width(LOG_MENU);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Activity log")
                    .font(theme::medium(12.0))
                    .color(theme::text()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(match self.general.extended_log {
                        true => "extended: what you did and what the window did by itself",
                        false => "what you did, and how it went",
                    })
                    .font(theme::body(11.0))
                    .color(theme::faint()),
                );
            });
        });
        theme::rule(ui);

        if self.log.is_empty() {
            ui.label(
                egui::RichText::new("Nothing has happened here yet.")
                    .font(theme::body(11.5))
                    .color(theme::faint()),
            );
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt(("activity-lines", self.salt))
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for line in self.log.iter().rev() {
                    ui.horizontal_top(|ui| {
                        ui.label(
                            egui::RichText::new(line.at.format("%H:%M:%S").to_string())
                                .font(theme::mono(10.5))
                                .color(theme::faint()),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&line.text)
                                .font(theme::body(11.5))
                                .color(match line.failed {
                                    true => theme::removed(),
                                    false => theme::text(),
                                }),
                        );
                    });
                }
            });
    }

    /// The references come along always, whether you look at the history or
    /// at the changes.
    fn refs_panel(&mut self, ctx: &egui::Context) {
        let mut picked = None;
        let remote_checkout = self.is_remote();
        // With a detached HEAD there is no branch to bring anything into.
        let on_branch = self.refs.iter().any(|entry| entry.is_head);

        egui::SidePanel::left(egui::Id::new(("refs-panel", self.salt)))
            .resizable(true)
            .default_width(222.0)
            .width_range(170.0..=360.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::panel())
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(true, |ui| {
                    picked = refs_view::show(
                        ui,
                        &self.refs,
                        &self.worktrees,
                        std::path::Path::new(&self.path),
                        &mut self.ref_filter,
                        self.selected_commit,
                        on_branch.then_some(self.head.as_str()),
                        self.job.is_some(),
                        remote_checkout,
                    );
                });
            });

        if let Some(picked) = picked {
            self.act_on_ref(picked, ctx);
        }
    }

    /// Carries out whatever was asked of a reference.
    ///
    /// The sidebar and the graph offer the same menu, so they answer to the
    /// same place: anything added here is in both at once, which is the point
    /// of them sharing it.
    fn act_on_ref(&mut self, picked: refs_view::Pick, ctx: &egui::Context) {
        if self.is_remote() {
            let result = match picked {
                refs_view::Pick::Show(oid) => { self.reveal_commit(oid); self.view = View::History; return; }
                refs_view::Pick::Switch { name, remote: true } => self.remote.as_ref().map(|repo| repo.checkout_remote(&name)),
                refs_view::Pick::Switch { name, remote: false } => self.remote.as_ref().map(|repo| repo.checkout_branch(&name)),
                refs_view::Pick::Merge(name) => self.remote.as_ref().map(|repo| repo.merge(&name)),
                refs_view::Pick::Rebase(name) => self.remote.as_ref().map(|repo| repo.rebase_onto(&name)),
                refs_view::Pick::Pull => self.remote.as_ref().map(|repo| repo.pull(PullMode::Merge)),
                refs_view::Pick::Push(name) => self.remote.as_ref().map(|repo| repo.push_branch(&name)),
                refs_view::Pick::Follow { branch, upstream } => self.remote.as_ref().map(|repo| repo.set_upstream(&branch, upstream.as_deref())),
                refs_view::Pick::DeleteBranch { name, remote } => { self.ask_delete_branch(name, remote); return; }
                refs_view::Pick::DeleteTag(name) => self.remote.as_ref().map(|repo| repo.delete_tag(&name).map(|_| format!("tag {name} deleted"))),
                refs_view::Pick::BranchAt(oid) => {
                    self.naming = Some(Naming { title: format!("A branch at {}", &oid.to_string()[..7]), hint: "feat/something".to_string(), verb: "Make it".to_string(), draft: String::new(), deed: Named::BranchAt(oid) });
                    return;
                }
                refs_view::Pick::Rename(name) => {
                    self.naming = Some(Naming { title: format!("Rename {name}"), hint: name.clone(), verb: "Rename".to_string(), draft: name.clone(), deed: Named::Rename(name) });
                    return;
                }
                refs_view::Pick::GoTo(oid) => self.remote.as_ref().map(|repo| repo.checkout_commit(oid)),
                refs_view::Pick::CherryPick(oid) => self.remote.as_ref().map(|repo| repo.cherry_pick(oid)),
                refs_view::Pick::Revert(oid) => self.remote.as_ref().map(|repo| repo.revert(oid)),
                refs_view::Pick::Copy(text) => { ctx.copy_text(text.clone()); self.said(format!("{text} copied")); return; }
                refs_view::Pick::FastForward(_) | refs_view::Pick::Isolate(_) | refs_view::Pick::Rewrite(_) | refs_view::Pick::MoveTo(_, _) | refs_view::Pick::Visit(_) | refs_view::Pick::VisitTab(_) | refs_view::Pick::Release(_) | refs_view::Pick::Join { .. } => { self.note_error("that action is not available for a remote checkout".to_string()); return; }
            };
            match result { Some(Ok(note)) => { self.load(); self.said(note); }, Some(Err(err)) => self.note_error(format!("{err:#}")), None => self.note_error("no remote checkout".to_string()) }
            return;
        }
        match picked {
            refs_view::Pick::Show(oid) => {
                self.reveal_commit(oid);
                self.view = View::History;
            }
            refs_view::Pick::Switch { name, remote } => self.switch(name, remote),
            refs_view::Pick::Merge(name) => {
                // It can rewrite the whole tree, so it goes apart.
                let doing = format!("merging {name}");
                self.spawn(&doing, move |repo| repo.merge(&name));
            }
            refs_view::Pick::DeleteBranch { name, remote } => self.ask_delete_branch(name, remote),
            refs_view::Pick::FastForward(name) => {
                self.note_down(move |repo| repo.fast_forward_to(&name))
            }
            refs_view::Pick::Rebase(name) => {
                let doing = format!("rebasing onto {name}");
                self.spawn(&doing, move |repo| repo.rebase_onto(&name));
            }
            refs_view::Pick::Pull => {
                self.spawn("pulling", move |repo| repo.pull(PullMode::Merge));
            }
            refs_view::Pick::Push(name) => {
                let doing = format!("pushing {name}");
                self.spawn(&doing, move |repo| repo.push_branch(&name));
            }
            refs_view::Pick::Follow { branch, upstream } => {
                self.note_down(move |repo| repo.set_upstream(&branch, upstream.as_deref()))
            }
            refs_view::Pick::Isolate(name) => {
                let doing = format!("opening {name} in a folder of its own");
                self.spawn(&doing, move |repo| repo.add_worktree(&name));
            }
            refs_view::Pick::GoTo(oid) => {
                let doing = format!("going to {}", &oid.to_string()[..7]);
                self.spawn(&doing, move |repo| repo.checkout_commit(oid));
            }
            refs_view::Pick::BranchAt(oid) => {
                self.naming = Some(Naming {
                    title: format!("A branch at {}", &oid.to_string()[..7]),
                    hint: "feat/something".to_string(),
                    verb: "Make it".to_string(),
                    draft: String::new(),
                    deed: Named::BranchAt(oid),
                });
            }
            refs_view::Pick::Rename(name) => {
                self.naming = Some(Naming {
                    title: format!("Rename {name}"),
                    hint: name.clone(),
                    verb: "Rename".to_string(),
                    draft: name.clone(),
                    deed: Named::Rename(name),
                });
            }
            refs_view::Pick::CherryPick(oid) => {
                let doing = format!("taking {} along", &oid.to_string()[..7]);
                self.spawn(&doing, move |repo| repo.cherry_pick(oid));
            }
            refs_view::Pick::Revert(oid) => {
                let doing = format!("undoing {}", &oid.to_string()[..7]);
                self.spawn(&doing, move |repo| repo.revert(oid));
            }
            refs_view::Pick::MoveTo(oid, how) => self.ask_move(oid, how),
            refs_view::Pick::Rewrite(oid) => self.start_rewrite(oid),
            refs_view::Pick::Visit(path) => self.visit(path),
            refs_view::Pick::VisitTab(path) => self.elsewhere = Some(path),
            refs_view::Pick::Release(name) => self.ask_release(name),
            refs_view::Pick::Join { from, onto } => self.ask_join(from, onto, ctx),
            refs_view::Pick::Copy(text) => {
                ctx.copy_text(text.clone());
                self.said(format!("{text} copied"));
            }
            refs_view::Pick::DeleteTag(name) => {
                match self.repo().and_then(|repo| repo.delete_tag(&name)) {
                    Ok(()) => {
                        self.load();
                        self.said(format!("tag {name} deleted"));
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
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

        // How far it can be dragged open. There is a cap because without one
        // a commit with a long message widens the panel until the graph's
        // column is nothing, but the cap was a fixed 520 pixels, which is a
        // third of a laptop and a quarter of a desk monitor: on a wide screen
        // it stopped the panel while there was still room to spare, and a
        // diff is read at whatever width there is. Half the window is as far
        // as it can go and still leave a graph behind it. The floor is the
        // old number, so a narrow window is no worse off than before.
        let widest = (ctx.screen_rect().width() * 0.5).max(520.0);

        egui::SidePanel::right(egui::Id::new(("detail", self.salt)))
            .resizable(true)
            .default_width(420.0)
            .width_range(300.0..=widest)
            .frame(
                egui::Frame::NONE
                    // This panel is primarily a file list and diff. In Sol
                    // it is an editor surface, not surrounding chrome.
                    .fill(theme::file_surface())
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
                    // A remote checkout can tag through its own Git command;
                    // commit detail is otherwise read-only in either backend.
                    let read_only = false;
                    clicked = detail_view::show(
                        ui,
                        self.detail.as_ref(),
                        self.detail_file.as_deref(),
                        self.detail_diff.as_ref(),
                        &mut self.tag,
                        read_only,
                    );
                }
            });

        if let Some(action) = action {
            self.handle(action);
        }
        match clicked {
            Some(detail_view::Pick::File(path)) => self.select_detail_file(path),
            Some(detail_view::Pick::Open(path)) if !self.is_remote() => self.look_at(path),
            Some(detail_view::Pick::Tag { name, message }) => self.create_tag(name, message),
            Some(detail_view::Pick::Open(_)) => {}
            None => {}
        }
    }

    /// Tags the commit being looked at.
    fn create_tag(&mut self, name: String, message: String) {
        let Some(oid) = self.selected_commit.filter(|oid| !oid.is_zero()) else {
            return self.note_error("no commit is selected".to_string());
        };

        let result = if let Some(remote) = self.remote.as_ref() {
            remote.tag(&name, oid, &message)
        } else {
            self.repo().and_then(|repo| repo.tag(&name, oid, &message))
        };
        match result {
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
