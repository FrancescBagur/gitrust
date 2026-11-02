//! The board: the branches laid out in columns by where they stand.
//!
//! Almost nothing here is stored. The four columns on the right come straight
//! off the graph, so they cannot fall out of date: the moment you commit, the
//! card changes column on its own. The one thing the graph cannot know is
//! whether anybody is looking at a branch, so the forge is asked that, and its
//! answer arrives later and moves the cards again.
//!
//! The backlog is the exception, and it is kept at the far left where the
//! reading starts. Work nobody has begun leaves no trace in a repository, so
//! it is the one column that has to be written down. What keeps it from
//! rotting is that you cannot do much with an item except start it, and
//! starting it turns it into a branch, which is read off the graph like
//! everything else.

use chrono::{Local, TimeZone};
use eframe::egui::{self, Color32, RichText, Stroke};
use gitrust_core::{Backlog, Board, Card, Item, Lane, PullRequestState, Worktree};

use crate::refs_view::Pick;
use crate::theme;

/// What is in the air right now.
///
/// One type for both kinds, because a column can only take one kind of
/// payload, and "To do" has to be able to take either: a branch dragged back
/// from somewhere, or an item being started.
#[derive(Clone)]
enum Dragged {
    Card {
        branch: String,
        lane: Lane,
        /// Whether it already has a pull request, open or otherwise.
        reviewed: bool,
    },
    Item {
        id: String,
    },
}

/// How the asking of the forge is going, as far as the header is concerned.
#[derive(Clone, Copy)]
pub struct Forge<'a> {
    pub asking: bool,
    /// Why it could not answer, when it could not.
    pub note: Option<&'a str>,
}

/// The backlog, and what is being typed into it.
///
/// It is passed in rather than read here because it is the one thing on the
/// board that is stored, and reading it means opening the repository.
pub struct Waiting<'a> {
    pub backlog: &'a Backlog,
    pub draft: &'a mut String,
}

/// What has been asked for from the board.
pub enum Act {
    /// The same as from the sidebar: see a commit, or switch branch.
    Pick(Pick),
    /// Write something down in the backlog.
    Note(String),
    /// Make that item's branch and take it out of the backlog.
    Start(String),
    /// Take it out without starting it.
    Drop(String),
    /// Send the backlog to the remote.
    Share,
    /// Go into that branch's own board.
    Enter(String),
    /// Go back out to that step of the trail.
    Up(usize),
    /// Open that branch in a folder of its own.
    Isolate(String),
    /// Look at the checkout in that folder.
    Visit(std::path::PathBuf),
    /// Take that checkout away.
    Release(String),
    /// Push that branch to the remote.
    Publish(String),
    /// Open a pull request for it.
    Review(String),
    /// Bring it into the main branch.
    Land(String),
    /// Ask the forge again.
    Reload,
    /// Open that address in a browser.
    Open(String),
}

/// Whether dropping something into a column has an operation behind it.
///
/// Only three moves do, and all for the same reason: they are the only ones
/// where you can name the command that would do what the drag promises. An
/// item becomes a branch, which is `git branch`. A branch with no commits
/// cannot be published, and you do not come back from "done" by dragging: that
/// would undo a merge, which is another thing, and graver.
///
/// Which command the review one is depends on who was asked. Unasked, a card
/// reaches "in review" by being pushed, so the drop pushes. Asked, it gets
/// there by having a pull request, so the drop opens one — and a branch that
/// already has one is left alone, because turning a draft into a review is
/// `gh pr ready`, a different thing that deserves its own place.
fn allows(dragged: &Dragged, to: Lane, asked: bool) -> bool {
    match dragged {
        Dragged::Item { .. } => to == Lane::Todo,
        Dragged::Card { lane, reviewed, .. } => match (lane, to) {
            (Lane::InProgress, Lane::InReview) => !(asked && *reviewed),
            (Lane::InProgress, Lane::Done) | (Lane::InReview, Lane::Done) => true,
            _ => false,
        },
    }
}

fn transition(dragged: &Dragged, to: Lane, asked: bool) -> Option<Act> {
    if !allows(dragged, to, asked) {
        return None;
    }
    let branch = match dragged {
        Dragged::Item { id } => return Some(Act::Start(id.clone())),
        Dragged::Card { branch, .. } => branch.clone(),
    };
    Some(match (to, asked) {
        (Lane::InReview, true) => Act::Review(branch),
        (Lane::InReview, false) => Act::Publish(branch),
        _ => Act::Land(branch),
    })
}

