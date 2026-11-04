//! Deciding the files a merge could not decide on its own.
//!
//! Two columns, because two is what there is: what this branch says and what
//! the other one says. A third pane for the common ancestor is what a proper
//! three-way editor gives you, and it earns its place when you are picking
//! line by line -- which this does not do. Here the ancestor is one line of
//! text saying whether there was one, because the only thing it changes is
//! what kind of clash you are looking at.
//!
//! The way out by hand is a text box with the file as it sits on disk,
//! markers and all. It is not a merge editor either. It is the file, and
//! anybody who wants to do it properly has an editor already open.

use eframe::egui::{self, RichText};
use gitrust_core::{Clash, Contents, Half, Side};

use crate::theme;

/// What has been asked for.
pub enum Act {
    /// Look at that file.
    Pick(String),
    /// Settle it by taking one side whole.
    Take { path: String, side: Side },
    /// Settle it with what is in the box.
    Write { path: String, text: String },
    /// Start editing it by hand, with the file as it is on disk.
    Edit(String),
    /// What is in the box changed. Not a decision: typing is not deciding,
    /// and the two had better not be the same message.
    Typed(String),
    /// Stop editing without settling anything.
    Drop,
    /// Write the merge commit.
    Finish,
    /// Throw the whole half-done thing away.
    Abandon,
    /// Put the screen away and leave it half-done.
    Close,
}

/// What is being decided, and what is half-typed into it.
#[derive(Default)]
pub struct Deciding {
    pub path: Option<String>,
    /// The file being edited by hand, when one is. `None` means the two
    /// columns are showing instead.
    pub draft: Option<String>,
}

pub fn show(
    ui: &mut egui::Ui,
    half: Half,
    clashes: &[Clash],
    state: &Deciding,
    busy: bool,
) -> Option<Act> {
    let mut act = None;

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("\u{2190} Back").font(theme::body(12.0)))
            .on_hover_text("leave it half-done and look at something else")
            .clicked()
        {
            act = Some(Act::Close);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("{} is half-done", capitalise(half.says())))
                .font(theme::medium(13.5))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new(match clashes.len() {
                0 => "nothing left to decide".to_string(),
                1 => "1 file to decide".to_string(),
                n => format!("{n} files to decide"),
            })
            .font(theme::body(12.0))
            .color(match clashes.is_empty() {
                true => theme::ADDED,
                false => theme::HEAD,
            }),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(RichText::new("Throw it away").font(theme::body(12.0))),
                )
                .on_hover_text(
                    "everything goes: the markers, both sides, anything typed \
                     into these files since",
                )
                .clicked()
            {
                act = Some(Act::Abandon);
            }

            // Only this one can be seen through here. A rebase half-done is
            // a sequence of clashes with a notion of carrying on between
            // them, and saying so beats drawing a button that cannot work.
            if half.ours_to_finish() {
                let ready = clashes.is_empty() && !busy;
                let write = ui.add_enabled(
                    ready,
                    egui::Button::new(
                        RichText::new("Write the merge").font(theme::medium(12.5)),
                    )
                    .fill(theme::CHOSEN),
                );
                let write = match clashes.is_empty() {
                    true => write,
                    false => write.on_disabled_hover_text(
                        "there are still files with both answers in them",
                    ),
                };
                if write.clicked() {
                    act = Some(Act::Finish);
                }
            }
        });
    });

    if !half.ours_to_finish() {
        ui.add_space(10.0);
        ui.label(
            RichText::new(
                "This one was not started here and cannot be finished here. \
                 Throwing it away works, or carry on at the command line.",
            )
            .font(theme::body(12.0))
            .color(theme::MUTED),
        );
    }

    theme::rule(ui);

    if clashes.is_empty() {
        ui.add_space(14.0);
        theme::empty(ui, "Every file is decided. The merge is waiting to be written.");
        return act;
    }

    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        // The list on the left, narrow: these are paths, and what matters is
        // which one you are on and how many are left.
        ui.vertical(|ui| {
            ui.set_width(250.0);
            egui::ScrollArea::vertical()
                .id_salt("clash-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for clash in clashes {
                        if let Some(asked) = row(ui, clash, state) {
                            act = Some(asked);
                        }
                    }
                });
        });

        // A vertical line, not theme::rule: a rule takes the width it is
        // given, and inside a row that is every pixel the two columns were
        // going to be drawn in.
        ui.separator();

        ui.vertical(|ui| {
            let Some(path) = &state.path else {
                ui.add_space(14.0);
                return theme::empty(ui, "Pick one to decide it");
            };
            let Some(clash) = clashes.iter().find(|clash| &clash.path == path) else {
                ui.add_space(14.0);
                return theme::empty(ui, "That one is decided");
            };

            match &state.draft {
                Some(draft) => by_hand(ui, clash, draft, busy, &mut act),
                None => sides(ui, clash, busy, &mut act),
            }
        });
    });

    act
}

fn row(ui: &mut egui::Ui, clash: &Clash, state: &Deciding) -> Option<Act> {
    let mut act = None;
    let here = state.path.as_deref() == Some(clash.path.as_str());

    let response = ui.selectable_label(
        here,
        RichText::new(&clash.path).font(theme::mono(11.5)).color(theme::TEXT),
    );
    if response.on_hover_text(clash.shape()).clicked() {
        act = Some(Act::Pick(clash.path.clone()));
    }
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(clash.shape())
                .font(theme::body(10.5))
                .color(theme::FAINT),
        );
    });
    ui.add_space(4.0);

    act
}

