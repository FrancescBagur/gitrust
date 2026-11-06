//! Deciding the files a merge, a rebase, a cherry-pick or an undo could not
//! decide on its own.
//!
//! Two columns, because two is what there is. What the two *are* depends on
//! what is half-done, and the screen never says "ours" and "theirs": a rebase
//! puts your own work in the column a merge puts the other branch in, and
//! anybody going by those two words takes the wrong side about half the time.
//! [`Half::sides`] names them and this draws whatever it is given.
//!
//! A third pane for the common ancestor is what a proper three-way editor
//! gives you, and it earns its place when you are picking line by line --
//! which this does not do. Here the ancestor is one line of text saying
//! whether there was one, because the only thing it changes is what kind of
//! clash you are looking at.
//!
//! The way out by hand is a text box with the file as it sits on disk,
//! markers and all. It is not a merge editor either. It is the file, and
//! anybody who wants to do it properly has an editor already open.

use eframe::egui::{self, RichText};
use gitrust_core::{Clash, Contents, Half, Side};

use crate::{syntax, theme};

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
    /// See it through: write the merge commit, or -- in a rebase -- write
    /// this commit and carry on to the next one.
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

            // Four things behind one button, and only three of them end
            // anything. "Write the merge" on a rebase that writes one commit
            // of nine and stops again at the next clash would be a lie told
            // at exactly the moment somebody is deciding whether to trust
            // this, so the core says what each one is called.
            if half.ours_to_finish() {
                let ready = clashes.is_empty() && !busy;
                let write = ui.add_enabled(
                    ready,
                    egui::Button::new(RichText::new(half.onward()).font(theme::medium(12.5)))
                        .fill(theme::CHOSEN),
                );
                let write = match clashes.is_empty() {
                    true => write.on_hover_text(match half {
                        Half::Rebase => {
                            "writes this commit and goes on to the next one, \
                             which may clash as well"
                        }
                        Half::CherryPick => "writes the change here as a commit of its own",
                        Half::Revert => "writes the commit that undoes it",
                        _ => "writes the merge commit",
                    }),
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
        theme::empty(
            ui,
            match half {
                Half::Rebase => {
                    "Every file is decided. This commit is waiting to be written, \
                     and the rebase goes on from there."
                }
                Half::CherryPick => {
                    "Every file is decided. The change is waiting to be written here."
                }
                Half::Revert => "Every file is decided. The undo is waiting to be written.",
                _ => "Every file is decided. The merge is waiting to be written.",
            },
        );
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
                None => sides(ui, half, clash, busy, &mut act),
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
fn sides(ui: &mut egui::Ui, half: Half, clash: &Clash, busy: bool, act: &mut Option<Act>) {
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

    // Whose two answers these are is not fixed: a rebase replays your work
    // on top of the other side, which puts them in the opposite columns to a
    // merge. The core says which, and this draws it.
    let (ours, theirs) = half.sides();

    // The room is halved before anything is drawn in it, and each column is
    // then given exactly its half and no more.
    //
    // Asking a column how wide it wants to be does not work here: a line of
    // code is as long as whoever wrote it made it, so the side with the long
    // line grew to fit it, and the other side was pushed off the edge of the
    // window -- its "Take this" button with it. The half that is there is the
    // half each one gets; what does not fit scrolls.
    let gap = ui.spacing().item_spacing.x;
    let room = ((ui.available_width() - gap) / 2.0).max(160.0);
    let tall = ui.available_height();

    ui.horizontal_top(|ui| {
        for (side, label, contents) in [
            (Side::Ours, ours, &clash.ours),
            (Side::Theirs, theirs, &clash.theirs),
        ] {
            ui.allocate_ui_with_layout(
                egui::vec2(room, tall),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(capitalise(label))
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
                            .on_hover_text(format!("this file becomes {}", side.says(half)))
                            .clicked()
                        {
                            *act = Some(Act::Take { path: clash.path.clone(), side });
                        }
                    });
                });
                ui.add_space(4.0);
                version(ui, contents, label, &clash.path);
                },
            );
        }
    });
}

/// One side's version of the file, under whatever that side is called here.
///
/// `path` is only for the colour: it is what says whether `#` starts a
/// comment or an attribute.
fn version(ui: &mut egui::Ui, contents: &Contents, label: &str, path: &str) {
    match contents {
        Contents::Absent => {
            ui.label(
                RichText::new(format!("Deleted in {label}"))
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
            // Scrolled both ways, and the across is not a nicety: the column
            // is half the room whatever is written in it, and a line longer
            // than that half would otherwise have an end nobody can read.
            //
            // What this costs is that the two sides scroll apart, and lining
            // them up is most of what anybody is doing here. It is the lesser
            // of the two: a column that keeps its place and hides the ends of
            // its long lines is still a column you can read down.
            //
            // Read for colour here, every frame, rather than once when the
            // clash is loaded: this view keeps nothing between frames. It is
            // the cheaper half of what this loop already does -- every line
            // below is a widget of its own, laid out whether or not anybody
            // scrolls down to it, and that is what costs.
            let painted = syntax::read(text, path);

            egui::ScrollArea::both()
                .id_salt(("version", label))
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
                            match painted.get(index) {
                                Some(pieces) => {
                                    ui.label(syntax::laid(pieces, theme::mono(11.0)));
                                }
                                None => {
                                    ui.label(
                                        RichText::new(line)
                                            .font(theme::mono(11.0))
                                            .color(theme::TEXT),
                                    );
                                }
                            }
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
