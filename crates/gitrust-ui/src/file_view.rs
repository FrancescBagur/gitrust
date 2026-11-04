//! One file, across the whole window.
//!
//! The diff panel on the right answers "what changed here" and is the right
//! width for it. The other three questions are not: blame wants a column for
//! the commit, one for who, one for when and then the line itself, and on a
//! panel three hundred pixels wide that is four things fighting over nothing.
//! So a file opened this way takes the middle of the window, where the graph
//! usually is.
//!
//! Like every view here it touches no repository. It is handed what was read
//! and gives back what was asked for.

use chrono::{Local, TimeZone};
use eframe::egui::{self, RichText};
use gitrust_core::{Commit, Contents, FileDiff, HunkId, Line, Oid, Story};

use crate::{diff_view, theme};

/// Which of the four questions is being asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// What this commit did to it.
    Diff,
    /// What the whole of it said.
    Whole,
    /// Who is answerable for each line.
    Blame,
    /// Everything that ever happened to it.
    Story,
}

impl Look {
    pub fn title(self) -> &'static str {
        match self {
            Look::Diff => "What changed",
            Look::Whole => "The file",
            Look::Blame => "Who wrote it",
            Look::Story => "Its history",
        }
    }
}

/// What has been asked for.
pub enum Act {
    /// Ask a different one of the four.
    Look(Look),
    /// Put it away and go back to the graph.
    Close,
    /// Go to that commit.
    Show(Oid),
    /// Look at this same file as it stood at that commit.
    At(Oid),
    /// Undo that piece of this commit, in the working tree.
    Revert(HunkId),
}

/// Everything read for the file being looked at.
///
/// Each of the four is read once, when it is first asked for, and kept. They
/// are `None` until then rather than empty: a file with no blame yet and a
/// file with no lines are not the same thing, and only one of them is worth
/// drawing a message about.
#[derive(Default)]
pub struct Read {
    pub diff: Option<FileDiff>,
    pub whole: Option<Contents>,
    pub blame: Option<Vec<Line>>,
    pub story: Option<Story>,
}

/// The file being looked at, and what has been read about it.
pub struct Looking {
    pub path: String,
    /// The commit it is being looked at from.
    pub at: Oid,
    pub how: Look,
    pub read: Read,
    /// What went wrong, when something did. Blame on something that is not
    /// text refuses, and that refusal belongs on the screen rather than in a
    /// status bar that the next thing overwrites.
    pub trouble: Option<String>,
}

pub fn show(ui: &mut egui::Ui, looking: &Looking, busy: bool) -> Option<Act> {
    let mut act = None;

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new("\u{2190} Back").font(theme::body(12.0)))
            .on_hover_text("back to the graph")
            .clicked()
        {
            act = Some(Act::Close);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(&looking.path)
                .font(theme::medium(13.5))
                .color(theme::TEXT),
        );
        ui.label(
            RichText::new(format!("at {}", short(looking.at)))
                .font(theme::mono(11.0))
                .color(theme::FAINT),
        );
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for how in [Look::Diff, Look::Whole, Look::Blame, Look::Story] {
            if ui.selectable_label(looking.how == how, how.title()).clicked() {
                act = Some(Act::Look(how));
            }
        }
    });
    theme::rule(ui);

    if let Some(trouble) = &looking.trouble {
        ui.add_space(10.0);
        ui.label(RichText::new(trouble).font(theme::body(12.0)).color(theme::REMOVED));
        return act;
    }

    egui::ScrollArea::vertical()
        .id_salt(("file", looking.how.title()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            match looking.how {
                Look::Diff => diff(ui, looking, busy, &mut act),
                Look::Whole => whole(ui, looking),
                Look::Blame => blame(ui, looking, &mut act),
                Look::Story => story(ui, looking, &mut act),
            }
            ui.add_space(20.0);
        });

    act
}

fn diff(ui: &mut egui::Ui, looking: &Looking, busy: bool, act: &mut Option<Act>) {
    let Some(diff) = &looking.read.diff else {
        return theme::empty(ui, "Working it out\u{2026}");
    };
    if diff.hunks.is_empty() {
        return theme::empty(ui, "This commit did not change this file");
    }

    for hunk in &diff.hunks {
        ui.horizontal(|ui| {
            diff_view::header(ui, hunk);
        });
        // Undoing one piece writes nothing: it puts the change back in the
        // files in front of you and leaves it there to look at.
        if !busy
            && ui
                .button(RichText::new("Undo this piece").font(theme::body(11.0)))
                .on_hover_text(
                    "puts this piece back in the working tree. \
                     Nothing in the history moves",
                )
                .clicked()
        {
            *act = Some(Act::Revert(hunk.id));
        }
        diff_view::lines(ui, hunk);
        ui.add_space(10.0);
    }
}

