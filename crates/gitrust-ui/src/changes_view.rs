//! Panel de cambios: listas de índice y árbol de trabajo, diff y commit.

use eframe::egui::{self, Color32, RichText};
use gitrust_core::{ChangeKind, FileChange, FileDiff, HunkId, WorkingState, SUBJECT_LIMIT};

use crate::diff_view::{self, ADDED, REMOVED};

/// Lo que el usuario ha pedido en este fotograma. La vista no toca el
/// repositorio: solo describe la intención y deja que `main` la ejecute.
pub enum Action {
    Select { path: String, staged: bool },
    Stage(String),
    Unstage(String),
    StageHunk { path: String, hunk: HunkId },
    UnstageHunk { path: String, hunk: HunkId },
    StageAll,
    UnstageAll,
    Commit,
}

fn kind_color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added => Color32::from_rgb(0x6D, 0xC9, 0x51),
        ChangeKind::Modified => Color32::from_rgb(0xF2, 0x9D, 0x38),
        ChangeKind::Deleted => Color32::from_rgb(0xE5, 0x5F, 0x5F),
        ChangeKind::Renamed => Color32::from_rgb(0x55, 0xA2, 0xF7),
        ChangeKind::TypeChange => Color32::from_rgb(0xB0, 0x8E, 0xF5),
        ChangeKind::Untracked => Color32::from_gray(150),
        ChangeKind::Conflicted => Color32::from_rgb(0xFF, 0x6B, 0x6B),
    }
}

pub fn show(
    ui: &mut egui::Ui,
    state: &WorkingState,
    selected: Option<(&str, bool)>,
    diff: Option<&FileDiff>,
    subject: &mut String,
    body: &mut String,
) -> Option<Action> {
    let mut action = None;

    egui::SidePanel::left("cambios")
        .resizable(true)
        .default_width(330.0)
        .show_inside(ui, |ui| {
            // El cuadro de commit va abajo del todo, fijo.
            egui::TopBottomPanel::bottom("commit")
                .resizable(false)
                .show_inside(ui, |ui| {
                    ui.add_space(6.0);

                    // El asunto va aparte del cuerpo: Git espera una línea en
                    // blanco entre ambos y así no hay que recordarlo.
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Título").small().weak());
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let length = subject.chars().count();
                                let color = if length > SUBJECT_LIMIT {
                                    Color32::from_rgb(0xF2, 0x9D, 0x38)
                                } else {
                                    Color32::from_gray(110)
                                };
                                ui.label(
                                    RichText::new(format!("{length}/{SUBJECT_LIMIT}"))
                                        .small()
                                        .monospace()
                                        .color(color),
                                );
                            },
                        );
                    });
                    ui.add(
                        egui::TextEdit::singleline(subject)
                            .desired_width(f32::INFINITY)
                            .hint_text("feat: resumen en una línea"),
                    );

                    ui.add_space(6.0);
                    ui.label(RichText::new("Descripción").small().weak());
                    ui.add(
                        egui::TextEdit::multiline(body)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY)
                            .hint_text("Opcional: qué cambia y por qué"),
                    );
                    ui.add_space(6.0);

                    let ready = !state.staged.is_empty() && !subject.trim().is_empty();
                    let shortcut = ready
                        && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(ready, egui::Button::new("Commit"))
                            .on_hover_text("Ctrl+Enter")
                            .clicked()
                            || shortcut
                        {
                            action = Some(Action::Commit);
                        }

                        let note = if state.staged.is_empty() {
                            "nada preparado".to_string()
                        } else {
                            format!("{} ficheros", state.staged.len())
                        };
                        ui.label(RichText::new(note).weak().small());
                    });
                    ui.add_space(6.0);
                });

            egui::ScrollArea::vertical()
                .id_salt("listas")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    section(
                        ui,
                        "Preparado",
                        &state.staged,
                        selected,
                        true,
                        &mut action,
                    );
                    ui.add_space(10.0);
                    section(
                        ui,
                        "Sin preparar",
                        &state.unstaged,
                        selected,
                        false,
                        &mut action,
                    );
                });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        show_diff(ui, selected, diff, &mut action);
    });

    action
}

