//! The screen for choosing a repository, cloning one, or starting one.
//!
//! It is what the plus in the tab strip opens, and the only screen in the
//! application that is not about a repository: everything else in the window
//! is a reading of one, and this is the list you reach before there is one to
//! read.
//!
//! Like every other view here it touches no repository. It is handed the list
//! and a line about each one, and hands back what was asked for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use gitrust_core::Known;

use crate::theme;

/// What has been asked for from the screen.
pub enum Act {
    /// Open that repository in a tab.
    Open(PathBuf),
    /// Bring one down from an address into a folder.
    Clone { url: String, into: PathBuf },
    /// Make an empty one there.
    Start(PathBuf),
    /// Pin it to the top, or stop.
    Star { path: PathBuf, starred: bool },
    /// Take it off the list. The folder is not touched.
    Forget(PathBuf),
    /// Put the screen away without choosing anything.
    Close,
    /// Ask the desktop for a folder, and put what comes back in a box.
    /// `beside` picks which: the one a clone lands in, or the one above it.
    Browse { beside: bool },
}

/// Which box is open, when one is.
#[derive(PartialEq, Eq, Clone, Copy)]
enum Asking {
    Open,
    Clone,
    Start,
}

impl Asking {
    fn hint(self) -> &'static str {
        match self {
            Asking::Open => "path to a repository",
            Asking::Clone => "address to clone from",
            Asking::Start => "path for the new repository",
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Asking::Open => "Open",
            Asking::Clone => "Clone",
            Asking::Start => "Start",
        }
    }
}

/// What is typed into the screen, which has to survive between frames.
#[derive(Default)]
pub struct Picker {
    needle: String,
    pub draft: String,
    /// Where a clone would land. Only used by the clone box, where it is
    /// filled in from the address as it is typed.
    pub beside: String,
    asking: Option<Asking>,
}

impl Picker {
    /// Where a clone typed in right now would go.
    fn landing(&self) -> Option<PathBuf> {
        gitrust_core::start::folder_for(&self.draft, Path::new(self.beside.trim()))
    }
}

/// Draws the screen and gives back whatever was asked of it.
///
/// `glance` is a line about each repository -- its branch and how far it is
/// from its remote -- worked out once by whoever opened this screen. Reading
/// it here, per row and per frame, would be opening twenty repositories sixty
/// times a second.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut Picker,
    known: &[Known],
    glance: &HashMap<PathBuf, String>,
    open_now: &[PathBuf],
    dialog: bool,
) -> Option<Act> {
    let mut act = None;

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Repositories")
                .font(theme::strong(17.0))
                .color(theme::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new("Close").min_size(egui::vec2(72.0, 26.0)))
                .clicked()
            {
                act = Some(Act::Close);
            }
        });
    });

    ui.add_space(12.0);
    if let Some(asked) = buttons(ui, state, dialog) {
        act = Some(asked);
    }

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut state.needle)
                .desired_width(ui.available_width() - 8.0)
                .margin(egui::vec2(8.0, 6.0))
                .font(theme::body(12.5))
                .hint_text(RichText::new("Find a repository").color(theme::FAINT)),
        );
    });
    ui.add_space(12.0);

    let needle = state.needle.trim().to_lowercase();
    let matching: Vec<&Known> = known
        .iter()
        .filter(|known| {
            needle.is_empty()
                || known.name().to_lowercase().contains(&needle)
                || known.path.display().to_string().to_lowercase().contains(&needle)
        })
        .collect();

    if known.is_empty() {
        theme::empty(
            ui,
            "No repositories yet. Open one, clone one, or start one.",
        );
        return act;
    }
    if matching.is_empty() {
        theme::empty(ui, "No repository by that name");
        return act;
    }

    // Three lists out of one, and a repository can be in two of them: the one
    // you are looking at can also be a favourite. Saying it twice is better
    // than making somebody look in both places to find out.
    let here: Vec<&Known> = matching.iter().copied().filter(|k| open_now.contains(&k.path)).collect();
    let starred: Vec<&Known> = matching.iter().copied().filter(|k| k.starred).collect();
    let rest: Vec<&Known> = matching
        .iter()
        .copied()
        .filter(|k| !k.starred && !open_now.contains(&k.path))
        .collect();

    egui::ScrollArea::vertical()
        .id_salt("picker")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (title, mark, group) in [
                ("Open now", theme::Glyph::Trees, here),
                ("Favourites", theme::Glyph::Tag, starred),
                ("Everything else", theme::Glyph::Folder, rest),
            ] {
                if group.is_empty() {
                    continue;
                }
                theme::section(ui, title, mark, group.len(), !needle.is_empty(), |ui| {
                    for known in group {
                        if let Some(asked) = row(ui, known, glance, open_now) {
                            act = Some(asked);
                        }
                    }
                });
            }
            ui.add_space(16.0);
        });

    act
}

