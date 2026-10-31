//! The board: the branches laid out in columns by where they stand.
//!
//! Nothing here is stored. What you see comes straight off the graph, so it
//! cannot fall out of date: the moment you commit, the card changes column on
//! its own.

use chrono::{Local, TimeZone};
use eframe::egui::{self, Color32, RichText, Stroke};
use gitrust_core::{Board, Card, Lane};

use crate::refs_view::Pick;
use crate::theme;

/// The card being dragged right now.
#[derive(Clone)]
struct Dragged {
    branch: String,
    lane: Lane,
}

/// What has been asked for from the board.
pub enum Act {
    /// The same as from the sidebar: see a commit, or switch branch.
    Pick(Pick),
    /// Push that branch to the remote.
    Publish(String),
    /// Bring it into the main branch.
    Land(String),
}

/// Whether dropping a card from one column into another has an operation behind it.
///
/// Only two moves do, and both for the same reason: they are the only ones
/// where you can name the command that would do what the drag promises. A
/// branch with no commits cannot be published, and you do not come back from
/// "done" by dragging: that would undo a merge, which is another thing, and graver.
fn allows(from: Lane, to: Lane) -> bool {
    matches!(
        (from, to),
        (Lane::InProgress, Lane::InReview)
            | (Lane::InProgress, Lane::Done)
            | (Lane::InReview, Lane::Done)
    )
}

fn transition(dragged: &Dragged, to: Lane) -> Option<Act> {
    if !allows(dragged.lane, to) {
        return None;
    }
    Some(match to {
        Lane::InReview => Act::Publish(dragged.branch.clone()),
        _ => Act::Land(dragged.branch.clone()),
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
pub fn show(
    ui: &mut egui::Ui,
    board: &Board,
    project: &mut Option<String>,
    busy: bool,
) -> Option<Act> {
    if board.cards.is_empty() {
        theme::empty(ui, "Only the main branch so far: make a branch and it will show up here");
        return None;
    }

    let mut picked = None;

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Measured against {}", board.trunk))
                .font(theme::body(11.5))
                .color(theme::FAINT),
        );
    });

    // The projects are the folders in the branch names: `theme/colors` and
    // `theme/typography` are the same job, and people already name them so.
    let projects = board.projects();
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
    ui.columns(Lane::ALL.len(), |columns| {
        for (ui, lane) in columns.iter_mut().zip(Lane::ALL) {
            let cards: Vec<&Card> = board
                .lane(lane)
                .filter(|card| match &filter {
                    Some(name) => card.project.as_deref() == Some(name.as_str()),
                    None => true,
                })
                .collect();

            header(ui, lane, cards.len());

            // The whole column takes whatever is dropped on it.
            let mut dropped = None;
            ui.scope(|ui| {
                paint_drop_zone(ui, lane);

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
                                if let Some(act) = draw_card(ui, card, busy) {
                                    picked = Some(act);
                                }
                                ui.add_space(6.0);
                            }
                        });
                });
                dropped = payload;
            });

            if let Some(dragged) = dropped {
                if let Some(act) = transition(&dragged, lane) {
                    picked = Some(act);
                }
            }
        }
    });

    picked
}

fn chip(text: &str, on: bool) -> RichText {
    RichText::new(text)
        .font(theme::body(11.5))
        .color(if on { theme::TEXT } else { theme::MUTED })
}

/// A column's label, with its count and its rule of color.
fn header(ui: &mut egui::Ui, lane: Lane, count: usize) {
    let (title, color) = column(lane);

    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(RichText::new(title).font(theme::medium(12.0)).color(color));
        ui.label(
            RichText::new(count.to_string()).font(theme::mono(10.5)).color(theme::FAINT),
        );
    });
    ui.add_space(4.0);

    let width = ui.available_width() - 8.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 2.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, theme::tint(color, 110));
    ui.add_space(8.0);
}

/// Leaves the drop zone in the column's color, but only if what is being
/// dragged can land there. A column that lights up for anything is not saying
/// anything.
fn paint_drop_zone(ui: &mut egui::Ui, lane: Lane) {
    let accepts = egui::DragAndDrop::payload::<Dragged>(ui.ctx())
        .is_some_and(|dragged| allows(dragged.lane, lane));
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
fn draw_card(ui: &mut egui::Ui, card: &Card, busy: bool) -> Option<Act> {
    let frame = egui::Frame::NONE
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .stroke(Stroke::new(1.0, theme::HAIRLINE));

    let id = egui::Id::new(("card", &card.branch));
    let dragging = ui.ctx().is_being_dragged(id);
    let payload = Dragged { branch: card.branch.clone(), lane: card.lane };

    let inner = ui.dnd_drag_source(id, payload, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;

            // The branch you are on wears HEAD's amber, as in the graph and in
            // the bar: it is the same question answered in the same color.
            ui.label(
                RichText::new(card.leaf())
                    .font(theme::medium(12.5))
                    .color(if card.current { theme::HEAD } else { theme::TEXT }),
            );
            ui.label(RichText::new(note(card)).font(theme::body(11.0)).color(theme::MUTED));
        });
    });

    let response = inner.response.interact(egui::Sense::click());

    // While it is in the air the card is painted on another layer and shifted,
    // so anything added here on top would land in the hole it left.
    if dragging {
        return None;
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

    let switchable = !card.current && !busy;
    let hint = match (switchable, allows(card.lane, Lane::Done)) {
        (true, true) => format!(
            "{}\nDouble-click to switch to it, or drag it to another column",
            card.branch
        ),
        (true, false) => format!("{}\nDouble-click to switch to this branch", card.branch),
        _ => card.branch.clone(),
    };
    let response = response.on_hover_text(hint);

    if switchable && response.double_clicked() {
        return Some(Act::Pick(Pick::Switch { name: card.branch.clone(), remote: false }));
    }
    response.clicked().then(|| Act::Pick(Pick::Show(card.target)))
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