fn section(
    ui: &mut egui::Ui,
    title: &str,
    files: &[FileChange],
    selected: Option<(&str, bool)>,
    staged: bool,
    action: &mut Option<Action>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.label(RichText::new(files.len().to_string()).weak());

        if !files.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if staged { "Quitar todo" } else { "Preparar todo" };
                if ui.small_button(label).clicked() {
                    *action = Some(if staged { Action::UnstageAll } else { Action::StageAll });
                }
            });
        }
    });
    ui.separator();

    if files.is_empty() {
        ui.label(RichText::new("—").weak());
        return;
    }

    for file in files {
        ui.horizontal(|ui| {
            let button = if staged { "−" } else { "+" };
            if ui.small_button(button).on_hover_text(if staged { "Quitar del índice" } else { "Preparar" }).clicked() {
                *action = Some(if staged {
                    Action::Unstage(file.path.clone())
                } else {
                    Action::Stage(file.path.clone())
                });
            }

            ui.label(
                RichText::new(file.kind.tag())
                    .monospace()
                    .color(kind_color(file.kind)),
            );

            let is_selected = selected == Some((file.path.as_str(), staged));
            if ui
                .selectable_label(is_selected, shorten(&file.path))
                .on_hover_text(&file.path)
                .clicked()
            {
                *action = Some(Action::Select { path: file.path.clone(), staged });
            }
        });
    }
}

/// Deja visible el nombre del fichero y recorta los directorios intermedios.
fn shorten(path: &str) -> String {
    const MAX: usize = 38;
    if path.chars().count() <= MAX {
        return path.to_string();
    }
    let tail: String = path.chars().rev().take(MAX - 2).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…/{}", tail.trim_start_matches('/'))
}

fn show_diff(
    ui: &mut egui::Ui,
    selected: Option<(&str, bool)>,
    diff: Option<&FileDiff>,
    action: &mut Option<Action>,
) {
    let Some((path, staged)) = selected else {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("Selecciona un fichero para ver su diff").weak());
        });
        return;
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(path).monospace().strong());
        ui.label(
            RichText::new(if staged { "índice" } else { "árbol de trabajo" })
                .weak()
                .small(),
        );
        if let Some(diff) = diff {
            let (added, removed) = diff.counts();
            ui.label(RichText::new(format!("+{added}")).color(ADDED).small());
            ui.label(RichText::new(format!("−{removed}")).color(REMOVED).small());
        }
    });
    ui.separator();

    let Some(diff) = diff else {
        ui.label(RichText::new("sin diff").weak());
        return;
    };

    if diff.binary {
        ui.label(RichText::new("Fichero binario").weak());
        return;
    }

    egui::ScrollArea::both()
        .id_salt("diff")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;

            for hunk in &diff.hunks {
                // Cada hunk se puede mover por su cuenta, que es lo que hace
                // `git add -p` sin tener que contestar preguntas en la terminal.
                ui.horizontal(|ui| {
                    let label = if staged { "− Quitar" } else { "+ Preparar" };
                    let hint = if staged {
                        "Devolver solo este hunk al árbol de trabajo"
                    } else {
                        "Preparar solo este hunk"
                    };

                    if ui.small_button(label).on_hover_text(hint).clicked() {
                        let path = path.to_string();
                        *action = Some(if staged {
                            Action::UnstageHunk { path, hunk: hunk.id }
                        } else {
                            Action::StageHunk { path, hunk: hunk.id }
                        });
                    }

                    if !hunk.header.is_empty() {
                        ui.label(
                            RichText::new(&hunk.header)
                                .monospace()
                                .color(Color32::from_rgb(0x7A, 0xA6, 0xD6)),
                        );
                    }

                    let (added, removed) = hunk.counts();
                    ui.label(RichText::new(format!("+{added}")).small().color(ADDED));
                    ui.label(RichText::new(format!("−{removed}")).small().color(REMOVED));
                });

                diff_view::lines(ui, hunk);
                ui.add_space(6.0);
            }
        });
}