/// Each column's label and the color of its rule.
///
/// The three colors come from the graph's lanes, which is where all the
/// saturated color in the window comes from. Amber is not among them: it
/// belongs to HEAD, and here it would fight the branch you are on, which
/// already wears it on its card.
fn column(lane: Lane) -> (&'static str, Color32) {
    match lane {
        Lane::Todo => ("To do", theme::MUTED),
        Lane::InProgress => ("In progress", theme::lane(0)),
        Lane::InReview => ("In review", theme::lane(6)),
        Lane::Done => ("Done", theme::lane(1)),
    }
}

/// Gives back whatever has been asked about a branch on the board.
///
/// `trail` is where you are: the trunk first, then every branch you went into.
/// It is what makes this the same function at every depth, because going into
/// a card does not open another screen, it measures this one against another
/// branch.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    board: &Board,
    project: &mut Option<String>,
    busy: bool,
    forge: Forge<'_>,
    waiting: Waiting<'_>,
    trail: &[String],
    trees: &[Worktree],
) -> Option<Act> {
    let mut picked = None;
    let inside = trail.len() > 1;

    ui.add_space(10.0);
    let (step, again) = trail_line(ui, board, forge, trail);
    if again {
        picked = Some(Act::Reload);
    }
    if let Some(step) = step {
        picked = Some(Act::Up(step));
    }

    // The way out has to be drawn before anything can be empty, or going into
    // a branch with nothing under it would be going somewhere with no door.
    if board.cards.is_empty() && (inside || waiting.backlog.items.is_empty()) {
        let said = match inside {
            true => "Nothing came out of this branch yet: make a branch from it and it will show up here",
            false => "Only the main branch so far: make a branch, or write something down in the backlog",
        };
        theme::empty(ui, said);
        return picked;
    }

    // The projects are the folders in the branch names: `theme/colors` and
    // `theme/typography` are the same job, and people already name them so. An
    // item waiting in the backlog counts as well: its folder is a project
    // before it is a branch.
    let mut projects = board.projects();
    for name in waiting.backlog.items.iter().filter_map(|item| item.project.as_deref()) {
        if !inside && !projects.contains(&name) {
            projects.push(name);
        }
    }
    projects.sort_unstable();
    if !projects.is_empty() {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            ui.spacing_mut().item_spacing.x = 4.0;

            if ui.selectable_label(project.is_none(), chip("All", project.is_none())).clicked() {
                *project = None;
            }
            for name in projects {
                let on = project.as_deref() == Some(name);
                if ui.selectable_label(on, chip(name, on)).clicked() {
                    *project = (!on).then(|| name.to_string());
                }
            }
        });
    }

    theme::rule(ui);

    let filter = project.clone();
    let asked = board.asked;
    let Waiting { backlog, draft } = waiting;

    // The backlog goes first because that is where the reading starts, and
    // because the work really does flow that way. Inside a branch it is not
    // there at all: the backlog is the repository's, not this branch's, and
    // repeating it at every depth would be saying the same thing over and over
    // in places where it answers nothing.
    let columns = match inside {
        true => Lane::ALL.len(),
        false => 1 + Lane::ALL.len(),
    };
    ui.columns(columns, |columns| {
        let (first, rest) = columns.split_at_mut(usize::from(!inside));
        if let Some(ui) = first.first_mut() {
            if let Some(act) = backlog_column(ui, backlog, draft, filter.as_deref()) {
                picked = Some(act);
            }
        }

        for (ui, lane) in rest.iter_mut().zip(Lane::ALL) {
            let cards: Vec<&Card> = board
                .lane(lane)
                .filter(|card| match &filter {
                    Some(name) => card.project.as_deref() == Some(name.as_str()),
                    None => true,
                })
                .collect();

            let (title, color) = column(lane);
            header(ui, title, color, cards.len());

            // The whole column takes whatever is dropped on it.
            let mut dropped = None;
            ui.scope(|ui| {
                paint_drop_zone(ui, lane, board.asked);

                let frame = egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(2, 4))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS));

                let (_, payload) = ui.dnd_drop_zone::<Dragged, _>(frame, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(column(lane).0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if cards.is_empty() {
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new("Nothing here")
                                        .font(theme::body(11.5))
                                        .color(theme::FAINT),
                                );
                                return;
                            }
                            for card in cards {
                                if let Some(act) = draw_card(ui, card, busy, asked, trees) {
                                    picked = Some(act);
                                }
                                ui.add_space(6.0);
                            }
                        });
                });
                dropped = payload;
            });

            if let Some(dragged) = dropped {
                if let Some(act) = transition(&dragged, lane, asked) {
                    picked = Some(act);
                }
            }
        }
    });

    picked
}

