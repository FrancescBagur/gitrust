//! Panel derecho: el detalle del commit seleccionado en el grafo.
//!
//! Se lee de arriba abajo en tres tramos —qué se hizo, en qué ficheros, y el
//! cambio línea a línea—, separados por un pelo en vez de por cajas.

use chrono::{Local, TimeZone};
use eframe::egui::{self, Color32, PopupCloseBehavior, RichText};
use gitrust_core::{Block, ChangeKind, CommitDetail, FileDiff};

use crate::{diff_view, theme};

/// Lo que se ha pedido sobre el commit que se está mirando.
pub enum Pick {
    /// Enseñar el cambio de ese fichero.
    File(String),
    /// Ponerle esta etiqueta.
    Tag { name: String, message: String },
}

/// La etiqueta que se está escribiendo, que tiene que seguir ahí al
/// fotograma siguiente.
#[derive(Default)]
pub struct TagDraft {
    name: String,
    message: String,
}

fn kind_color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added => theme::ADDED,
        ChangeKind::Deleted => theme::REMOVED,
        ChangeKind::Renamed => theme::lane(0),
        ChangeKind::TypeChange => theme::lane(6),
        ChangeKind::Conflicted => theme::REMOVED,
        _ => theme::CHANGED,
    }
}

/// Devuelve lo que se haya pedido sobre este commit.
pub fn show(
    ui: &mut egui::Ui,
    detail: Option<&CommitDetail>,
    selected_file: Option<&str>,
    diff: Option<&FileDiff>,
    tag: &mut TagDraft,
) -> Option<Pick> {
    let Some(detail) = detail else {
        theme::empty(ui, "Elige un commit del grafo para verlo aquí");
        return None;
    };

    let mut picked = None;

    ui.add_space(12.0);

    // El asunto manda: es lo único a tamaño de titular en toda la ventana.
    ui.label(
        RichText::new(&detail.summary)
            .font(theme::strong(15.0))
            .color(theme::TEXT),
    );

    ui.add_space(6.0);
    let when = Local
        .timestamp_opt(detail.time, 0)
        .single()
        .map(|dt| dt.format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_default();

    ui.horizontal(|ui| {
        // El correo se guarda para el aviso del ratón: identifica a quien
        // firma, pero repite lo que el nombre ya dice y no cabe al lado de la
        // fecha.
        ui.label(
            RichText::new(&detail.author)
                .font(theme::medium(12.0))
                .color(theme::MUTED),
        )
        .on_hover_text(&detail.email);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(&detail.short_id)
                    .font(theme::mono(11.5))
                    .color(theme::FAINT),
            );
            if detail.is_merge() {
                ui.label(
                    RichText::new("fusión")
                        .font(theme::medium(10.5))
                        .color(theme::lane(6))
                        .background_color(theme::tint(theme::lane(6), 38)),
                );
            }
            ui.label(
                RichText::new(when)
                    .font(theme::mono(11.0))
                    .color(theme::FAINT),
            );
        });
    });

    // El cuerpo se reparte en bloques antes de pintarlo: la prosa se vuelve a
    // plegar al ancho del panel, y las listas, el código y los trailers se
    // quedan como estaban, en mono, porque ahí el salto de línea significa
    // algo.
    if !detail.body.is_empty() {
        ui.add_space(10.0);
        for block in gitrust_core::blocks(&detail.body) {
            let text = match &block {
                Block::Prose(text) => RichText::new(text).font(theme::body(12.5)),
                Block::Literal(text) => RichText::new(text).font(theme::mono(11.0)),
            };
            ui.add(egui::Label::new(text.color(theme::MUTED)).wrap());
            ui.add_space(7.0);
        }
    }

    // Lo que se le puede hacer a este commit. De momento una cosa, pero la
    // fila está puesta para las que vengan.
    ui.add_space(9.0);
    if let Some(action) = tag_menu(ui, tag) {
        picked = Some(action);
    }

    theme::rule(ui);

    let (added, modified, deleted) = detail.counts();
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(
            RichText::new(theme::files_label(detail.files.len()))
                .font(theme::medium(11.5))
                .color(theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (count, word, color) in [
                (deleted, "borrados", theme::REMOVED),
                (modified, "modificados", theme::CHANGED),
                (added, "nuevos", theme::ADDED),
            ] {
                if count > 0 {
                    ui.label(
                        RichText::new(format!("{count} {word}"))
                            .font(theme::body(11.0))
                            .color(color),
                    );
                }
            }
        });
    });
    ui.add_space(4.0);

    // La lista de ficheros ocupa como mucho un tercio del panel; el resto es
    // para el diff, que es lo que se lee de verdad.
    let list_height = (ui.available_height() * 0.35).clamp(80.0, 260.0);
    egui::ScrollArea::vertical()
        .id_salt("ficheros-commit")
        .max_height(list_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for file in &detail.files {
                let is_selected = selected_file == Some(file.path.as_str());
                if file_row(ui, &file.path, file.kind, is_selected) {
                    picked = Some(Pick::File(file.path.clone()));
                }
            }
        });

    theme::rule(ui);

    match diff {
        None => theme::empty(ui, "Elige un fichero para ver su cambio"),
        Some(diff) if diff.binary => theme::empty(ui, "Fichero binario"),
        Some(diff) => {
            egui::ScrollArea::both()
                .id_salt("diff-commit")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for hunk in &diff.hunks {
                        diff_view::header(ui, hunk);
                        diff_view::lines(ui, hunk);
                        ui.add_space(8.0);
                    }
                });
        }
    }

    picked
}