/// The two answers, side by side, each with the button that takes it.
fn sides(ui: &mut egui::Ui, clash: &Clash, busy: bool, act: &mut Option<Act>) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&clash.path)
                .font(theme::medium(12.5))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new(match &clash.base {
                Contents::Absent => "neither side had it before",
                _ => "both sides changed what was here",
            })
            .font(theme::body(11.0))
            .color(theme::FAINT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(RichText::new("Write it myself").font(theme::body(11.5))),
                )
                .on_hover_text("the file as it is on disk, markers and all")
                .clicked()
            {
                *act = Some(Act::Edit(clash.path.clone()));
            }
        });
    });
    ui.add_space(8.0);

    let room = (ui.available_width() - 14.0) / 2.0;
    ui.horizontal_top(|ui| {
        for (side, label, contents) in [
            (Side::Ours, "This branch", &clash.ours),
            (Side::Theirs, "The other branch", &clash.theirs),
        ] {
            ui.vertical(|ui| {
                ui.set_width(room);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(label)
                            .font(theme::medium(12.0))
                            .color(match side {
                                Side::Ours => theme::HEAD,
                                Side::Theirs => theme::lane(0),
                            }),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(
                                    RichText::new("Take this").font(theme::body(11.5)),
                                ),
                            )
                            .clicked()
                        {
                            *act = Some(Act::Take { path: clash.path.clone(), side });
                        }
                    });
                });
                ui.add_space(4.0);
                version(ui, contents, side);
            });
        }
    });
}

/// One side's version of the file.
fn version(ui: &mut egui::Ui, contents: &Contents, side: Side) {
    // Deliberately not scrolled on its own. Two panes scrolling separately
    // means two files you cannot line up, and lining them up is the whole of
    // what you are doing here.
    match contents {
        Contents::Absent => {
            ui.label(
                RichText::new(match side {
                    Side::Ours => "Deleted on this branch",
                    Side::Theirs => "Deleted on the other branch",
                })
                .font(theme::body(12.0))
                .color(theme::REMOVED),
            );
        }
        Contents::Binary(size) => {
            ui.label(
                RichText::new(format!("Not text: {size} bytes"))
                    .font(theme::body(12.0))
                    .color(theme::MUTED),
            );
        }
        Contents::Text(text) => {
            egui::ScrollArea::vertical()
                .id_salt(("version", matches!(side, Side::Ours)))
                .max_height(ui.available_height() - 20.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Lines of a file want to sit as close as lines of a
                    // file do. The spacing is only half of it: a row is at
                    // least `interact_size` tall, and that is sized for
                    // buttons, not for text.
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.spacing_mut().interact_size.y = 13.0;
                    for (index, line) in text.lines().enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.add_sized(
                                [34.0, 15.0],
                                egui::Label::new(
                                    RichText::new((index + 1).to_string())
                                        .font(theme::mono(10.5))
                                        .color(theme::FAINT),
                                ),
                            );
                            ui.label(
                                RichText::new(line)
                                    .font(theme::mono(11.0))
                                    .color(theme::TEXT),
                            );
                        });
                    }
                });
        }
    }
}

/// The file on disk, in a box, for somebody who wants to say it their own way.
fn by_hand(
    ui: &mut egui::Ui,
    clash: &Clash,
    draft: &str,
    busy: bool,
    act: &mut Option<Act>,
) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&clash.path)
                .font(theme::medium(12.5))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new("take the markers out and say when it is right")
                .font(theme::body(11.0))
                .color(theme::FAINT),
        );
    });
    ui.add_space(8.0);

    // The box holds the draft; the owner of it is the application, because
    // this view may not keep anything between frames.
    let mut text = draft.to_string();
    let room = ui.available_height() - 48.0;
    egui::ScrollArea::vertical()
        .id_salt("by-hand")
        .max_height(room)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(24)
                    .font(theme::mono(11.5)),
            );
        });
    if text != draft {
        // Typing is a change to the draft, not a decision. Deciding is the
        // button below, and the two must not arrive as the same message.
        *act = Some(Act::Typed(text));
        return;
    }

    ui.add_space(8.0);
    let left = draft.lines().any(is_marker);
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(RichText::new("Never mind").font(theme::body(12.0))))
            .clicked()
        {
            *act = Some(Act::Drop);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let done = ui.add_enabled(
                !left && !busy,
                egui::Button::new(RichText::new("That is right").font(theme::medium(12.5)))
                    .fill(theme::CHOSEN),
            );
            let done = match left {
                false => done,
                true => done.on_disabled_hover_text(
                    "the conflict markers are still in it. \
                     Nothing afterwards would complain, and it would break \
                     when somebody ran it",
                ),
            };
            if done.clicked() {
                *act = Some(Act::Write {
                    path: clash.path.clone(),
                    text: draft.to_string(),
                });
            }
        });
    });
}

/// The same reading the core does, so the button and the refusal agree.
fn is_marker(line: &str) -> bool {
    line.starts_with("<<<<<<< ")
        || line.starts_with(">>>>>>> ")
        || line == "======="
        || line.starts_with("||||||| ")
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