/// What the columns are measured against, and who was asked.
///
/// The second half of this line is the difference between a board that knows
/// and a board that is guessing, and that difference has to be readable
/// without opening anything. "In review means pushed" is the guess said out
/// loud; the reason the forge could not be reached waits in the hover, where
/// it is there for whoever wants it and in nobody's way otherwise.
///
/// Gives back the step of the trail clicked, and whether the forge should be
/// asked again.
fn trail_line(
    ui: &mut egui::Ui,
    board: &Board,
    forge: Forge<'_>,
    trail: &[String],
) -> (Option<usize>, bool) {
    let mut again = false;
    let mut step = None;

    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(RichText::new("Measured against").font(theme::body(11.5)).color(theme::FAINT));

        // Every step but the last is a board that is already there, so it is a
        // link and not a button: clicking one does nothing to the repository,
        // it looks somewhere else.
        let last = trail.len().saturating_sub(1);
        ui.spacing_mut().item_spacing.x = 3.0;
        for (depth, name) in trail.iter().enumerate() {
            if depth > 0 {
                ui.label(RichText::new("›").font(theme::body(11.5)).color(theme::FAINT));
            }

            let leaf = name.rsplit_once('/').map_or(name.as_str(), |(_, leaf)| leaf);
            if depth == last {
                ui.label(RichText::new(leaf).font(theme::medium(11.5)).color(theme::TEXT));
                continue;
            }

            let label = egui::Label::new(
                RichText::new(leaf).font(theme::body(11.5)).color(theme::MUTED),
            );
            if ui
                .add(label.sense(egui::Sense::click()))
                .on_hover_text(format!("Back out to {name}"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                step = Some(depth);
            }
        }
        ui.spacing_mut().item_spacing.x = 5.0;

        let reviews = board.cards.iter().filter(|card| card.pull.is_some()).count();
        let (said, hint) = match (forge.asking, board.asked, forge.note) {
            (true, _, _) => ("· asking the forge…".to_string(), None),
            (_, true, _) => (
                format!("· {}", theme::count_label(reviews, "pull request")),
                Some("Click to ask the forge again".to_string()),
            ),
            (_, _, Some(note)) => (
                "· in review means pushed".to_string(),
                Some(format!("{note}\n\nClick to try the forge again")),
            ),
            _ => (String::new(), None),
        };

        if said.is_empty() {
            return;
        }

        let label = egui::Label::new(RichText::new(said).font(theme::body(11.5)).color(theme::FAINT));
        let response = match hint {
            Some(hint) => {
                let response = ui.add(label.sense(egui::Sense::click()));
                response.on_hover_text(hint)
            }
            None => ui.add(label),
        };
        again = response.clicked();
    });

    (step, again)
}

fn chip(text: &str, on: bool) -> RichText {
    RichText::new(text)
        .font(theme::body(11.5))
        .color(if on { theme::TEXT } else { theme::MUTED })
}

/// A column's label, with its count and its rule of color.
fn header(ui: &mut egui::Ui, title: &str, color: Color32, count: usize) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(RichText::new(title).font(theme::medium(12.0)).color(color));
        ui.label(
            RichText::new(count.to_string()).font(theme::mono(10.5)).color(theme::FAINT),
        );
    });
    ui.add_space(4.0);
    rule_of(ui, color);
}

/// The line of color under a column's label.
fn rule_of(ui: &mut egui::Ui, color: Color32) {
    let width = ui.available_width() - 8.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 2.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, theme::tint(color, 110));
    ui.add_space(8.0);
}

