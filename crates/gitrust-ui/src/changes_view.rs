//! Panel de cambios: listas de índice y árbol de trabajo, diff y commit.

use eframe::egui::{self, Color32, RichText};
use gitrust_core::{ChangeKind, FileChange, FileDiff, HunkId, WorkingState, SUBJECT_LIMIT};

use crate::{diff_view, theme};

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
    Amend,
    /// El interruptor de enmendar ha cambiado de postura.
    AmendToggled,
}

/// El mensaje que se está escribiendo, y en qué commit va a acabar.
pub struct Draft<'a> {
    pub subject: &'a mut String,
    pub body: &'a mut String,
    /// Si lo escrito va a rehacer el último commit en vez de crear otro.
    pub amend: &'a mut bool,
    /// Hay un commit que enmendar. Sin él, el interruptor no se enseña.
    pub amendable: bool,
    /// El remoto que ya tiene el último commit, si lo tiene. Enmendarlo
    /// entonces obliga a forzar el siguiente envío, y eso hay que decirlo
    /// antes, no después.
    pub pushed: Option<&'a str>,
}

fn kind_color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added => theme::ADDED,
        ChangeKind::Modified => theme::CHANGED,
        ChangeKind::Deleted => theme::REMOVED,
        ChangeKind::Renamed => theme::lane(0),
        ChangeKind::TypeChange => theme::lane(6),
        ChangeKind::Untracked => theme::MUTED,
        ChangeKind::Conflicted => theme::REMOVED,
    }
}

pub fn show(
    ui: &mut egui::Ui,
    state: &WorkingState,
    selected: Option<(&str, bool)>,
    diff: Option<&FileDiff>,
    draft: &mut Draft<'_>,
) -> Option<Action> {
    let mut action = None;

    egui::SidePanel::left("cambios")
        .resizable(true)
        .default_width(330.0)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(10, 0)),
        )
        .show_inside(ui, |ui| {
            // El cuadro de commit va abajo del todo, fijo.
            egui::TopBottomPanel::bottom("commit")
                .resizable(false)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::PANEL)
                        .inner_margin(egui::Margin::symmetric(0, 10)),
                )
                .show_inside(ui, |ui| commit_box(ui, state, draft, &mut action));

            egui::ScrollArea::vertical()
                .id_salt("listas")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(10.0);
                    section(ui, "Preparado", &state.staged, selected, true, &mut action);
                    ui.add_space(14.0);
                    section(ui, "Sin preparar", &state.unstaged, selected, false, &mut action);
                    ui.add_space(10.0);
                });
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::NONE
                .fill(theme::CANVAS)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show_inside(ui, |ui| {
            show_diff(ui, selected, diff, &mut action);
        });

    action
}

/// El mismo panel en una sola columna, para el lado derecho de la historia.
///
/// Es lo que sale al elegir en el grafo la fila de cambios sin confirmar: las
/// dos listas, el cuadro de commit y el diff, uno debajo de otro, de modo que
/// se pueda preparar y commitear sin salir del grafo.
pub fn compact(
    ui: &mut egui::Ui,
    state: &WorkingState,
    selected: Option<(&str, bool)>,
    diff: Option<&FileDiff>,
    draft: &mut Draft<'_>,
) -> Option<Action> {
    let mut action = None;

    ui.add_space(12.0);
    ui.label(
        RichText::new("Cambios sin confirmar")
            .font(theme::strong(15.0))
            .color(theme::TEXT),
    );
    ui.add_space(3.0);
    let note = theme::pending_label(state.staged.len(), state.unstaged.len());
    ui.label(
        RichText::new(if note.is_empty() { "El árbol está limpio".to_string() } else { note })
            .font(theme::body(11.5))
            .color(theme::MUTED),
    );

    theme::rule(ui);

    // Las listas se quedan en un tercio del panel; el resto es para el cuadro
    // de commit y el diff, que es donde se trabaja.
    let list_height = (ui.available_height() * 0.34).clamp(90.0, 300.0);
    egui::ScrollArea::vertical()
        .id_salt("listas-estrechas")
        .max_height(list_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            section(ui, "Preparado", &state.staged, selected, true, &mut action);
            ui.add_space(12.0);
            section(ui, "Sin preparar", &state.unstaged, selected, false, &mut action);
        });

    theme::rule(ui);
    commit_box(ui, state, draft, &mut action);
    theme::rule(ui);
    show_diff(ui, selected, diff, &mut action);

    action
}

