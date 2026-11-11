//! The screen for the identities this machine works under.
//!
//! The second screen in the application that is not about a repository --
//! the first is the one for choosing one -- and it is here for the same
//! reason: which client you are working as is a question you answer before
//! there is a repository open, and the answer outlives the tab.
//!
//! The list is on the left and whichever one is picked is on the right,
//! which is the shape every preferences window has had for thirty years and
//! is not worth being original about. What is worth saying is what the
//! screen does *not* do: it never writes into `~/.gitconfig` or
//! `~/.ssh/config`. A profile is gitrust's own answer to "who am I right
//! now", and a program that rewrites the files other programs read is a
//! program that breaks them while you are not looking.
//!
//! Like every other view here it touches nothing: it is handed the profiles
//! and hands back what was asked of it. The one thing it does change in
//! place is the text in the boxes, because that is what a text box is.

use std::path::Path;

use eframe::egui::{self, RichText};
use gitrust_core::{Account, Forge, Profiles};

use crate::theme;

/// What has been asked of the screen.
pub enum Act {
    /// Something was typed: write the profiles down as they now are.
    ///
    /// Sent on the keystroke rather than on a button, because a Save button
    /// is a thing to forget to press and the file is four lines long.
    Changed,
    /// Work as this one from now on, or as nobody.
    Use(Option<usize>),
    /// Make one, with nothing in it.
    Add,
    /// Make one that is a copy of this one.
    Duplicate(usize),
    /// Take one away.
    Remove(usize),
    /// Ask the desktop for a key file for the profile being looked at.
    Browse(usize),
    /// Make this profile a key of its own, where it says it wants one.
    MakeKey(usize),
    /// Put the public half of this profile's key on the clipboard, which is
    /// the only thing anybody ever does with it: a forge asks for it in a
    /// box on a web page.
    CopyKey(usize),
    /// Open this screen. It is the one thing only the menu in the tab strip
    /// asks for, and it is here because that menu answers in the same words
    /// as the screen does.
    Manage,
    /// Put the screen away.
    Close,
}

/// What the screen remembers between frames: which profile is open, and
/// whether the tokens are being shown.
#[derive(Default)]
pub struct Managing {
    /// The one being edited. It is a position, so a profile renamed under
    /// the pointer stays open.
    pub at: Option<usize>,
    /// Tokens are dots until somebody asks to see one. The asking is per
    /// screen and not remembered: it goes back to dots the next time this is
    /// opened, which is what somebody expects of a password box.
    showing: bool,
    /// The profile whose deletion has been asked for but not yet confirmed.
    ///
    /// Deleting used to be one click, which is one click away from losing a
    /// token that is written down nowhere else.
    asking: Option<usize>,
}

impl Managing {
    /// Opens the screen on one profile in particular, with nothing pending.
    pub fn open(&mut self, at: usize) {
        self.at = Some(at);
        self.asking = None;
    }

    /// Nothing is waiting on an answer any more.
    pub fn settled(&mut self) {
        self.asking = None;
    }
}

/// Draws the screen and gives back whatever was asked of it.
pub fn show(ui: &mut egui::Ui, state: &mut Managing, profiles: &mut Profiles) -> Option<Act> {
    let mut act = None;

    // A profile that was deleted leaves the pointer past the end of the list.
    if state.at.is_some_and(|at| at >= profiles.list.len()) {
        state.at = None;
    }
    if state.at.is_none() && !profiles.list.is_empty() {
        state.at = Some(profiles.at().unwrap_or(0).min(profiles.list.len() - 1));
    }

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Profiles")
                .font(theme::strong(17.0))
                .color(theme::text()),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new("Who you commit as, the key that opens the remote, and the accounts behind the board")
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
    ui.add_space(8.0);

    ui.horizontal_top(|ui| {
        ui.add_space(4.0);
        if let Some(asked) = list(ui, state, profiles) {
            act = Some(asked);
        }
        ui.add_space(14.0);
        theme::vrule(ui);
        ui.add_space(14.0);
        if let Some(asked) = editor(ui, state, profiles) {
            act = Some(asked);
        }
    });

    act
}