/// The one column that is written rather than worked out.
///
/// It takes nothing dropped on it. You leave the backlog by starting
/// something; you do not come back to it by dragging a branch here, because
/// that would mean deleting the branch, which is a different thing and said
/// somewhere it can be read.
fn backlog_column(
    ui: &mut egui::Ui,
    backlog: &Backlog,
    draft: &mut String,
    project: Option<&str>,
) -> Option<Act> {
    let mut picked = None;

    let items: Vec<&Item> = backlog
        .items
        .iter()
        .filter(|item| match project {
            Some(name) => item.project.as_deref() == Some(name),
            None => true,
        })
        .collect();

    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(RichText::new("Backlog").font(theme::medium(12.0)).color(theme::FAINT));
        ui.label(
            RichText::new(items.len().to_string()).font(theme::mono(10.5)).color(theme::FAINT),
        );

        // What has not gone out yet is said here rather than left to be
        // discovered, in the same quiet voice the board uses to admit when it
        // is guessing. Amber would have been louder and would have been wrong:
        // it is the branch you are on, and it says so everywhere else.
        if backlog.unshared > 0 {
            let label = egui::Label::new(
                RichText::new(format!("· {} not shared", backlog.unshared))
                    .font(theme::body(11.0))
                    .color(theme::FAINT),
            );
            let response = ui
                .add(label.sense(egui::Sense::click()))
                .on_hover_text(
                    "The whole backlog has that many nobody else has yet, \
                     the ones a project filter is hiding included.\n\n\
                     Click to send it to the remote",
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                picked = Some(Act::Share);
            }
        }
    });
    ui.add_space(4.0);
    rule_of(ui, theme::FAINT);

    // With a project picked, what is written down belongs to it: the chip is
    // already saying which job this is, so saying it twice would be a field
    // nobody needs to fill.
    let hint = match project {
        Some(name) => format!("Something to do in {name}"),
        None => "Something to do".to_string(),
    };
    let typing = ui.add(
        egui::TextEdit::singleline(draft)
            .hint_text(RichText::new(hint).font(theme::body(11.5)).color(theme::FAINT))
            .font(theme::body(12.0))
            .desired_width(f32::INFINITY),
    );
    if typing.lost_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter))
        && !draft.trim().is_empty()
    {
        picked = Some(Act::Note(std::mem::take(draft)));
    }
    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .id_salt("Backlog")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if items.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Nothing waiting").font(theme::body(11.5)).color(theme::FAINT),
                );
                return;
            }
            for item in items {
                if let Some(act) = draw_item(ui, item) {
                    picked = Some(act);
                }
                ui.add_space(6.0);
            }
        });

    picked
}

/// An item: what it says, and the branch it would turn into.
///
/// The branch name is on the card from the start so that starting one is never
/// a surprise. It is worked out from the title, so changing the name means
/// saying the thing differently, which is the right way round: the title is
/// what somebody will read.
fn draw_item(ui: &mut egui::Ui, item: &Item) -> Option<Act> {
    let frame = egui::Frame::NONE
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .stroke(Stroke::new(1.0, theme::HAIRLINE));

    let id = egui::Id::new(("item", &item.id));
    let dragging = ui.ctx().is_being_dragged(id);

    let inner = ui.dnd_drag_source(id, Dragged::Item { id: item.id.clone() }, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(RichText::new(&item.title).font(theme::medium(12.5)).color(theme::TEXT));
            ui.label(RichText::new(item.branch()).font(theme::mono(10.5)).color(theme::FAINT));
        });
    });

    if dragging {
        return None;
    }

    let response = inner.response.interact(egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            egui::CornerRadius::same(theme::RADIUS),
            Stroke::new(1.0, theme::EDGE),
            egui::StrokeKind::Inside,
        );
    }

    // Dropping an item lives in the right-click menu rather than on a button,
    // which on a card you drag would be fighting the drag for the same press.
    let mut dropped = None;
    response.context_menu(|ui| {
        if ui.button("Drop it").clicked() {
            dropped = Some(Act::Drop(item.id.clone()));
            ui.close_menu();
        }
    });
    if dropped.is_some() {
        return dropped;
    }

    let mut hint = "Drag it to To do and it becomes a branch".to_string();
    if !item.by.is_empty() {
        hint.push_str(&format!("\n\nWritten down by {}", item.by));
    }
    if let Some(when) = ago(item.added) {
        hint.push_str(&format!(" · {when}"));
    }
    response.on_hover_text(hint);
    None
}

