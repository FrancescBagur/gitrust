//! The newer version: looking for it, saying so, and bringing it down.
//!
//! What the window does about [`gitrust_core::update`]. It looks a little
//! after it opens and once a day after that, on a thread of its own,
//! and when there is something newer a word appears at the end of the
//! tab strip. Nothing else happens until that word is pressed: no dialog
//! over the repository, nothing brought down, nothing installed.
//!
//! Pressed, it offers the file for this system. That is brought down to
//! the folder downloads go to, checked against the sums published beside
//! it, and left there with a button that opens it -- which on most
//! systems is what installing it starts with. The last step is whoever
//! is sitting there's, and why is in the module this one speaks for.
//!
//! A look that nobody asked for says nothing when it fails: no network
//! is not news. One asked for from the preferences says what happened.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use eframe::egui::{self, RichText};
use gitrust_core::update::{self, Release, Version};

use crate::theme;

/// How long after the window opens the first look is made. Not at once:
/// the first seconds belong to the repository being read.
const FIRST: Duration = Duration::from_secs(15);
/// And how often after that.
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// After a look that could not be made, which is usually no network.
const AGAIN: Duration = Duration::from_secs(60 * 60);
/// How wide the menu under the word is.
const MENU: f32 = 320.0;

/// What was pressed in the menu.
pub enum Act {
    /// Bring the file for this system down.
    Fetch,
    /// Open what was brought down.
    Open(PathBuf),
    /// Open the folder it is in.
    Folder(PathBuf),
    /// Open the page that says what is in the version.
    Page(String),
    /// Do not mention this version again.
    Skip(String),
}

/// What is known about newer versions, and what is under way.
pub struct Updates {
    running: Version,
    /// When the next look that nobody asked for is due.
    next: Instant,
    /// Whether betas counted in the last look. A look made under the
    /// other answer is not an answer to this one.
    betas: Option<bool>,
    looking: Option<Receiver<Result<Option<Release>>>>,
    /// Whether the look under way was asked for, and so says how it went.
    asked: bool,
    found: Option<Release>,
    /// What the last look that was asked for had to say.
    answer: Option<(String, bool)>,
    fetching: Option<Receiver<Result<PathBuf>>>,
    fetched: Option<PathBuf>,
    /// Why the file did not come down, when it did not.
    trouble: Option<String>,
}

impl Updates {
    pub fn new() -> Self {
        Self {
            running: running(),
            next: Instant::now() + FIRST,
            betas: None,
            looking: None,
            asked: false,
            found: None,
            answer: None,
            fetching: None,
            fetched: None,
            trouble: None,
        }
    }

    /// The version this is, as the preferences show it.
    pub fn running(&self) -> String {
        self.running.to_string()
    }

    /// Whether a look is under way.
    pub fn looking(&self) -> bool {
        self.looking.is_some()
    }

    /// What the last look somebody asked for found, and whether that was
    /// a failure.
    pub fn answer(&self) -> Option<(&str, bool)> {
        self.answer.as_ref().map(|(text, wrong)| (text.as_str(), *wrong))
    }

    /// Hears what the threads have to say, and looks when it is time to.
    ///
    /// Every frame. `wanted` is whether the preferences say to look
    /// unasked, and `betas` whether a beta counts.
    pub fn tick(&mut self, ctx: &egui::Context, wanted: bool, betas: bool) {
        self.hear();
        if !wanted || self.looking.is_some() {
            return;
        }
        // The answer on screen was to another question.
        if self.betas.is_some_and(|looked| looked != betas) {
            self.next = Instant::now();
        }
        let now = Instant::now();
        match now >= self.next {
            true => self.look(ctx, betas, false),
            // egui sleeps when nothing moves. Without this the day would
            // be up whenever somebody next touched the mouse.
            false => ctx.request_repaint_after(self.next - now),
        }
    }

    /// Looks now, because somebody pressed the button that says so.
    pub fn look_now(&mut self, ctx: &egui::Context, betas: bool) {
        if self.looking.is_none() {
            self.answer = None;
            self.look(ctx, betas, true);
        }
    }