/// El cuadro de commit: título y descripción por separado, como espera Git.
fn commit_box(
    ui: &mut egui::Ui,
    state: &WorkingState,
    draft: &mut Draft<'_>,
    action: &mut Option<Action>,
) {
    // El asunto va aparte del cuerpo: Git espera una línea en blanco entre
    // ambos y así no hay que recordarlo.
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Título")
                .font(theme::medium(11.5))
                .color(theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let length = draft.subject.chars().count();
            let color = if length > SUBJECT_LIMIT {
                theme::CHANGED
            } else {
                theme::FAINT
            };
            ui.label(
                RichText::new(format!("{length}/{SUBJECT_LIMIT}"))
                    .font(theme::mono(10.5))
                    .color(color),
            );
        });
    });
    ui.add_space(3.0);
    ui.add(
        egui::TextEdit::singleline(draft.subject)
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(8.0, 6.0))
            .font(theme::body(12.5))
            .hint_text(RichText::new("feat: resumen en una línea").color(theme::FAINT)),
    );

    ui.add_space(8.0);
    ui.label(
        RichText::new("Descripción")
            .font(theme::medium(11.5))
            .color(theme::MUTED),
    );
    ui.add_space(3.0);
    ui.add(
        egui::TextEdit::multiline(draft.body)
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(8.0, 6.0))
            .font(theme::mono(11.5))
            .hint_text(RichText::new("Qué cambia y por qué").color(theme::FAINT)),
    );
    ui.add_space(8.0);

    // Enmendando no hace falta tener nada preparado: cambiar solo el mensaje
    // del último commit es la mitad de las veces que se enmienda.
    let amending = *draft.amend;
    let ready = !draft.subject.trim().is_empty() && (amending || !state.staged.is_empty());
    let shortcut =
        ready && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));

    ui.horizontal(|ui| {
        let label = if amending { "Enmendar" } else { "Commit" };
        let button = egui::Button::new(RichText::new(label).font(theme::medium(13.0)))
            .fill(if ready { theme::tint(theme::lane(0), 54) } else { theme::RAISED })
            .min_size(egui::vec2(88.0, 28.0));

        if ui
            .add_enabled(ready, button)
            .on_hover_text("Ctrl+Enter")
            .clicked()
            || shortcut
        {
            *action = Some(if amending { Action::Amend } else { Action::Commit });
        }

        // Si no se puede commitear, el aviso dice qué falta en vez de
        // limitarse a describir el estado.
        let note = if !amending && state.staged.is_empty() {
            "Prepara algún cambio primero".to_string()
        } else if draft.subject.trim().is_empty() {
            "Falta el título".to_string()
        } else if state.staged.is_empty() {
            "Solo cambiará el mensaje".to_string()
        } else {
            theme::files_label(state.staged.len())
        };
        ui.label(
            RichText::new(note)
                .font(theme::body(11.5))
                .color(theme::FAINT),
        );

        if draft.amendable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let text = RichText::new("Enmendar el último")
                    .font(theme::body(11.5))
                    .color(if amending { theme::TEXT } else { theme::MUTED });

                if ui
                    .checkbox(draft.amend, text)
                    .on_hover_text("Rehace el último commit en vez de escribir otro")
                    .changed()
                {
                    *action = Some(Action::AmendToggled);
                }
            });
        }
    });

    // Enmendar un commit que ya salió deja dos historias distintas con el
    // mismo nombre. Se puede hacer, pero no a ciegas.
    if let (true, Some(remote)) = (amending, draft.pushed) {
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Ese commit ya está en {remote}: el próximo push habrá que forzarlo"
            ))
            .font(theme::body(11.0))
            .color(theme::CHANGED),
        );
    }
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
        ui.add_space(2.0);
        ui.label(
            RichText::new(title)
                .font(theme::medium(11.5))
                .color(theme::MUTED),
        );
        ui.label(
            RichText::new(files.len().to_string())
                .font(theme::mono(10.5))
                .color(theme::FAINT),
        );

        if !files.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if staged { "Quitar todo" } else { "Preparar todo" };
                if ui
                    .add(egui::Button::new(
                        RichText::new(label).font(theme::body(11.0)).color(theme::MUTED),
                    ))
                    .clicked()
                {
                    *action = Some(if staged { Action::UnstageAll } else { Action::StageAll });
                }
            });
        }
    });
    ui.add_space(4.0);

    if files.is_empty() {
        let note = if staged {
            "El índice está vacío"
        } else {
            "Nada sin preparar"
        };
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            ui.label(
                RichText::new(note)
                    .font(theme::body(11.5))
                    .color(theme::FAINT),
            );
        });
        return;
    }

    for file in files {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let button = if staged { "−" } else { "+" };
            let hint = if staged { "Quitar del índice" } else { "Preparar" };
            if ui
                .add(
                    egui::Button::new(RichText::new(button).font(theme::mono(12.0)))
                        .min_size(egui::vec2(22.0, 20.0)),
                )
                .on_hover_text(hint)
                .clicked()
            {
                *action = Some(if staged {
                    Action::Unstage(file.path.clone())
                } else {
                    Action::Stage(file.path.clone())
                });
            }

            ui.label(
                RichText::new(file.kind.tag())
                    .font(theme::mono(11.0))
                    .color(kind_color(file.kind)),
            );

            let is_selected = selected == Some((file.path.as_str(), staged));
            if ui
                .selectable_label(is_selected, theme::path_text(&file.path, 11.5, is_selected))
                .on_hover_text(&file.path)
                .clicked()
            {
                *action = Some(Action::Select { path: file.path.clone(), staged });
            }
        });
    }
}