/// Leaves the drop zone in the column's color, but only if what is being
/// dragged can land there. A column that lights up for anything is not saying
/// anything.
fn paint_drop_zone(ui: &mut egui::Ui, lane: Lane, asked: bool) {
    let accepts = egui::DragAndDrop::payload::<Dragged>(ui.ctx())
        .is_some_and(|dragged| allows(&dragged, lane, asked));
    let color = column(lane).1;

    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.active.bg_fill = if accepts { theme::tint(color, 20) } else { Color32::TRANSPARENT };
    widgets.active.bg_stroke = if accepts {
        Stroke::new(1.0, color)
    } else {
        Stroke::NONE
    };
}

/// A card: the branch, which project it is from, how much it carries and how old it is.
fn draw_card(
    ui: &mut egui::Ui,
    card: &Card,
    busy: bool,
    asked: bool,
    trees: &[Worktree],
) -> Option<Act> {
    let open_at = open_at(trees, &card.branch);
    let frame = egui::Frame::NONE
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .stroke(Stroke::new(1.0, theme::HAIRLINE));

    let id = egui::Id::new(("card", &card.branch));
    let dragging = ui.ctx().is_being_dragged(id);
    let payload = Dragged::Card {
        branch: card.branch.clone(),
        lane: card.lane,
        reviewed: card.pull.is_some(),
    };

    let mut badges = Badges::default();

    let inner = ui.dnd_drag_source(id, payload.clone(), |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;

            // The badges are laid out before the name, so they take the room
            // they need and the name takes what is left. The other way round
            // a long branch name ran straight over the top of them.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // What is true about the card, rather than its name, goes
                    // to the far side where the eye is not reading. The way in
                    // is the outermost of them, because it is the one you
                    // reach for.
                    if card.children > 0 {
                        badges.subtasks = Some(subtask_badge(ui, card.children));
                    }
                    if let Some(pull) = &card.pull {
                        badges.review = review_badge(ui, pull);
                    }
                    if let Some(tree) = open_at {
                        badges.folder = Some(folder_badge(ui, tree));
                    }

                    // The branch you are on wears HEAD's amber, as in the graph
                    // and in the bar: the same question answered in the same
                    // color. Cut short rather than wrapped, because a card
                    // whose height depends on its name makes a ragged column.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(card.leaf())
                                    .font(theme::medium(12.5))
                                    .color(match card.current {
                                        true => theme::HEAD,
                                        false => theme::TEXT,
                                    }),
                            )
                            .truncate(),
                        );
                    });
                });
            });
            ui.label(RichText::new(note(card)).font(theme::body(11.0)).color(theme::MUTED));
        });
    });

    let response = inner.response.interact(egui::Sense::click());

    // While it is in the air the card is painted on another layer and shifted,
    // so anything added here on top would land in the hole it left.
    if dragging {
        return None;
    }

    // The drag covers the whole card, so a badge cannot change the cursor
    // from inside: it is changed out here, where the drag has had its say.
    if let Some(pos) = response.hover_pos() {
        if badges.anywhere(pos) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    // The highlight goes on the border, not the fill: a fill on top would
    // cover what the card says.
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            egui::CornerRadius::same(theme::RADIUS),
            Stroke::new(1.0, theme::EDGE),
            egui::StrokeKind::Inside,
        );
    }
    if card.current {
        let bar = egui::Rect::from_min_size(
            response.rect.min,
            egui::vec2(2.0, response.rect.height()),
        );
        ui.painter().rect_filled(bar, 0.0, theme::HEAD);
    }

    // What it can be done to lives here rather than on the card, because a
    // card you drag has no room for buttons that would fight the drag for the
    // same press.
    let mut asked_for = None;
    response.context_menu(|ui| {
        // Said in words as well as drawn, because the badge is small and the
        // board inside a card is easy to not know about at all.
        if card.children > 0 {
            let says = match card.children {
                1 => "Go into its subtask".to_string(),
                n => format!("Go into its {n} subtasks"),
            };
            if ui.button(says).clicked() {
                asked_for = Some(Act::Enter(card.branch.clone()));
                ui.close_menu();
            }
        }

        match open_at {
            Some(tree) => {
                if ui.button("Go to its folder").clicked() {
                    asked_for = Some(Act::Visit(tree.path.clone()));
                    ui.close_menu();
                }
                if ui.button("Close its folder").clicked() {
                    asked_for = Some(Act::Release(tree.name.clone()));
                    ui.close_menu();
                }
            }
            None if !card.current => {
                if ui
                    .button("Open in its own folder")
                    .on_hover_text("a second checkout, beside the repository, so this branch can be worked on without leaving where you are")
                    .clicked()
                {
                    asked_for = Some(Act::Isolate(card.branch.clone()));
                    ui.close_menu();
                }
            }
            // The branch you are on is already open: you are standing in it.
            None => {}
        }
    });
    if asked_for.is_some() {
        return asked_for;
    }

    // A branch open in another folder cannot be switched to here, so the
    // double click goes there instead. It is the same intention either way:
    // take me to that branch.
    let switchable = !card.current && !busy;
    let draggable = allows(&payload, Lane::Done, asked);
    let hint = match (switchable, open_at, draggable) {
        (true, Some(tree), _) => format!(
            "{}\nOpen in {}\nDouble-click to go there",
            card.branch,
            tree.path.display()
        ),
        (true, None, true) => format!(
            "{}\nDouble-click to switch to it, or drag it to another column",
            card.branch
        ),
        (true, None, false) => format!("{}\nDouble-click to switch to this branch", card.branch),
        _ => card.branch.clone(),
    };
    // The badges are drawn inside the drag source and cannot be hovered on
    // their own, so everything there is to say about them is said here.
    let hint = match card.children {
        0 => hint,
        1 => format!("{hint}\nOne branch came out of this one: the › badge goes in"),
        n => format!("{hint}\n{n} branches came out of this one: the › badge goes in"),
    };
    let hint = match &card.pull {
        Some(pull) if !pull.url.is_empty() => {
            format!("{hint}\nPull request #{}: the number opens it", pull.number)
        }
        _ => hint,
    };
    let response = response.on_hover_text(hint);

    if switchable && response.double_clicked() {
        return Some(match open_at {
            Some(tree) => Act::Visit(tree.path.clone()),
            None => Act::Pick(Pick::Switch { name: card.branch.clone(), remote: false }),
        });
    }
    if response.clicked() {
        let landed = response.interact_pointer_pos().map(|pos| badges.at(pos));
        match landed {
            Some(Hit::Subtasks) => return Some(Act::Enter(card.branch.clone())),
            Some(Hit::Review) => {
                if let Some(pull) = &card.pull {
                    return Some(Act::Open(pull.url.clone()));
                }
            }
            Some(Hit::Folder) => {
                if let Some(tree) = open_at {
                    return Some(Act::Visit(tree.path.clone()));
                }
            }
            _ => {}
        }
        return Some(Act::Pick(Pick::Show(card.target)));
    }

    None
}

