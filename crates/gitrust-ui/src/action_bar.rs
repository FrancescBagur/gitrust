//! What gets done to the whole repository, in a single row.
//!
//! Four things, and all four look at the current branch: bring down what is
//! missing, send up what is surplus, open a new branch where you are, and set
//! aside what is in the way. The first two carry their count beside them,
//! because knowing what you owe is half of deciding whether to press.

use chrono::{Local, TimeZone};
use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat},
    Color32, PopupCloseBehavior, RichText,
};
use gitrust_core::{PullMode, StashEntry, Tracking};

use crate::theme;

/// How much room one stash takes in the list, with its padding. It is used to
/// give the list its height, as it cannot measure itself in there.
const STASH_ROW: f32 = 52.0;

/// How wide each menu is, measured inside its frame.
///
/// Told, not asked for. A menu that asks how much room there is gets a
/// different answer depending on what is in it that day, and the answer is
/// always generous: that is how the stash one came to be three times the
/// width of the button it hangs from.
const PULL_MENU: f32 = 248.0;
const PUSH_MENU: f32 = 190.0;
const BRANCH_MENU: f32 = 250.0;
const STASH_MENU: f32 = 310.0;

pub enum Action {
    /// Fetch and join, in whichever way is asked for.
    Pull(PullMode),
    /// Fetch from every remote and join nothing.
    Fetch,
    Push,
    PushForce,
    PushTags,
    Branch(String),
    StashSave(String),
    StashApply(usize),
    StashPop(usize),
    StashDrop(usize),
}

/// What is being typed in the popups. It lives in the app because it has to
/// still be there on the next frame.
#[derive(Default)]
pub struct Draft {
    branch: String,
    stash: String,
}

/// Paints the group and gives back whatever was pressed.
///
/// With `busy` set there is an operation under way: the buttons go dark
/// altogether rather than letting a second one queue up while the first is
/// out on the network.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    tracking: Option<&Tracking>,
    stashes: &[StashEntry],
    mode: &mut PullMode,
    busy: bool,
    draft: &mut Draft,
) -> Option<Action> {
    let mut action = None;

    ui.scope(|ui| {
        // Two gaps, and the difference between them is the whole of what says
        // which caret belongs to which button. A split control is one thing
        // with two halves: its caret sits a hair from its own button and a
        // clear step from the next control.
        //
        // The spacing in force when a widget is placed is the gap that opens
        // *after* it, not before it. Set a pixel and then draw the caret and
        // it is the caret that ends up glued to whatever comes next -- which
        // is what was happening: every caret hung off the button to its
        // right, and said so plainly, while the menu under it belonged to
        // the one on its left.
        const PAIRED: f32 = 1.0;
        const APART: f32 = 14.0;

        let behind = tracking.map(|t| t.behind).unwrap_or(0);
        let ahead = tracking.map(|t| t.ahead).unwrap_or(0);

        // With no remote branch to look at there is nowhere to fetch from;
        // pushing you can do, and that is exactly how one gets created.
        // The rectangle is kept because the menu behind the caret hangs from
        // the pair, not from the caret: see `anchored`.
        ui.spacing_mut().item_spacing.x = PAIRED;
        let pull = count_button(ui, "↓ Pull", behind, tracking.is_some() && !busy)
            .on_hover_text(pull_hint(tracking, *mode));
        if pull.clicked() {
            action = Some(Action::Pull(*mode));
        }

        ui.spacing_mut().item_spacing.x = APART;
        if let Some(other) = pull_menu(ui, busy, mode, pull.rect) {
            action = Some(other);
        }

        ui.spacing_mut().item_spacing.x = APART;
        if let Some(other) = push_menu(ui, busy, ahead, tracking) {
            action = Some(other);
        }

        if let Some(name) = branch_menu(ui, busy, &mut draft.branch) {
            action = Some(Action::Branch(name));
        }
        if let Some(done) = stash_menu(ui, busy, stashes, &mut draft.stash) {
            action = Some(done);
        }
    });

    action
}

/// A button with its count flush against it, in the same amber chip the
/// changes tab wears: it means the same thing, that something of yours is due.
fn count_button(ui: &mut egui::Ui, label: &str, count: usize, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(count_label(label, count, enabled)))
}

