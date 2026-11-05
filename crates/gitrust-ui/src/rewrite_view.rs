//! Arranging what should become of a run of commits.
//!
//! One row per commit, oldest at the top. That order is the opposite of the
//! graph's and it is not a slip: "fold this into the one above" has to mean
//! the row above, and a list that reads newest first would have it folding
//! downwards, which nobody has ever managed to picture.
//!
//! Nothing here is dangerous until the one button at the top right. Changing
//! a row, moving a row, typing a new message: all of it is arranging, and the
//! repository has not been touched. That is worth the screen saying out loud,
//! because the thing being arranged is the one operation in gitrust that
//! cannot be stopped halfway once it starts.

use eframe::egui::{self, RichText};
use gitrust_core::{Plan, Step};

use crate::theme;

/// How wide the dropdown saying what becomes of a commit is. Wide enough for
/// the longest of the five with room to spare, so it does not jump about.
const STEP: f32 = 96.0;

/// How tall the box for a new message is allowed to grow.
const REWORD: f32 = 96.0;

/// What has been asked for.
pub enum Act {
    /// Change what becomes of one commit.
    Step { at: usize, step: Step },
    /// Move one commit up or down the list.
    Move { at: usize, up: bool },
    /// What is being typed into a new message. Not a decision: typing is not
    /// running, and the two had better not arrive as the same message.
    Typed { at: usize, text: String },
    /// Do it.
    Go,
    /// Put it away, having changed nothing.
    Close,
}

pub fn show(ui: &mut egui::Ui, plan: &Plan, busy: bool) -> Option<Act> {
    let mut act = None;
    let trouble = plan.trouble();

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("\u{2190} Never mind").font(theme::body(12.0)))
            .on_hover_text("nothing here has been done yet")
            .clicked()
        {
            act = Some(Act::Close);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("Rewriting {}", plan.branch))
                .font(theme::medium(13.5))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new(plan.says())
                .font(theme::body(12.0))
                .color(theme::MUTED),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = trouble.is_none() && !busy;
            let go = ui.add_enabled(
                ready,
                egui::Button::new(RichText::new("Rewrite it").font(theme::medium(12.5)))
                    .fill(theme::CHOSEN),
            );
            let go = match &trouble {
                None => go.on_hover_text(
                    "works the whole thing out first. If anything clashes, \
                     nothing is written at all",
                ),
                Some(trouble) => go.on_disabled_hover_text(trouble.clone()),
            };
            if go.clicked() {
                act = Some(Act::Go);
            }
        });
    });

    ui.add_space(6.0);
    match &trouble {
        Some(trouble) => {
            ui.label(
                RichText::new(trouble)
                    .font(theme::body(12.0))
                    .color(theme::REMOVED),
            );
        }
        // The one thing about rewriting that surprises people, said where
        // they are about to do it rather than afterwards.
        None => {
            ui.label(
                RichText::new(
                    "Every one of these comes out as a new commit, even the ones \
                     kept exactly as they are. Anything already pushed and already \
                     pulled by somebody else will not match theirs afterwards.",
                )
                .font(theme::body(11.5))
                .color(theme::FAINT),
            );
        }
    }

    theme::rule(ui);

    egui::ScrollArea::vertical()
        .id_salt("rewrite-plan")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            let last = plan.lines.len().saturating_sub(1);
            for (at, line) in plan.lines.iter().enumerate() {
                row(ui, plan, at, last, busy, &mut act);
                if line.step == Step::Reword {
                    reword(ui, plan, at, &mut act);
                }
            }
            ui.add_space(20.0);
        });

    act
}

fn row(
    ui: &mut egui::Ui,
    plan: &Plan,
    at: usize,
    last: usize,
    busy: bool,
    act: &mut Option<Act>,
) {
    let line = &plan.lines[at];
    let gone = line.step == Step::Drop;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        // Up and down rather than dragging. A drag that lands one row off is
        // a different history, and this list is read before it is run.
        ui.add_enabled_ui(!busy, |ui| {
            if ui
                .add_enabled(at > 0, egui::Button::new(RichText::new("\u{2191}").font(theme::body(11.0))))
                .on_hover_text("move it up, so it happens sooner")
                .clicked()
            {
                *act = Some(Act::Move { at, up: true });
            }
            if ui
                .add_enabled(at < last, egui::Button::new(RichText::new("\u{2193}").font(theme::body(11.0))))
                .on_hover_text("move it down, so it happens later")
                .clicked()
            {
                *act = Some(Act::Move { at, up: false });
            }
        });

        ui.add_enabled_ui(!busy, |ui| {
            ui.scope(|ui| {
                ui.set_width(STEP);
                egui::ComboBox::from_id_salt(("step", at))
                    .selected_text(
                        RichText::new(line.step.says())
                            .font(theme::body(11.5))
                            .color(match line.step {
                                Step::Drop => theme::REMOVED,
                                Step::Pick => theme::MUTED,
                                _ => theme::HEAD,
                            }),
                    )
                    .width(STEP)
                    .show_ui(ui, |ui| {
                        for step in
                            [Step::Pick, Step::Reword, Step::Squash, Step::Fixup, Step::Drop]
                        {
                            if ui
                                .selectable_label(line.step == step, step.says())
                                .on_hover_text(step.means())
                                .clicked()
                            {
                                *act = Some(Act::Step { at, step });
                            }
                        }
                    });
            });
        });

        // A commit that folds into the one above is drawn under it, so the
        // shape of the list says what the words say.
        if line.step.folds() {
            ui.add_space(18.0);
        }

        ui.label(
            RichText::new(&line.short)
                .font(theme::mono(10.5))
                .color(match gone {
                    true => theme::FAINT,
                    false => theme::lane(0),
                }),
        );

        ui.add(
            egui::Label::new(
                RichText::new(&line.summary)
                    .font(theme::body(12.0))
                    .color(match gone {
                        true => theme::FAINT,
                        false => theme::TEXT,
                    })
                    .strikethrough_if(gone),
            )
            .truncate(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(&line.author)
                    .font(theme::body(10.5))
                    .color(theme::FAINT),
            );
        });
    });
    ui.add_space(3.0);
}

/// The box for a commit being given something else to say.
fn reword(ui: &mut egui::Ui, plan: &Plan, at: usize, act: &mut Option<Act>) {
    let line = &plan.lines[at];

    ui.horizontal(|ui| {
        ui.add_space(STEP + 56.0);
        ui.vertical(|ui| {
            let mut text = line.message.clone();
            // Scrolled, and capped. A multiline box grows to whatever is in
            // it, and a commit with a long message would push every other
            // row in the plan off the bottom of the window -- which is the
            // one thing this screen is for looking at.
            egui::ScrollArea::vertical()
                .id_salt(("reword", at))
                .max_height(REWORD)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(3)
                            .font(theme::mono(11.5))
                            .hint_text("what this commit should say"),
                    );
                });
            if text != line.message {
                *act = Some(Act::Typed { at, text });
            }
        });
    });
    ui.add_space(6.0);
}

/// `RichText` has no "strikethrough when", and a dropped commit wants one.
trait Struck {
    fn strikethrough_if(self, yes: bool) -> Self;
}

impl Struck for RichText {
    fn strikethrough_if(self, yes: bool) -> Self {
        match yes {
            true => self.strikethrough(),
            false => self,
        }
    }
}