/// Where a card's badges ended up.
///
/// A card is dragged as a whole, so the drag source covers everything drawn
/// inside it: nothing in there can be a control of its own, and for a while
/// none of these three did anything at all. They are places on the card
/// instead, and what was pressed is worked out from where the press landed.
#[derive(Default)]
struct Badges {
    subtasks: Option<egui::Rect>,
    review: Option<egui::Rect>,
    folder: Option<egui::Rect>,
}

/// What a press landed on. The card itself is the answer for most of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Subtasks,
    Review,
    Folder,
    Card,
}

impl Badges {
    /// Asked in the order they are drawn, outermost first. They do not
    /// overlap, so the order decides nothing except which one answers when
    /// two of them meet along an edge.
    fn at(&self, pos: egui::Pos2) -> Hit {
        if self.subtasks.is_some_and(|rect| rect.contains(pos)) {
            return Hit::Subtasks;
        }
        if self.review.is_some_and(|rect| rect.contains(pos)) {
            return Hit::Review;
        }
        if self.folder.is_some_and(|rect| rect.contains(pos)) {
            return Hit::Folder;
        }
        Hit::Card
    }

    fn anywhere(&self, pos: egui::Pos2) -> bool {
        self.at(pos) != Hit::Card
    }
}

/// The checkout a branch is open in, when it is open in one of its own.
fn open_at<'a>(trees: &'a [Worktree], branch: &str) -> Option<&'a Worktree> {
    trees
        .iter()
        .find(|tree| !tree.main && tree.branch.as_deref() == Some(branch))
}