fn count_label(label: &str, count: usize, enabled: bool) -> LayoutJob {
    let ink = if enabled { theme::text() } else { theme::faint() };
    let mut job = LayoutJob::default();
    job.append(label, 0.0, TextFormat::simple(theme::medium(12.5), ink));

    if count > 0 {
        job.append(
            &format!(" {count} "),
            6.0,
            TextFormat {
                font_id: theme::mono(10.5),
                color: if enabled { theme::changed() } else { theme::faint() },
                background: if enabled {
                    theme::tint(theme::changed(), 32)
                } else {
                    Color32::TRANSPARENT
                },
                ..Default::default()
            },
        );
    }

    job
}

/// Hands egui a response pinned to the whole control instead of to the part
/// that was pressed.
///
/// A popup opens at the left edge of the widget egui is given, and grows
/// rightwards from there. For a split button that widget is the caret, which
/// is the narrow half sitting flush against the next button along, so the menu
/// opens where *that* button starts and reads as its menu. What has to be
/// pinned is the pair.
///
/// Only the rectangle is changed. Everything else egui reads off a response --
/// what is hovered, what was clicked, where the pointer went -- is how it
/// works out that a click landed outside and the menu should close, and that
/// is still the caret's business and nobody else's.
fn anchored(
    ui: &egui::Ui,
    opener: &egui::Response,
    control: egui::Rect,
    width: f32,
) -> egui::Response {
    let whole = width + egui::Frame::popup(ui.style()).total_margin().sum().x;

    // Left edges together, which is what belonging looks like. Unless the menu
    // would hang off the window from there -- which is what happens to the
    // last control in the row -- and then the right edges go together instead,
    // and it still meets the control that opened it.
    let left = match control.left() + whole > ui.ctx().screen_rect().right() {
        true => control.right() - whole,
        false => control.left(),
    };

    let mut anchored = opener.clone();
    anchored.rect = control.translate(egui::vec2(left - control.left(), 0.0));
    anchored
}

/// While its menu is open, the control wears an outline.
///
/// Where the menu hangs says which button opened it, but it says it only to
/// somebody who was watching it appear. The outline says it to whoever looks
/// down afterwards, and around a split button it says the second thing too:
/// that the two halves are one control.
///
/// In the caret's own grey. The borders in this window are hairlines meant to
/// be felt rather than seen, and a mark nobody can see is not a mark; this is
/// the same ink as the arrow sitting inside the control, so it reads as part
/// of the same control rather than as a line somebody drew round it.
fn open_mark(ui: &egui::Ui, id: egui::Id, control: egui::Rect) {
    if !ui.memory(|memory| memory.is_popup_open(id)) {
        return;
    }

    ui.painter().rect_stroke(
        control,
        egui::CornerRadius::same(theme::RADIUS),
        egui::Stroke::new(1.0, theme::muted()),
        egui::StrokeKind::Outside,
    );
}

fn pull_hint(tracking: Option<&Tracking>, mode: PullMode) -> String {
    let how = match mode {
        PullMode::Merge => "merging if it has to",
        PullMode::FastForward => "fast-forward only",
        PullMode::Rebase => "rebasing",
    };
    match tracking {
        Some(t) if t.behind > 0 => format!("pull {} commits from {}, {how}", t.behind, t.remote),
        Some(t) => format!("{} has nothing new", t.remote),
        None => "this branch tracks no remote one".to_string(),
    }
}

/// The other ways of fetching.
///
/// What you pick here happens at once and stays as the button's position: the
/// button repeats the last thing you asked for, which is what one expects of
/// a control with a memory, and needs explaining nowhere.
fn pull_menu(ui: &mut egui::Ui, busy: bool, mode: &mut PullMode, main: egui::Rect) -> Option<Action> {
    let caret = ui.add_enabled(!busy, egui::Button::new("").min_size(egui::vec2(20.0, 0.0)));
    theme::caret(ui, &caret);
    let id = ui.make_persistent_id("menu-pull");
    let control = main.union(caret.rect);

    if caret.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    open_mark(ui, id, control);

    let anchor = anchored(ui, &caret, control, PULL_MENU);
    egui::popup::popup_below_widget(ui, id, &anchor, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_width(PULL_MENU);
        let mut action = None;

        // Fetching joins nothing, so it does not change what the button does.
        if ui
            .button("Fetch all")
            .on_hover_text("from every remote, touching nothing here")
            .clicked()
        {
            action = Some(Action::Fetch);
        }

        theme::rule(ui);

        for (label, what) in [
            ("Pull (fast-forward if possible)", PullMode::Merge),
            ("Pull (fast-forward only)", PullMode::FastForward),
            ("Pull (rebase)", PullMode::Rebase),
        ] {
            if ui.selectable_label(*mode == what, label).clicked() {
                *mode = what;
                action = Some(Action::Pull(what));
            }
        }

        if action.is_some() {
            ui.memory_mut(|memory| memory.close_popup());
        }
        action
    })
    .flatten()
}