    fn look(&mut self, ctx: &egui::Context, betas: bool, asked: bool) {
        let (say, heard) = mpsc::channel();
        let (running, ctx) = (self.running.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = say.send(update::look(&running, betas));
            ctx.request_repaint();
        });
        self.looking = Some(heard);
        self.asked = asked;
        self.betas = Some(betas);
        self.next = Instant::now() + EVERY;
    }

    /// Starts bringing the file down, unless it is already on its way.
    pub fn fetch(&mut self, ctx: &egui::Context) {
        let (Some(release), None) = (self.found.clone(), &self.fetching) else {
            return;
        };
        let Some(folder) = update::downloads() else {
            self.trouble = Some("there is no home folder to put it in".to_string());
            return;
        };
        let (say, heard) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = say.send(update::fetch(&release, &folder));
            ctx.request_repaint();
        });
        self.fetching = Some(heard);
        self.fetched = None;
        self.trouble = None;
    }

    fn hear(&mut self) {
        if let Some(looking) = &self.looking {
            match looking.try_recv() {
                Ok(Ok(found)) => {
                    if self.asked {
                        self.answer = Some((
                            match &found {
                                Some(release) => format!("{} is out.", release.version),
                                None => "There is nothing newer than this.".to_string(),
                            },
                            false,
                        ));
                    }
                    // Another version than the one a file was brought down
                    // for: that file is not this one's.
                    if found != self.found {
                        self.fetched = None;
                        self.trouble = None;
                    }
                    self.found = found;
                    self.looking = None;
                }
                Ok(Err(err)) => {
                    if self.asked {
                        self.answer = Some((format!("Could not look: {err:#}"), true));
                    }
                    self.next = Instant::now() + AGAIN;
                    self.looking = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.looking = None,
            }
        }
        if let Some(fetching) = &self.fetching {
            match fetching.try_recv() {
                Ok(Ok(place)) => {
                    self.fetched = Some(place);
                    self.fetching = None;
                }
                Ok(Err(err)) => {
                    self.trouble = Some(format!("{err:#}"));
                    self.fetching = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.fetching = None,
            }
        }
    }

    /// The word at the end of the tab strip, and the menu under it.
    ///
    /// Nothing at all when there is nothing newer, or when the newer one
    /// is the one somebody said not to mention.
    pub fn word(&self, ui: &mut egui::Ui, skipped: &str) -> Option<Act> {
        let release = self.found.as_ref().filter(|release| release.tag != skipped)?;
        let mut act = None;

        let button = ui
            .add(
                egui::Button::new(
                    RichText::new(format!("{} is out", release.version))
                        .font(theme::medium(11.5))
                        .color(theme::text()),
                )
                .fill(theme::chosen())
                .corner_radius(egui::CornerRadius::same(theme::RADIUS)),
            )
            .on_hover_text(format!("A newer gitrust than this one, which is {}", self.running));
        let id = ui.make_persistent_id("menu-update");
        if button.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }

        egui::popup::popup_below_widget(
            ui,
            id,
            &button,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_width(MENU);
                // A menu stretches what is in it from edge to edge, which
                // suits a list of choices and not a paragraph or a path.
                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(
                    RichText::new(format!("gitrust {} is out", release.version))
                        .font(theme::strong(13.0))
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(format!("This one is {}.", self.running))
                        .font(theme::body(11.5))
                        .color(theme::muted()),
                );
                ui.add_space(2.0);

                let file = release.file();
                match (&self.fetched, self.fetching.is_some(), &file) {
                    (Some(place), _, _) => {
                        small(ui, "Brought down, and checked against the sums the release publishes:");
                        ui.label(
                            RichText::new(place.display().to_string())
                                .font(theme::mono(11.0))
                                .color(theme::text()),
                        );
                        if let Some(next) = place.file_name().and_then(|name| what_next(&name.to_string_lossy())) {
                            small(ui, next);
                        }
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new("Open it").fill(theme::chosen())).clicked() {
                                act = Some(Act::Open(place.clone()));
                            }
                            if ui.button("Show the folder").clicked() {
                                if let Some(folder) = place.parent() {
                                    act = Some(Act::Folder(folder.to_path_buf()));
                                }
                            }
                        });
                    }
                    (None, true, Some(name)) => {
                        small(ui, &format!("Bringing {name} down\u{2026}"));
                        ui.spinner();
                    }
                    (None, _, Some(name)) => {
                        if let Some(trouble) = &self.trouble {
                            ui.label(
                                RichText::new(trouble).font(theme::body(11.0)).color(theme::removed()),
                            );
                        }
                        let label = match self.trouble.is_some() {
                            true => "Try again",
                            false => "Download",
                        };
                        if ui.add(egui::Button::new(label).fill(theme::chosen())).clicked() {
                            act = Some(Act::Fetch);
                        }
                        small(
                            ui,
                            &format!(
                                "{name}, into your downloads, checked against the release's \
                                 SHA256SUMS. Nothing is installed: that is left to you."
                            ),
                        );
                    }
                    (None, _, None) => small(
                        ui,
                        "The release has no file for this system. Its page has every file, \
                         and the source.",
                    ),
                }

                theme::rule(ui);
                ui.horizontal(|ui| {
                    if ui.button("What is in it").clicked() {
                        act = Some(Act::Page(release.page()));
                    }
                    if ui
                        .button("Not this version")
                        .on_hover_text("Says no more about this one. The one after it is mentioned.")
                        .clicked()
                    {
                        act = Some(Act::Skip(release.tag.clone()));
                    }
                });
                });
            },
        );

        // Everything but the download puts the menu away: the download is
        // watched from it.
        if act.as_ref().is_some_and(|act| !matches!(act, Act::Fetch)) {
            ui.memory_mut(|memory| memory.close_popup());
        }
        act
    }
}