/// El desplegable de etiquetar, con el nombre y el para qué.
///
/// Escribir el mensaje o no es lo que decide la clase de etiqueta, igual que
/// en `git tag`. No hay interruptor porque no hace falta: el que quiere una
/// etiqueta anotada es exactamente el que tiene algo que contar sobre ella.
fn tag_menu(ui: &mut egui::Ui, draft: &mut TagDraft) -> Option<Pick> {
    let button = ui.add(egui::Button::new(
        RichText::new("Etiquetar     ").font(theme::body(11.5)).color(theme::MUTED),
    ));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-etiqueta");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }

    egui::popup::popup_below_widget(ui, id, &button, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(266.0);
        ui.label(
            RichText::new("Etiqueta en este commit")
                .font(theme::medium(12.0))
                .color(theme::TEXT),
        );
        ui.add_space(5.0);

        let entry = ui.add(
            egui::TextEdit::singleline(&mut draft.name)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(8.0, 5.0))
                .font(theme::mono(12.0))
                .hint_text(RichText::new("v1.0").color(theme::FAINT)),
        );
        ui.add_space(5.0);
        ui.add(
            egui::TextEdit::multiline(&mut draft.message)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(8.0, 5.0))
                .font(theme::body(11.5))
                .hint_text(RichText::new("qué trae esta versión").color(theme::FAINT)),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Con mensaje sale anotada; sin él, ligera")
                .font(theme::body(10.5))
                .color(theme::FAINT),
        );
        ui.add_space(6.0);

        let entered = entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let pressed = ui
            .add_enabled(!draft.name.trim().is_empty(), egui::Button::new("Crear"))
            .clicked();

        (entered || pressed).then(|| {
            ui.memory_mut(|memory| memory.close_popup());
            Pick::Tag {
                name: std::mem::take(&mut draft.name),
                message: std::mem::take(&mut draft.message),
            }
        })
    })
    .flatten()
}

/// Una fila de la lista de ficheros: la letra de estado en su canal, y la ruta
/// con la carpeta apagada para que el nombre del fichero salte a la vista.
fn file_row(ui: &mut egui::Ui, path: &str, kind: ChangeKind, selected: bool) -> bool {
    let mut clicked = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(kind.tag())
                .font(theme::mono(11.0))
                .color(kind_color(kind)),
        );

        if ui
            .selectable_label(selected, theme::path_text(path, 11.5, selected))
            .on_hover_text(path)
            .clicked()
        {
            clicked = true;
        }
    });

    clicked
}