fn push_hint(tracking: Option<&Tracking>) -> String {
    match tracking {
        Some(t) if t.ahead > 0 => format!("push {} commits to {}", t.ahead, t.remote),
        Some(t) => format!("{} already has everything", t.remote),
        None => "push this branch for the first time".to_string(),
    }
}

/// The push menu deliberately has the same one-piece control as Branch and
/// Stash. A separate caret looked like it belonged to the next action.
fn push_menu(
    ui: &mut egui::Ui,
    busy: bool,
    ahead: usize,
    tracking: Option<&Tracking>,
) -> Option<Action> {
    let mut label = count_label("↑ Push", ahead, !busy);
    // `caret` is painted inside the button, just as it is for Branch and
    // Stash. Reserve its room in the text job so it never sits on the count.
    label.append(
        "     ",
        0.0,
        TextFormat::simple(theme::body(11.0), theme::muted()),
    );
    let button = ui
        .add_enabled(!busy, egui::Button::new(label))
        .on_hover_text(push_hint(tracking));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-push");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    open_mark(ui, id, button.rect);

    let anchor = anchored(ui, &button, button.rect, PUSH_MENU);
    egui::popup::popup_below_widget(ui, id, &anchor, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_width(PUSH_MENU);
        let mut action = None;

        if ui.button("Push").clicked() {
            action = Some(Action::Push);
        }
        theme::rule(ui);

        if ui
            .button("Force the push")
            .on_hover_text("refuses if anyone has touched the branch since you fetched")
            .clicked()
        {
            action = Some(Action::PushForce);
        }
        if ui
            .button("Send the tags")
            .on_hover_text("the ones the remote does not have yet")
            .clicked()
        {
            action = Some(Action::PushTags);
        }

        if action.is_some() {
            ui.memory_mut(|memory| memory.close_popup());
        }
        action
    })
    .flatten()
}

/// The branch popup: it only creates, and it creates where you are.
///
/// Switching to a branch that already exists rewrites files on disk, and that
/// deserves its own place with its own warning. This touches nothing: the new
/// branch points at the same commit, so whatever is half-done stays as it is.
fn branch_menu(ui: &mut egui::Ui, busy: bool, name: &mut String) -> Option<String> {
    let button = ui.add_enabled(!busy, egui::Button::new(label("Branch")));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-branch");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    open_mark(ui, id, button.rect);

    // Here the caret is painted inside the button, so the control and the part
    // that was pressed are the same thing.
    let anchor = anchored(ui, &button, button.rect, BRANCH_MENU);
    egui::popup::popup_below_widget(ui, id, &anchor, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_width(BRANCH_MENU);
        ui.label(
            RichText::new("New branch right here")
                .font(theme::medium(12.0))
                .color(theme::text()),
        );
        ui.add_space(5.0);

        let entry = ui.add(
            egui::TextEdit::singleline(name)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(8.0, 5.0))
                .font(theme::mono(12.0))
                .hint_text(RichText::new("branch name").color(theme::faint())),
        );
        ui.add_space(5.0);

        let entered = entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let pressed = ui
            .add_enabled(!name.trim().is_empty(), egui::Button::new("Create and switch"))
            .clicked();

        ui.add_space(2.0);
        ui.label(
            RichText::new("The working tree is left alone")
                .font(theme::body(11.0))
                .color(theme::faint()),
        );

        (entered || pressed).then(|| {
            ui.memory_mut(|memory| memory.close_popup());
            std::mem::take(name)
        })
    })
    .flatten()
}