/// The folder this branch is open in.
///
/// A branch can only be checked out in one place, so this is not decoration:
/// it is the reason the card cannot be switched to from here, said before you
/// try rather than after.
fn folder_badge(ui: &mut egui::Ui, tree: &Worktree) -> egui::Rect {
    let name = match tree.name.len() > 14 {
        true => format!("{}…", &tree.name[..13]),
        false => tree.name.clone(),
    };

    let label = egui::Label::new(
        RichText::new(name).font(theme::mono(10.5)).color(theme::FAINT),
    );
    ui.add(label).rect
}

/// How many branches came out of this one, and the way into them.
///
/// It is on the card because a branch is on one board only, its parent's.
/// Without a number here, work could sit somewhere nobody thought to look.
fn subtask_badge(ui: &mut egui::Ui, count: usize) -> egui::Rect {
    // It wears a pill rather than going bare. It is the only way into the
    // board inside this card, and as plain text nobody read it as a way into
    // anything. A label and not a button: a button would fight the drag for
    // the same press, which is why nothing on a card is one.
    let label = egui::Label::new(
        RichText::new(format!("{count} ›")).font(theme::mono(10.5)).color(theme::TEXT),
    );

    egui::Frame::NONE
        .fill(theme::RAISED)
        .stroke(Stroke::new(1.0, theme::EDGE))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| ui.add(label))
        .response
        .rect
}

/// The pull request's number, in the color of the column it argues for.
///
/// A draft and a closed one go muted: they are both pull requests that are not
/// asking anybody for anything, and the card they sit on is not in review.
/// Gives back whether it was clicked, which means open it in a browser.
fn review_badge(ui: &mut egui::Ui, pull: &gitrust_core::PullRequest) -> Option<egui::Rect> {
    let (color, _says) = match (pull.state, pull.draft) {
        (PullRequestState::Merged, _) => (theme::lane(1), "merged"),
        (PullRequestState::Open, false) => (theme::lane(6), "open"),
        (PullRequestState::Open, true) => (theme::MUTED, "a draft"),
        (PullRequestState::Closed, _) => (theme::MUTED, "closed, not merged"),
    };

    let label = egui::Label::new(
        RichText::new(format!("#{}", pull.number))
            .font(theme::mono(10.5))
            .color(color),
    );

    let rect = ui.add(label).rect;

    // Without a URL there is nowhere to go, so it does not pretend to be a
    // link: it stays a number that says which pull request this is.
    match pull.url.is_empty() {
        true => None,
        false => Some(rect),
    }
}

/// "theme - 3 commits - 2 h ago", saying nothing of what is not there.
fn note(card: &Card) -> String {
    let mut parts = Vec::new();

    if let Some(project) = &card.project {
        parts.push(project.clone());
    }

    // With nothing of its own, the date is that of the commit the branch was
    // cut from, which says nothing about the branch. It keeps quiet and says
    // the one thing it does know.
    if card.lane == Lane::Todo {
        parts.push("not started".to_string());
        return parts.join(" · ");
    }

    match card.ahead {
        0 => {}
        1 => parts.push("1 commit".to_string()),
        n => parts.push(format!("{n} commits")),
    }
    if let Some(when) = ago(card.time) {
        parts.push(when);
    }

    parts.join(" · ")
}