fn show_diff(
    ui: &mut egui::Ui,
    selected: Option<(&str, bool)>,
    diff: Option<&FileDiff>,
    action: &mut Option<Action>,
) {
    let Some((path, staged)) = selected else {
        theme::empty(ui, "Elige un fichero para ver su cambio");
        return;
    };

    ui.horizontal(|ui| {
        ui.label(theme::path_text(path, 12.5, false));
        ui.label(
            RichText::new(if staged { "en el índice" } else { "en el árbol de trabajo" })
                .font(theme::body(11.0))
                .color(theme::FAINT),
        );
        if let Some(diff) = diff {
            let (added, removed) = diff.counts();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                diff_view::counts(ui, added, removed);
            });
        }
    });
    theme::rule(ui);

    let Some(diff) = diff else {
        theme::empty(ui, "Sin cambios que mostrar");
        return;
    };

    if diff.binary {
        theme::empty(ui, "Fichero binario");
        return;
    }

    egui::ScrollArea::both()
        .id_salt("diff")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;

            for hunk in &diff.hunks {
                // Cada hunk se puede mover por su cuenta, que es lo que hace
                // `git add -p` sin tener que contestar preguntas en la terminal.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    let label = if staged { "− Quitar" } else { "+ Preparar" };
                    let hint = if staged {
                        "Devolver solo este hunk al árbol de trabajo"
                    } else {
                        "Preparar solo este hunk"
                    };

                    if ui
                        .add(egui::Button::new(
                            RichText::new(label).font(theme::body(11.0)),
                        ))
                        .on_hover_text(hint)
                        .clicked()
                    {
                        let path = path.to_string();
                        *action = Some(if staged {
                            Action::UnstageHunk { path, hunk: hunk.id }
                        } else {
                            Action::StageHunk { path, hunk: hunk.id }
                        });
                    }

                    ui.label(
                        RichText::new(hunk.header.trim())
                            .font(theme::mono(11.0))
                            .color(theme::FAINT),
                    );

                    let (added, removed) = hunk.counts();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        diff_view::counts(ui, added, removed);
                    });
                });

                diff_view::lines(ui, hunk);
                ui.add_space(8.0);
            }
        });
}