/// The version this is.
///
/// A build made for trying things can be told it is an older one, so
/// that the word at the top can be seen without waiting for a release.
/// Not a build anybody is given: there the variable is not read.
fn running() -> Version {
    if cfg!(debug_assertions) {
        if let Some(pretended) = std::env::var("GITRUST_PRETEND_VERSION")
            .ok()
            .and_then(|text| Version::parse(&text))
        {
            return pretended;
        }
    }
    Version::running()
}

/// What to do with a file once it is open, in a line.
fn what_next(name: &str) -> Option<&'static str> {
    if name.ends_with(".deb") {
        Some("Opening it hands it to the system's installer.")
    } else if name.ends_with(".tar.gz") {
        Some("Unpack it, and run packaging/install.sh --no-build from inside.")
    } else if name.contains("macos") {
        Some("Open it and drag Gitrust.app to Applications.")
    } else if name.contains("windows") {
        Some("Unpack it; the .exe runs from wherever you put it.")
    } else {
        None
    }
}

fn small(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::body(11.0)).color(theme::faint()));
}

/// Hands a file, a folder or an address to whatever the desktop opens
/// such things with.
///
/// The three systems have three programs for it. Only the first has been
/// run from here.
pub fn open_outside(what: impl AsRef<std::ffi::OsStr>) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        // Not `cmd /C start`, which reads an address with an ampersand in
        // it as two commands.
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(what).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_file_says_what_is_done_with_it() {
        assert!(what_next("gitrust_0.2.0_amd64.deb").unwrap().contains("installer"));
        assert!(what_next("gitrust-0.2.0-linux-x86_64.tar.gz").unwrap().contains("install.sh"));
        assert!(what_next("gitrust-0.2.0-macos-universal.zip").unwrap().contains("Applications"));
        assert!(what_next("gitrust-0.2.0-windows-x86_64.zip").unwrap().contains(".exe"));
        assert_eq!(what_next("SHA256SUMS"), None);
    }

    #[test]
    fn nothing_is_looked_for_before_its_time_or_when_not_wanted() {
        let ctx = egui::Context::default();
        let mut updates = Updates::new();
        // Just opened: the first look is a few seconds off.
        updates.tick(&ctx, true, false);
        assert!(!updates.looking());
        // Its time has come, and the preferences say not to.
        updates.next = Instant::now();
        updates.tick(&ctx, false, false);
        assert!(!updates.looking());
        assert!(updates.answer().is_none());
    }
}
