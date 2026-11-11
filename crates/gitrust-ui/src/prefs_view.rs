//! The screen for how the window behaves.
//!
//! The third screen that is not about a repository, after the one for
//! choosing one and the one for the profiles, and here for the reason they
//! are: how often to fetch and how much history to read are answers about
//! this computer, given once, and not about whichever tab is in front.
//!
//! It is one column of rows -- what the thing is called on the left, the
//! thing itself beside it, and a sentence under that saying what it does --
//! because that is what a preferences page is everywhere, and somebody who
//! has used one knows where to look on this one.
//!
//! There is no Save button. A row changed is a row written down, as on the
//! profiles screen and for the same reason: a button to forget to press.
//! The one row that is not a preference -- forgetting the tokens -- asks
//! before it does anything, because it is the one that cannot be undone by
//! clicking again.
//!
//! Like every other view here it touches nothing but what it is handed.

use eframe::egui::{self, RichText};
use gitrust_core::{start, General, FEWEST_COMMITS, LONGEST_FETCH_WAIT};

use crate::theme;

/// What has been asked of the screen.
pub enum Act {
    /// Take the token out of every account of every profile. Sent only
    /// after the second click.
    Forget,
    /// Put the screen away.
    Close,
}

/// What the screen remembers between frames.
pub struct Preferring {
    /// What is in the branch box, which is not always what is kept: a name
    /// no branch can have stays in the box, in red, and out of the file.
    branch: String,
    /// What Git would call a first branch here, read once when the screen
    /// opens. It is what the empty box shows.
    git_branch: String,
    /// Whether forgetting the tokens has been asked for and not confirmed.
    asking: bool,
}

impl Preferring {
    pub fn open(general: &General) -> Self {
        Self {
            branch: general.default_branch.clone(),
            git_branch: start::git_default_branch(),
            asking: false,
        }
    }
}

/// How wide the column of names is. Wide enough for the longest of them on
/// one line, so that every control starts at the same place.
const NAMES: f32 = 268.0;
/// How wide the sentences run before they wrap. A line the width of the
/// window is a line nobody reads to the end of.
const PROSE: f32 = 520.0;