/// The three ways a repository gets here, and the box behind whichever was
/// pressed.
fn buttons(ui: &mut egui::Ui, state: &mut Picker, dialog: bool) -> Option<Act> {
    let mut act = None;

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        for asking in [Asking::Open, Asking::Clone, Asking::Start] {
            let on = state.asking == Some(asking);
            let label = match asking {
                Asking::Open => "Open a folder",
                Asking::Clone => "Clone from an address",
                Asking::Start => "Start an empty one",
            };
            if ui.selectable_label(on, RichText::new(label).font(theme::body(12.5))).clicked() {
                // Pressing the one already open puts it away, which is what
                // anybody expects of a control that looks pressed in.
                state.asking = (!on).then_some(asking);
                state.draft.clear();
                if asking == Asking::Clone && state.beside.is_empty() {
                    // Somewhere to start from, since a clone needs a folder
                    // to land beside and typing a whole path to say "here" is
                    // a lot of typing.
                    state.beside = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                }
            }
        }
    });

    let Some(asking) = state.asking else {
        return None;
    };

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let room = ui.available_width() - 90.0;
        let entry = ui.add(
            egui::TextEdit::singleline(&mut state.draft)
                .desired_width(room)
                .margin(egui::vec2(8.0, 6.0))
                .font(theme::mono(12.0))
                .hint_text(RichText::new(asking.hint()).color(theme::FAINT)),
        );
        // Only while nothing else has it: asking every frame outright would
        // take it back off the button the moment the pointer put it there.
        if ui.memory(|memory| memory.focused().is_none()) {
            entry.request_focus();
        }

        // Only where a path is being asked for. There is no folder on disk
        // to point at when what is wanted is an address to clone from.
        if dialog && asking != Asking::Clone && ui.button("Browse\u{2026}").clicked() {
            act = Some(Act::Browse { beside: false });
        }

        let typed = !state.draft.trim().is_empty();
        let go = ui.add_enabled(
            typed,
            egui::Button::new(RichText::new(asking.verb()).font(theme::medium(12.5)))
                .fill(theme::CHOSEN)
                .min_size(egui::vec2(76.0, 28.0)),
        );
        let entered = entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        if typed && (go.clicked() || entered) {
            let path = PathBuf::from(state.draft.trim());
            act = match asking {
                Asking::Open => Some(Act::Open(path)),
                Asking::Start => Some(Act::Start(path)),
                Asking::Clone => state.landing().map(|into| Act::Clone {
                    url: state.draft.trim().to_string(),
                    into,
                }),
            };
        }
    });

    // Where a clone will land, said before it lands rather than after.
    if asking == Asking::Clone {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("into").font(theme::body(11.5)).color(theme::FAINT));
            ui.add(
                egui::TextEdit::singleline(&mut state.beside)
                    .desired_width(240.0)
                    .margin(egui::vec2(6.0, 4.0))
                    .font(theme::mono(11.0)),
            );
            if dialog && ui.small_button("\u{2026}").on_hover_text("Pick the folder").clicked() {
                act = Some(Act::Browse { beside: true });
            }
            let landing = state
                .landing()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "nowhere yet".to_string());
            ui.label(RichText::new(landing).font(theme::mono(11.0)).color(theme::MUTED));
        });
    }

    // Browse is the one answer that leaves the box where it is: what comes
    // back goes into it, and nothing has been chosen yet.
    if act.is_some() && !matches!(act, Some(Act::Browse { .. })) {
        state.asking = None;
        state.draft.clear();
    }
    act
}

/// One repository: what it is called, where it is, and what it was at.
fn row(
    ui: &mut egui::Ui,
    known: &Known,
    glance: &HashMap<PathBuf, String>,
    open_now: &[PathBuf],
) -> Option<Act> {
    let mut act = None;
    let there = known.there();
    let open = open_now.contains(&known.path);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        // The star is a button whether or not it is on, so the row does not
        // change shape when you press it.
        let star = ui
            .add(
                egui::Button::new(
                    RichText::new(match known.starred {
                        true => "\u{2605}",
                        false => "\u{2606}",
                    })
                    .font(theme::body(13.0))
                    .color(match known.starred {
                        true => theme::HEAD,
                        false => theme::FAINT,
                    }),
                )
                .frame(false)
                .min_size(egui::vec2(18.0, 18.0)),
            )
            .on_hover_text(match known.starred {
                true => "Stop keeping it at the top",
                false => "Keep it at the top",
            });
        if star.clicked() {
            act = Some(Act::Star { path: known.path.clone(), starred: !known.starred });
        }

        let name = ui.selectable_label(
            open,
            RichText::new(known.name())
                .font(theme::medium(12.5))
                .color(match there {
                    true => theme::TEXT,
                    false => theme::FAINT,
                }),
        );

        ui.label(
            RichText::new(known.parent())
                .font(theme::mono(11.0))
                .color(theme::FAINT),
        );

        // A folder that is gone is not the same as a repository that is gone:
        // a disk comes back. It is said and left on the list.
        if !there {
            ui.label(
                RichText::new("not where it was")
                    .font(theme::body(11.0))
                    .color(theme::REMOVED),
            );
        } else if let Some(line) = glance.get(&known.path) {
            ui.label(RichText::new(line).font(theme::mono(11.0)).color(theme::MUTED));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("\u{00d7}").font(theme::body(13.0)).color(theme::FAINT))
                        .frame(false)
                        .min_size(egui::vec2(18.0, 18.0)),
                )
                .on_hover_text("Forget it. The folder is not touched")
                .clicked()
            {
                act = Some(Act::Forget(known.path.clone()));
            }
        });

        let name = name.on_hover_text(match (there, open) {
            (false, _) => format!("{}\n\nNothing there right now", known.path.display()),
            (_, true) => format!("{}\n\nAlready open in a tab", known.path.display()),
            _ => format!("{}\n\nClick to open it in a tab", known.path.display()),
        });
        if name.clicked() && there {
            act = Some(Act::Open(known.path.clone()));
        }
    });
    ui.add_space(2.0);

    act
}