/// The profiles, and the one in use among them.
fn list(ui: &mut egui::Ui, state: &mut Managing, profiles: &Profiles) -> Option<Act> {
    let mut act = None;

    ui.vertical(|ui| {
        ui.set_width(236.0);

        // Working as nobody is a row like any other, because it is a choice
        // and not the absence of one: it means Git's own settings, the SSH
        // agent, and whatever `gh` is logged in as.
        let plain = ui.selectable_label(
            profiles.at().is_none(),
            RichText::new("Git's own settings")
                .font(theme::body(12.5))
                .color(match profiles.at().is_none() {
                    true => theme::text(),
                    false => theme::muted(),
                }),
        );
        if plain
            .on_hover_text(
                "No profile: the name from your Git configuration, the key from the \
                 SSH agent, and the forge as its own tool is logged in",
            )
            .clicked()
        {
            act = Some(Act::Use(None));
        }

        ui.add_space(6.0);
        theme::rule(ui);
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("profiles")
            .max_height(ui.available_height() - 44.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (which, profile) in profiles.list.iter().enumerate() {
                    let using = profiles.at() == Some(which);
                    let open = state.at == Some(which);

                    ui.horizontal(|ui| {
                        // The dot says which one is in use, and pressing it
                        // is how a profile is put to work.
                        let mark = dot(ui, using).on_hover_text(match using {
                            true => "In use",
                            false => "Work as this one",
                        });
                        if mark.clicked() {
                            act = Some(Act::Use(Some(which)));
                        }

                        let name = match profile.name.trim().is_empty() {
                            true => "(no name)".to_string(),
                            false => profile.name.clone(),
                        };
                        let row = ui.add_sized(
                            egui::vec2(ui.available_width() - 4.0, 22.0),
                            egui::SelectableLabel::new(
                                open,
                                RichText::new(name)
                                    .font(match using {
                                        true => theme::medium(12.5),
                                        false => theme::body(12.5),
                                    })
                                    .color(match open || using {
                                        true => theme::text(),
                                        false => theme::muted(),
                                    }),
                            ),
                        );
                        if row.clicked() {
                            state.at = Some(which);
                        }
                    });
                }
            });

        ui.add_space(8.0);
        if ui
            .add(egui::Button::new(RichText::new("+  Add a profile").font(theme::body(12.0))))
            .clicked()
        {
            act = Some(Act::Add);
        }
    });

    act
}