fn whole(ui: &mut egui::Ui, looking: &Looking) {
    match &looking.read.whole {
        None => theme::empty(ui, "Reading it\u{2026}"),
        Some(Contents::Absent) => {
            theme::empty(ui, "This file was not there yet at this commit")
        }
        Some(Contents::Binary(size)) => theme::empty(
            ui,
            &format!("Not text: {size} bytes of something else"),
        ),
        Some(Contents::Text(text)) => {
            // Painted as numbered lines rather than put in a text box: a box
            // would let somebody type into a file as it was in 2023, which
            // is not a thing that can be done.
            for (index, line) in text.lines().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add_sized(
                        [44.0, 15.0],
                        egui::Label::new(
                            RichText::new((index + 1).to_string())
                                .font(theme::mono(11.0))
                                .color(theme::FAINT),
                        ),
                    );
                    ui.label(RichText::new(line).font(theme::mono(11.5)).color(theme::TEXT));
                });
            }
        }
    }
}

fn blame(ui: &mut egui::Ui, looking: &Looking, act: &mut Option<Act>) {
    let Some(lines) = &looking.read.blame else {
        return theme::empty(ui, "Working out who wrote what\u{2026}");
    };
    if lines.is_empty() {
        return theme::empty(ui, "There is nothing in this file");
    }

    for line in lines {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Only the first line of a run says whose it is. Twenty lines
            // from one commit want saying once; repeating it turns the
            // left-hand side into noise you have to read past.
            let said = line.first;
            let hash = ui.add_sized(
                [52.0, 15.0],
                egui::Label::new(
                    RichText::new(match said {
                        true => line.short.clone(),
                        false => String::new(),
                    })
                    .font(theme::mono(10.5))
                    .color(theme::lane(0)),
                )
                .sense(egui::Sense::click()),
            );
            if said && !line.commit.is_zero() {
                let hash = hash.on_hover_text(format!(
                    "{}\n{}\n{}",
                    line.summary,
                    line.author,
                    when(line.time)
                ));
                if hash.clicked() {
                    *act = Some(Act::Show(line.commit));
                }
            }

            ui.add_sized(
                [120.0, 15.0],
                egui::Label::new(
                    RichText::new(match said {
                        // Nobody's yet: written since the last commit.
                        true if line.commit.is_zero() => "not committed".to_string(),
                        true => line.author.clone(),
                        false => String::new(),
                    })
                    .font(theme::body(10.5))
                    .color(match line.commit.is_zero() {
                        true => theme::ADDED,
                        false => theme::MUTED,
                    }),
                )
                .truncate(),
            );

            ui.add_sized(
                [40.0, 15.0],
                egui::Label::new(
                    RichText::new(line.number.to_string())
                        .font(theme::mono(10.5))
                        .color(theme::FAINT),
                ),
            );
            ui.label(
                RichText::new(&line.text)
                    .font(theme::mono(11.5))
                    .color(theme::TEXT),
            );
        });
    }
}

fn story(ui: &mut egui::Ui, looking: &Looking, act: &mut Option<Act>) {
    let Some(story) = &looking.read.story else {
        return theme::empty(ui, "Walking the history\u{2026}");
    };
    if story.commits.is_empty() {
        return theme::empty(ui, "Nothing in this history touched this file");
    }

    for commit in &story.commits {
        row(ui, commit, looking.at, act);
    }

    // A list that might be missing its oldest entries has to say so, or it
    // is a different answer wearing the same clothes.
    if story.cut_short {
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "The walk stopped before the beginning. Git keeps no history \
                 for a file, so this is found by reading commits one by one, \
                 and there is a limit on how many.",
            )
            .font(theme::body(11.0))
            .color(theme::FAINT),
        );
    }
}

fn row(ui: &mut egui::Ui, commit: &Commit, at: Oid, act: &mut Option<Act>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;

        let here = commit.id == at;
        let hash = ui.add_sized(
            [56.0, 17.0],
            egui::Label::new(
                RichText::new(&commit.short_id)
                    .font(theme::mono(11.0))
                    .color(match here {
                        true => theme::HEAD,
                        false => theme::lane(0),
                    }),
            )
            .sense(egui::Sense::click()),
        );
        if hash
            .on_hover_text("look at the file as it stood here")
            .clicked()
        {
            *act = Some(Act::At(commit.id));
        }

        let summary = ui.add(
            egui::Label::new(
                RichText::new(&commit.summary)
                    .font(theme::body(12.0))
                    .color(theme::TEXT),
            )
            .truncate()
            .sense(egui::Sense::click()),
        );
        if summary.on_hover_text("show this commit in the graph").clicked() {
            *act = Some(Act::Show(commit.id));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(when(commit.time))
                    .font(theme::mono(10.5))
                    .color(theme::FAINT),
            );
            ui.label(
                RichText::new(&commit.author)
                    .font(theme::body(11.0))
                    .color(theme::MUTED),
            );
        });
    });
    ui.add_space(3.0);
}

fn when(seconds: i64) -> String {
    match Local.timestamp_opt(seconds, 0).single() {
        Some(time) => time.format("%d/%m/%Y").to_string(),
        None => String::new(),
    }
}

fn short(oid: Oid) -> String {
    oid.to_string().chars().take(7).collect()
}