fn stash_menu(
    ui: &mut egui::Ui,
    busy: bool,
    stashes: &[StashEntry],
    message: &mut String,
) -> Option<Action> {
    let mut job = LayoutJob::default();
    job.append("Stash", 0.0, TextFormat::simple(theme::medium(12.5), theme::text()));
    if !stashes.is_empty() {
        job.append(
            &format!(" {} ", stashes.len()),
            6.0,
            TextFormat {
                font_id: theme::mono(10.5),
                color: theme::muted(),
                background: theme::tint(theme::muted(), 26),
                ..Default::default()
            },
        );
    }
    job.append("     ", 0.0, TextFormat::simple(theme::body(11.0), theme::muted()));

    let button = ui.add_enabled(!busy, egui::Button::new(job));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-stash");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }
    open_mark(ui, id, button.rect);

    let anchor = anchored(ui, &button, button.rect, STASH_MENU);
    egui::popup::popup_below_widget(ui, id, &anchor, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_width(STASH_MENU);
        let mut action = None;

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(message)
                    .desired_width(ui.available_width() - 76.0)
                    .margin(egui::vec2(8.0, 5.0))
                    .font(theme::body(12.0))
                    .hint_text(RichText::new("what you are setting aside").color(theme::faint())),
            );
            if ui.button("Stash").clicked() {
                action = Some(Action::StashSave(std::mem::take(message)));
            }
        });

        if stashes.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Nothing stashed")
                    .font(theme::body(11.5))
                    .color(theme::faint()),
            );
        } else {
            theme::rule(ui);

            // The height is told, not asked for. Inside a popup that sizes
            // itself, a ScrollArea that asks how much room there is gets less
            // than there is and clips the last row.
            let height = stashes.len().min(6) as f32 * STASH_ROW;
            egui::ScrollArea::vertical()
                .id_salt("stash-stack")
                .max_height(height)
                .min_scrolled_height(height)
                .show(ui, |ui| {
                    for entry in stashes {
                        if let Some(done) = stash_row(ui, entry) {
                            action = Some(done);
                        }
                    }
                });
        }

        if action.is_some() {
            ui.memory_mut(|memory| memory.close_popup());
        }
        action
    })
    .flatten()
}

/// One stash: what it is, where from and when, and what can be done to it.
fn stash_row(ui: &mut egui::Ui, entry: &StashEntry) -> Option<Action> {
    let mut action = None;
    let (branch, text) = parts(&entry.message);

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add(egui::Label::new(RichText::new(text).font(theme::body(12.0)).color(theme::text())))
            .on_hover_text(&entry.message);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // Which branch it came off: stashing on one and popping on another is
        // easy to do by accident, and this is the only thing that warns you.
        ui.label(RichText::new(branch).font(theme::body(11.0)).color(theme::muted()));
        if let Some(when) = when(entry.time) {
            ui.label(RichText::new("·").font(theme::body(11.0)).color(theme::faint()));
            ui.label(RichText::new(when).font(theme::mono(10.5)).color(theme::faint()));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);

            if ui.button(small("Drop", theme::muted())).on_hover_text("there is no getting it back").clicked() {
                action = Some(Action::StashDrop(entry.index));
            }
            if ui.button(small("Apply", theme::muted())).on_hover_text("and it stays stashed").clicked()
            {
                action = Some(Action::StashApply(entry.index));
            }
            if ui.button(small("Pop", theme::text())).on_hover_text("and it leaves the stack").clicked() {
                action = Some(Action::StashPop(entry.index));
            }
        });
    });
    });
    ui.add_space(4.0);

    action
}

/// Splits "On master: measuring the lane" into the branch and what you wrote.
///
/// The prefix Git puts there is the same phrase on every one, so read down a
/// list it tells nothing apart: what separates one stash from another is behind it.
fn parts(message: &str) -> (&str, &str) {
    let Some((head, text)) = message.split_once(": ") else {
        return ("", message);
    };
    let branch = head.rsplit(' ').next().unwrap_or("");

    // The automatic message carries the hash in front of the commit's subject,
    // which is surplus: what matters is what the stashed work was about.
    let text = text
        .split_once(' ')
        .filter(|(first, _)| first.len() >= 7 && first.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|(_, rest)| rest)
        .unwrap_or(text);

    (branch, text)
}

fn small(text: &str, color: Color32) -> RichText {
    RichText::new(text).font(theme::body(11.0)).color(color)
}

/// The text of a button that opens a popup, with room at the end for the
/// caret `theme::caret` paints.
fn label(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat::simple(theme::medium(12.5), theme::text()));
    job.append("     ", 0.0, TextFormat::simple(theme::body(11.0), theme::muted()));
    job
}

fn when(seconds: i64) -> Option<String> {
    (seconds > 0)
        .then(|| Local.timestamp_opt(seconds, 0).single())
        .flatten()
        .map(|dt| dt.format("%d/%m %H:%M").to_string())
}