/// The profile that is open: everything it says, and everything it can say.
fn editor(ui: &mut egui::Ui, state: &mut Managing, profiles: &mut Profiles) -> Option<Act> {
    let mut act = None;
    let Some(which) = state.at else {
        ui.vertical(|ui| {
            theme::empty(
                ui,
                "No profile yet. Add one for each client you work for:\n\
                 a name for the commits, a key for the remote, an account for the forge.",
            );
        });
        return None;
    };
    let showing = state.showing;
    let using = profiles.at() == Some(which);
    // Carried out of the borrow below and put back after it, because the
    // profile being edited and the screen's own memory cannot both be held
    // at once.
    let confirming = state.asking == Some(which);
    let mut asking = state.asking;
    let Some(profile) = profiles.list.get_mut(which) else {
        return None;
    };
    let called = match profile.name.trim().is_empty() {
        true => "(no name)".to_string(),
        false => profile.name.trim().to_string(),
    };

    ui.vertical(|ui| {
        ui.set_width(ui.available_width() - 8.0);

        egui::ScrollArea::vertical()
            .id_salt("profile")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if field(ui, "Name", &mut profile.name, "Acme Ltd", false) {
                    act = Some(Act::Changed);
                }

                ui.add_space(14.0);
                heading(ui, "On the commits");
                hint(
                    ui,
                    "Left empty, commits are written with the name and address in your Git \
                     configuration, exactly as they are now.",
                );
                if field(ui, "Author", &mut profile.author, "Someone", false) {
                    act = Some(Act::Changed);
                }
                if field(ui, "Email", &mut profile.email, "someone@acme.example", false) {
                    act = Some(Act::Changed);
                }

                ui.add_space(14.0);
                heading(ui, "On the remote");
                hint(
                    ui,
                    "The private key for this client's host. Left empty, the SSH agent answers \
                     as it does today \u{2014} which is where a key with a passphrase has to stay, \
                     because there is nowhere here to ask for one.",
                );
                ui.horizontal(|ui| {
                    if field(ui, "SSH key", &mut profile.key, "~/.ssh/acme_ed25519", true) {
                        act = Some(Act::Changed);
                    }
                    if ui.button("Browse\u{2026}").clicked() {
                        act = Some(Act::Browse(which));
                    }
                });

                // Three states, and each one gets a different line under the
                // box, because "no file there" while a key is being made and
                // "no file there" after somebody mistyped a path are the
                // same four words about two different situations.
                let key = profile.key_file();
                let made = key.as_deref().is_some_and(Path::exists);
                let public = profile.public_key().filter(|path| path.exists());
                ui.horizontal(|ui| {
                    ui.add_space(LABEL + 8.0);
                    match (&key, made) {
                        (Some(_), false) => {
                            ui.label(
                                RichText::new("there is no file there")
                                    .font(theme::body(11.0))
                                    .color(theme::removed()),
                            );
                        }
                        _ => {}
                    }
                    // Making one is offered whether or not a path is typed:
                    // with one it makes that key, without one it makes a key
                    // named after the profile under `~/.ssh`. What it will
                    // never do is write over a file that is already there.
                    if !made
                        && ui
                            .add(egui::Button::new(
                                RichText::new("Make one").font(theme::body(11.5)),
                            ))
                            .on_hover_text(
                                "ed25519, no passphrase, beside your other keys. The public \
                                 half is yours to paste into the client's forge",
                            )
                            .clicked()
                    {
                        act = Some(Act::MakeKey(which));
                    }
                    if public.is_some()
                        && ui
                            .add(egui::Button::new(
                                RichText::new("Copy the public key").font(theme::body(11.5)),
                            ))
                            .on_hover_text(
                                "The half that goes in the box on the forge's settings page",
                            )
                            .clicked()
                    {
                        act = Some(Act::CopyKey(which));
                    }
                    if made && public.is_none() {
                        ui.label(
                            RichText::new("the key is there; its .pub half is not")
                                .font(theme::body(11.0))
                                .color(theme::faint()),
                        );
                    }
                });
                ui.add_space(2.0);

                ui.add_space(14.0);
                heading(ui, "Accounts");
                hint(
                    ui,
                    "Which forge answers for a repository is decided by the host: a remote on \
                     one of these hosts is read with this account's token, and asked about \
                     with its own tool \u{2014} gh for GitHub, glab for GitLab.",
                );

                let mut drop = None;
                for (at, account) in profile.accounts.iter_mut().enumerate() {
                    ui.add_space(6.0);
                    if let Some(asked) = account_row(ui, account, at, showing) {
                        match asked {
                            Row::Changed => act = Some(Act::Changed),
                            Row::Remove => drop = Some(at),
                            Row::Show(now) => state.showing = now,
                        }
                    }
                }
                if let Some(at) = drop {
                    profile.accounts.remove(at);
                    act = Some(Act::Changed);
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(LABEL + 8.0);
                    for forge in [Forge::GitHub, Forge::GitLab] {
                        if ui
                            .add(egui::Button::new(
                                RichText::new(format!("+  {}", forge.label())).font(theme::body(12.0)),
                            ))
                            .clicked()
                        {
                            profile.accounts.push(Account::new(forge));
                            act = Some(Act::Changed);
                        }
                    }
                });

                ui.add_space(18.0);
                theme::rule(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let use_it = ui.add_enabled(
                        !using,
                        egui::Button::new(
                            RichText::new(match using {
                                true => "In use",
                                false => "Work as this one",
                            })
                            .font(theme::medium(12.0)),
                        )
                        .fill(match using {
                            true => theme::raised(),
                            false => theme::chosen(),
                        })
                        .min_size(egui::vec2(140.0, 28.0)),
                    );
                    if use_it.clicked() {
                        act = Some(Act::Use(Some(which)));
                    }

                    if ui
                        .add(egui::Button::new(
                            RichText::new("Duplicate").font(theme::body(12.0)),
                        ))
                        .on_hover_text(
                            "The same key and the same accounts under a second name, which \
                             is what a second person at the same client is",
                        )
                        .clicked()
                    {
                        act = Some(Act::Duplicate(which));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                RichText::new("Delete this profile")
                                    .font(theme::body(12.0))
                                    .color(theme::removed()),
                            ))
                            .clicked()
                        {
                            asking = Some(which);
                        }
                    });
                });

                // The asking, in place rather than in a window over the
                // screen: there is one thing to answer and it is about the
                // row that is already open.
                if confirming {
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Delete \u{201c}{called}\u{201d}? The key stays on the disk \
                                 and the account stays at the forge. What goes is this list's \
                                 line about them, and it does not come back."
                            ))
                            .font(theme::body(11.5))
                            .color(theme::text()),
                        );
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Delete it")
                                        .font(theme::medium(12.0))
                                        .color(theme::removed()),
                                )
                                .min_size(egui::vec2(100.0, 26.0)),
                            )
                            .clicked()
                        {
                            act = Some(Act::Remove(which));
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Keep it").font(theme::body(12.0)))
                                    .min_size(egui::vec2(90.0, 26.0)),
                            )
                            .clicked()
                        {
                            asking = None;
                        }
                    });
                }
                ui.add_space(16.0);
            });
    });

    state.asking = asking;
    act
}

/// What a row of an account asked for.
enum Row {
    Changed,
    Remove,
    /// Show the tokens, or go back to dots.
    Show(bool),
}

