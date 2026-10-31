//! The sidebar of references: locals, remotes and tags.
//!
//! Branches are grouped by their folder (`feat/`, `fix/`...) the way the
//! branch tree of the graphical clients does, because a naming convention
//! with slashes is the norm as soon as a repository grows.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use gitrust_core::{Oid, RefEntry, RefKind};

use crate::theme;

/// What has been asked for from the sidebar.
pub enum Pick {
    /// Show in the graph the commit the reference points at.
    Show(Oid),
    /// Switch to that branch.
    Switch { name: String, remote: bool },
    /// Take that tag off here.
    DeleteTag(String),
    /// Delete that branch: the one here, or the one on the remote.
    DeleteBranch { name: String, remote: bool },
    /// Bring that branch into the one you are on.
    Merge(String),
}

/// Gives back whatever has been asked about a reference.
///
/// With `busy` set there is an operation under way and branches stop being
/// clickable: switching branches while something is coming down from the
/// remote is asking two things at once of the same files.
pub fn show(
    ui: &mut egui::Ui,
    refs: &[RefEntry],
    filter: &mut String,
    selected: Option<Oid>,
    head: Option<&str>,
    busy: bool,
) -> Option<Pick> {
    let mut picked = None;

    ui.add_space(8.0);
    ui.add(
        egui::TextEdit::singleline(filter)
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(8.0, 5.0))
            .font(theme::body(12.5))
            .hint_text(RichText::new("Find a branch").color(theme::FAINT)),
    );
    ui.add_space(8.0);

    let needle = filter.trim().to_lowercase();
    let matching: Vec<&RefEntry> = refs
        .iter()
        .filter(|entry| needle.is_empty() || entry.name.to_lowercase().contains(&needle))
        .collect();

    if refs.is_empty() {
        theme::empty(ui, "No references");
        return None;
    }

    if matching.is_empty() {
        theme::empty(ui, "No branch by that name");
        return None;
    }

    egui::ScrollArea::vertical()
        .id_salt("refs")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (title, kind) in [
                ("Branches", RefKind::LocalBranch),
                ("Remotes", RefKind::RemoteBranch),
                ("Tags", RefKind::Tag),
            ] {
                section(ui, title, kind, &matching, selected, head, busy, &mut picked);
            }
            ui.add_space(12.0);
        });

    picked
}

#[allow(clippy::too_many_arguments)]
fn section(
    ui: &mut egui::Ui,
    title: &str,
    kind: RefKind,
    refs: &[&RefEntry],
    selected: Option<Oid>,
    head: Option<&str>,
    busy: bool,
    picked: &mut Option<Pick>,
) {
    let entries: Vec<&RefEntry> = refs.iter().copied().filter(|e| e.kind == kind).collect();
    if entries.is_empty() {
        return;
    }

    theme::section(ui, title, entries.len());

    // The gesture cannot be seen, so it is said. And only where it is any
    // use: with one local branch, the one you are on, there is nowhere to go.
    if kind == RefKind::LocalBranch && entries.len() > 1 {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            ui.label(
                RichText::new("Double-click to switch branch")
                    .font(theme::body(10.5))
                    .color(theme::FAINT),
            );
        });
    }

    ui.add_space(2.0);

    // The ones with no folder go on top; the rest, in their group.
    let mut folders: BTreeMap<&str, Vec<&RefEntry>> = BTreeMap::new();
    for entry in &entries {
        match entry.folder() {
            Some(folder) => folders.entry(folder).or_default().push(entry),
            None => leaf(ui, entry, selected, head, busy, picked),
        }
    }

    for (folder, group) in folders {
        let label = egui::RichText::new(folder)
            .font(theme::body(12.0))
            .color(theme::MUTED);
        egui::CollapsingHeader::new(label)
            .id_salt((title, folder))
            .default_open(group.len() <= 8)
            .show(ui, |ui| {
                for entry in group {
                    leaf(ui, entry, selected, head, busy, picked);
                }
            });
    }

    ui.add_space(10.0);
}

#[allow(clippy::too_many_arguments)]
fn leaf(
    ui: &mut egui::Ui,
    entry: &RefEntry,
    selected: Option<Oid>,
    head: Option<&str>,
    busy: bool,
    picked: &mut Option<Pick>,
) {
    // The color says what kind of reference it is: locals in the graph's
    // azure, remotes muted because they usually duplicate a local one, and
    // tags in the violet of their chip.
    let color = match (entry.is_head, entry.kind) {
        (true, _) | (_, RefKind::Head) => theme::HEAD,
        (_, RefKind::LocalBranch) => theme::lane(0),
        (_, RefKind::RemoteBranch) => theme::MUTED,
        (_, RefKind::Tag) => theme::lane(6),
    };

    let font = if entry.is_head {
        theme::medium(12.5)
    } else {
        theme::body(12.5)
    };

    let is_selected = selected == Some(entry.target);
    let response = ui.selectable_label(
        is_selected,
        RichText::new(entry.leaf()).font(font).color(color),
    );

    // You can go to a branch. To the one you are already on there is nowhere
    // to go, and to a tag not yet: that leaves HEAD detached, which is a
    // place you have to know how to get out of and we do not show how here.
    let branch = matches!(entry.kind, RefKind::LocalBranch | RefKind::RemoteBranch);
    let switchable = branch && !entry.is_head && !busy;

    let response = response.on_hover_text(if switchable {
        format!("{}\nDouble-click to switch to this branch", entry.name)
    } else {
        entry.name.clone()
    });

    // A tag leads nowhere, but it can be taken out of the way.
    let removable = entry.kind == RefKind::Tag && !busy;

    if switchable || removable {
        let remote = entry.kind == RefKind::RemoteBranch;
        response.context_menu(|ui| {
            if switchable {
                let button = ui.button("Switch to this branch");
                let button = if remote {
                    button.on_hover_text("creates the local branch here if it is not there yet")
                } else {
                    button
                };
                if button.clicked() {
                    *picked = Some(Pick::Switch { name: entry.name.clone(), remote });
                    ui.close_menu();
                }
            }

            // Bringing it into the one you are on. With no branch checked out
            // there is nowhere to bring it, so then it is not even offered.
            if let Some(head) = head.filter(|_| switchable) {
                if ui
                    .button(format!("Merge into {head}"))
                    .on_hover_text("if they clash, nothing is touched and it says where")
                    .clicked()
                {
                    *picked = Some(Pick::Merge(entry.name.clone()));
                    ui.close_menu();
                }
            }

            if switchable {
                let label = if remote {
                    "Delete the branch on the remote"
                } else {
                    "Delete the branch"
                };
                let hint = if remote {
                    "deleted on the server: everyone will see it"
                } else {
                    "only the one here; anything on a remote stays"
                };
                if ui.button(label).on_hover_text(hint).clicked() {
                    *picked = Some(Pick::DeleteBranch { name: entry.name.clone(), remote });
                    ui.close_menu();
                }
            }

            if removable
                && ui
                    .button("Delete the tag")
                    .on_hover_text("only the one here; the remote's stays put")
                    .clicked()
            {
                *picked = Some(Pick::DeleteTag(entry.name.clone()));
                ui.close_menu();
            }
        });

        if switchable && response.double_clicked() {
            *picked = Some(Pick::Switch { name: entry.name.clone(), remote });
            return;
        }
    }

    if response.clicked() {
        *picked = Some(Pick::Show(entry.target));
    }
}