/// Draws the screen. Gives back whether a row changed -- write the
/// preferences down as they now are -- and whatever was asked of it.
///
/// The two are apart because they can happen on one frame: a number typed
/// into a box is taken when the box loses the keyboard, and what takes the
/// keyboard away is often the click on Close.
///
/// `tokens` is how many are written down across the profiles: it is what
/// decides whether there is anything for the last row to forget.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut Preferring,
    general: &mut General,
    tokens: usize,
) -> (bool, Option<Act>) {
    let mut act = None;
    let before = general.clone();

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new("Preferences").font(theme::strong(17.0)).color(theme::text()));
        ui.add_space(10.0);
        ui.label(
            RichText::new("How this window behaves, whichever repository is in front")
                .font(theme::body(11.5))
                .color(theme::faint()),
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
    theme::rule(ui);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.add_space(28.0);
                ui.label(RichText::new("General").font(theme::strong(15.0)).color(theme::text()));
            });
            ui.add_space(18.0);

            row(ui, "Auto-Fetch Interval", |ui| {
                ui.add(
                    egui::DragValue::new(&mut general.auto_fetch)
                        .range(0..=LONGEST_FETCH_WAIT)
                        .speed(0.1),
                );
                help(
                    ui,
                    &format!(
                        "In minutes, between 0 and {LONGEST_FETCH_WAIT} (0 disables auto-fetch). \
                         Fetches every remote of the repository in front, without a word \
                         unless something came."
                    ),
                );
                warning(
                    ui,
                    "A checkout on another machine is not fetched this way: it is fetched \
                     when you ask.",
                );
            });

            row(ui, "Auto-Prune", |ui| {
                ui.checkbox(&mut general.auto_prune, "");
                help(
                    ui,
                    "A fetch also takes away the remote branches that have gone from the \
                     remote. Off leaves that to Git's own fetch.prune.",
                );
            });

            row(ui, "Keep submodules up to date", |ui| {
                ui.checkbox(
                    &mut general.submodules,
                    RichText::new("Bring every submodule to its recorded commit after a Git action")
                        .font(theme::body(11.5))
                        .color(theme::muted()),
                );
            });

            row(ui, "Default Branch Name", |ui| {
                let good = state.branch.is_empty() || start::is_branch_name(&state.branch);
                let typed = ui.add(
                    egui::TextEdit::singleline(&mut state.branch)
                        .desired_width(240.0)
                        .margin(egui::vec2(8.0, 5.0))
                        .font(theme::body(12.0))
                        .text_color(match good {
                            true => theme::text(),
                            false => theme::removed(),
                        })
                        .hint_text(RichText::new(&state.git_branch).color(theme::faint())),
                );
                if typed.changed() {
                    // A space is never part of a branch's name, and the one
                    // that gets typed is the one after it.
                    state.branch.retain(|letter| !letter.is_whitespace());
                    if state.branch.is_empty() || start::is_branch_name(&state.branch) {
                        general.default_branch = state.branch.clone();
                    }
                }
                match good {
                    true => help(
                        ui,
                        "The branch a repository made here starts on. Empty is what Git \
                         itself would call it.",
                    ),
                    false => complaint(
                        ui,
                        &format!(
                            "That is not a name a branch can have. New repositories still \
                             start on {}.",
                            match general.default_branch.is_empty() {
                                true => state.git_branch.as_str(),
                                false => general.default_branch.as_str(),
                            }
                        ),
                    ),
                }
            });

            row(ui, "Delete \u{201c}.orig\u{201d} files after merging", |ui| {
                ui.checkbox(&mut general.delete_orig, "");
                help(
                    ui,
                    "The copy a merge tool leaves beside a file goes when the conflict in \
                     that file is settled here. One the repository keeps is kept.",
                );
            });

            row(ui, "Show All Commits in Graph", |ui| {
                ui.checkbox(&mut general.show_all, "");
                warning(ui, "A long history takes longer to read every time the graph is.");
            });

            // The two below say how much of the history to read, which is a
            // question that has been answered already when the answer is
            // "all of it".
            if !general.show_all {
                row(ui, "Initial Commits in Graph", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut general.initial_commits)
                            .range(FEWEST_COMMITS..=10_000_000)
                            .speed(50.0)
                            .update_while_editing(false),
                    );
                    help(
                        ui,
                        &format!(
                            "Minimum {FEWEST_COMMITS}. With lazy loading on, more are read \
                             when you reach the earliest one in the graph."
                        ),
                    );
                    warning(ui, "A larger number takes longer to read every time the graph is.");
                });

                row(ui, "Lazy Load Commits in Graph", |ui| {
                    ui.checkbox(&mut general.lazy_load, "");
                });
            }

            row(ui, "Remember tabs", |ui| {
                ui.checkbox(&mut general.remember_tabs, "");
                help(
                    ui,
                    "The repositories open now are open the next time. A checkout on \
                     another machine comes back waiting to be reconnected.",
                );
            });

            row(ui, "Use extended logging in activity log", |ui| {
                ui.checkbox(&mut general.extended_log, "");
                help(
                    ui,
                    "Also lists what the window did by itself: the fetches nobody asked \
                     for, the reloads, the submodules. The log opens from the line at the \
                     bottom right of a repository.",
                );
            });

            row(ui, "Forget all tokens", |ui| {
                match state.asking {
                    false => {
                        let button = egui::Button::new(
                            RichText::new("Forget All").font(theme::body(12.0)).color(theme::removed()),
                        )
                        .stroke(egui::Stroke::new(1.0, theme::tint(theme::removed(), 150)))
                        .min_size(egui::vec2(88.0, 24.0));
                        if ui.add_enabled(tokens > 0, button).clicked() {
                            state.asking = true;
                        }
                    }
                    true => {
                        ui.horizontal(|ui| {
                            let sure = egui::Button::new(
                                RichText::new("Forget them").font(theme::medium(12.0)).color(theme::text()),
                            )
                            .fill(theme::tint(theme::removed(), 90))
                            .min_size(egui::vec2(96.0, 24.0));
                            if ui.add(sure).clicked() {
                                state.asking = false;
                                act = Some(Act::Forget);
                            }
                            if ui
                                .add(egui::Button::new("Keep them").min_size(egui::vec2(88.0, 24.0)))
                                .clicked()
                            {
                                state.asking = false;
                            }
                        });
                    }
                }
                help(
                    ui,
                    &match (tokens, state.asking) {
                        (0, _) => "gitrust has no stored tokens to remove.".to_string(),
                        (count, true) => format!(
                            "{} will be taken out of the profiles, and they are written \
                             down nowhere else. The accounts and the keys stay.",
                            match count {
                                1 => "1 token".to_string(),
                                n => format!("{n} tokens"),
                            }
                        ),
                        (_, false) => "Takes the token out of every account of every profile. \
                                       Keys are files and are left alone; so is whatever Git's \
                                       credential helper has."
                            .to_string(),
                    },
                );
            });

            row(ui, "Detect conflicts with the target branch", |ui| {
                ui.checkbox(&mut general.foresee_conflicts, "");
                help(
                    ui,
                    "Tries the merge into the main branch of the remote ahead of time, in \
                     memory, and says in the bar under a repository when it would not go \
                     in cleanly. Nothing is touched to find out.",
                );
            });

            ui.add_space(24.0);
        });

    (*general != before, act)
}

/// One row: its name on the left, right-aligned against the controls so the
/// eye runs down one edge, and whatever it is on the right.
fn row(ui: &mut egui::Ui, name: &str, inside: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(NAMES, 22.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(RichText::new(name).font(theme::body(12.5)).color(theme::text()));
            },
        );
        ui.add_space(20.0);
        ui.vertical(|ui| {
            ui.set_max_width(PROSE);
            inside(ui);
        });
    });
    ui.add_space(16.0);
}

/// The sentence under a control saying what it does.
fn help(ui: &mut egui::Ui, text: &str) {
    ui.add_space(3.0);
    ui.label(RichText::new(text).font(theme::body(11.0)).color(theme::faint()));
}

/// The same, for the box that has something wrong in it.
fn complaint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(3.0);
    ui.label(RichText::new(text).font(theme::body(11.0)).color(theme::removed()));
}

/// A sentence about what a setting costs, behind a small triangle.
///
/// The triangle is drawn: the fonts this program ships have no warning sign
/// in them, and a missing-glyph box is a worse warning than none.
fn warning(ui: &mut egui::Ui, text: &str) {
    ui.add_space(3.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
        let colour = theme::muted();
        let (top, left, right) = (
            egui::pos2(rect.center().x, rect.top() + 2.5),
            egui::pos2(rect.left() + 0.5, rect.bottom() - 2.0),
            egui::pos2(rect.right() - 0.5, rect.bottom() - 2.0),
        );
        let painter = ui.painter();
        painter.add(egui::Shape::closed_line(vec![top, right, left], egui::Stroke::new(1.1, colour)));
        painter.line_segment(
            [egui::pos2(top.x, top.y + 3.6), egui::pos2(top.x, left.y - 3.4)],
            egui::Stroke::new(1.1, colour),
        );
        painter.circle_filled(egui::pos2(top.x, left.y - 1.8), 0.6, colour);
        ui.label(RichText::new(text).font(theme::body(11.0)).color(theme::muted()));
    });
}