/// One account: which forge, where, as whom, and with what.
fn account_row(ui: &mut egui::Ui, account: &mut Account, at: usize, showing: bool) -> Option<Row> {
    let mut asked = None;

    ui.horizontal(|ui| {
        ui.add_space(LABEL + 8.0);
        // The forge is two buttons and not a dropdown: there are two of them,
        // and a menu to choose between two things is a click spent on
        // nothing.
        for forge in [Forge::GitHub, Forge::GitLab] {
            let on = account.forge == forge;
            if ui
                .selectable_label(on, RichText::new(forge.label()).font(theme::body(11.5)))
                .clicked()
                && !on
            {
                // The host moves with the forge only while it is the other
                // forge's own: a company's own server keeps its address.
                if account.host.is_empty() || account.host == account.forge.home() {
                    account.host = forge.home().to_string();
                }
                account.forge = forge;
                asked = Some(Row::Changed);
            }
        }

        if ui
            .add(
                egui::Button::new(RichText::new("\u{00d7}").font(theme::body(13.0)).color(theme::faint()))
                    .frame(false)
                    .min_size(egui::vec2(18.0, 18.0)),
            )
            .on_hover_text("Take this account off the profile")
            .clicked()
        {
            asked = Some(Row::Remove);
        }
    });

    ui.horizontal(|ui| {
        ui.add_space(LABEL + 8.0);
        if box_(ui, &mut account.host, "host", 180.0, ("host", at), false) {
            asked = Some(Row::Changed);
        }
        if box_(ui, &mut account.user, "user name", 150.0, ("user", at), false) {
            asked = Some(Row::Changed);
        }
        if box_(ui, &mut account.token, "token", 190.0, ("token", at), !showing) {
            asked = Some(Row::Changed);
        }
        // In words, not in a symbol. An eye is a picture the fonts on this
        // machine do not have, and a box where the eye should be is worse
        // than the word it stands for.
        let eye = ui.add(
            egui::Button::new(
                RichText::new(match showing {
                    true => "hide",
                    false => "show",
                })
                .font(theme::body(11.0))
                .color(theme::faint()),
            )
            .frame(false)
            .min_size(egui::vec2(34.0, 18.0)),
        );
        if eye.clicked() {
            asked = Some(Row::Show(!showing));
        }
    });

    if account.token.trim().is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(LABEL + 8.0);
            ui.label(
                RichText::new(format!(
                    "no token: {} is left to use whatever it is logged in as",
                    account.forge.tool()
                ))
                .font(theme::body(10.5))
                .color(theme::faint()),
            );
        });
    }

    asked
}

/// The mark that says which profile is in use, and the button that puts one
/// to work.
///
/// Painted rather than typed, for the same reason the theme paints its
/// glyphs: the fonts actually on this machine have no filled circle in them,
/// and a symbol that comes out as an empty box says less than nothing.
fn dot(ui: &mut egui::Ui, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let colour = match on {
        true => theme::head(),
        false => theme::faint(),
    };
    let painter = ui.painter();
    match on {
        true => painter.circle_filled(rect.center(), 4.0, colour),
        false => painter.circle_stroke(rect.center(), 4.0, egui::Stroke::new(1.2, colour)),
    };
    response
}

/// How wide the labels down the left of the editor are, so the boxes beside
/// them line up whatever they are called.
const LABEL: f32 = 76.0;

/// One labelled box. `true` when somebody typed in it.
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, wide: bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.allocate_ui(egui::vec2(LABEL, 22.0), |ui| {
            ui.label(RichText::new(label).font(theme::body(12.0)).color(theme::muted()));
        });
        let room = match wide {
            true => (ui.available_width() - 90.0).max(180.0),
            false => 320.0,
        };
        changed = ui
            .add(
                egui::TextEdit::singleline(value)
                    .desired_width(room)
                    .margin(egui::vec2(8.0, 5.0))
                    .font(match wide {
                        true => theme::mono(11.5),
                        false => theme::body(12.0),
                    })
                    .hint_text(RichText::new(hint).color(theme::faint())),
            )
            .changed();
    });
    ui.add_space(4.0);
    changed
}

/// A box with no label of its own, for the rows of accounts.
fn box_(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    width: f32,
    salt: (&str, usize),
    secret: bool,
) -> bool {
    ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(salt)
            .desired_width(width)
            .margin(egui::vec2(8.0, 5.0))
            .font(theme::mono(11.5))
            .password(secret)
            .hint_text(RichText::new(hint).color(theme::faint())),
    )
    .changed()
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).font(theme::medium(11.5)).color(theme::muted()));
    });
    ui.add_space(4.0);
}

/// A line under a heading saying what happens when the boxes are left empty.
/// Every one of them is a sentence about the default, because the default is
/// the thing nobody can see.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).font(theme::body(11.0)).color(theme::faint()));
    });
    ui.add_space(6.0);
}