/// How long ago, rounded to what one would say out loud.
fn ago(seconds: i64) -> Option<String> {
    let when = Local.timestamp_opt(seconds, 0).single()?;
    let elapsed = Local::now().signed_duration_since(when);

    let minutes = elapsed.num_minutes();
    if minutes < 1 {
        return Some("just now".to_string());
    }
    if minutes < 60 {
        return Some(format!("{minutes} min ago"));
    }

    let hours = elapsed.num_hours();
    if hours < 24 {
        return Some(format!("{hours} h ago"));
    }

    let days = elapsed.num_days();
    match days {
        0..=1 => Some("yesterday".to_string()),
        2..=30 => Some(format!("{days} days ago")),
        _ => Some(when.format("%d/%m/%Y").to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(lane: Lane, reviewed: bool) -> Dragged {
        Dragged::Card {
            branch: "fix/footer".to_string(),
            lane,
            reviewed,
        }
    }

    fn badges() -> Badges {
        // Laid out right to left along the top of a card, as they are drawn.
        Badges {
            subtasks: Some(egui::Rect::from_min_max(
                egui::pos2(180.0, 10.0),
                egui::pos2(210.0, 26.0),
            )),
            review: Some(egui::Rect::from_min_max(
                egui::pos2(140.0, 10.0),
                egui::pos2(176.0, 26.0),
            )),
            folder: Some(egui::Rect::from_min_max(
                egui::pos2(90.0, 10.0),
                egui::pos2(136.0, 26.0),
            )),
        }
    }

    #[test]
    fn a_press_on_a_badge_is_a_press_on_that_badge_and_not_on_the_card() {
        let badges = badges();
        assert_eq!(badges.at(egui::pos2(195.0, 18.0)), Hit::Subtasks);
        assert_eq!(badges.at(egui::pos2(158.0, 18.0)), Hit::Review);
        assert_eq!(badges.at(egui::pos2(110.0, 18.0)), Hit::Folder);
    }

    #[test]
    fn a_press_anywhere_else_on_the_card_is_a_press_on_the_card() {
        let badges = badges();
        // The name, to the left of them all.
        assert_eq!(badges.at(egui::pos2(20.0, 18.0)), Hit::Card);
        // The line underneath.
        assert_eq!(badges.at(egui::pos2(195.0, 40.0)), Hit::Card);
        // The gaps between them, which belong to nobody.
        assert_eq!(badges.at(egui::pos2(178.0, 18.0)), Hit::Card);
        assert!(!badges.anywhere(egui::pos2(20.0, 18.0)));
    }

    #[test]
    fn a_card_without_badges_answers_the_card_wherever_it_is_pressed() {
        let bare = Badges::default();
        for pos in [egui::pos2(0.0, 0.0), egui::pos2(195.0, 18.0), egui::pos2(999.0, 999.0)] {
            assert_eq!(bare.at(pos), Hit::Card, "{pos:?}");
            assert!(!bare.anywhere(pos));
        }
    }

    /// A pull request with no address is a number and nothing more, so there
    /// is no rectangle for a press to land on.
    #[test]
    fn a_pull_request_with_nowhere_to_go_is_not_somewhere_to_press() {
        let badges = Badges { review: None, ..badges() };
        assert_eq!(badges.at(egui::pos2(158.0, 18.0)), Hit::Card);
    }

    fn item() -> Dragged {
        Dragged::Item {
            id: "abc123".to_string(),
        }
    }

    #[test]
    fn an_item_leaves_the_backlog_by_becoming_a_branch_and_no_other_way() {
        assert!(matches!(
            transition(&item(), Lane::Todo, true),
            Some(Act::Start(_))
        ));
        for lane in [Lane::InProgress, Lane::InReview, Lane::Done] {
            assert!(transition(&item(), lane, true).is_none(), "{lane:?}");
        }
    }

    #[test]
    fn unasked_a_card_reaches_review_by_being_pushed_and_asked_by_a_pull_request() {
        assert!(matches!(
            transition(&card(Lane::InProgress, false), Lane::InReview, false),
            Some(Act::Publish(_))
        ));
        assert!(matches!(
            transition(&card(Lane::InProgress, false), Lane::InReview, true),
            Some(Act::Review(_))
        ));
    }

    #[test]
    fn a_branch_that_already_has_a_pull_request_is_left_alone() {
        assert!(transition(&card(Lane::InProgress, true), Lane::InReview, true).is_none());
        // Unasked there is nothing known about pull requests, so pushing it
        // again is still a thing that can be named and done.
        assert!(transition(&card(Lane::InProgress, true), Lane::InReview, false).is_some());
    }

    #[test]
    fn nothing_comes_back_out_of_done_by_dragging() {
        for from in [Lane::Todo, Lane::InProgress, Lane::InReview, Lane::Done] {
            for to in [Lane::Todo, Lane::InProgress] {
                assert!(transition(&card(from, false), to, true).is_none(), "{from:?} to {to:?}");
            }
        }
        assert!(transition(&card(Lane::Done, false), Lane::InReview, true).is_none());
    }

    #[test]
    fn a_branch_with_nothing_of_its_own_cannot_be_published() {
        assert!(transition(&card(Lane::Todo, false), Lane::InReview, false).is_none());
        assert!(transition(&card(Lane::Todo, false), Lane::Done, true).is_none());
    }
}
