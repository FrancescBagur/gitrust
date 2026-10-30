//! Panel derecho: el detalle del commit seleccionado en el grafo.

use chrono::{Local, TimeZone};
use eframe::egui::{self, Color32, RichText};
use gitrust_core::{ChangeKind, CommitDetail, FileDiff};

use crate::diff_view;

fn kind_color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added => diff_view::ADDED,
        ChangeKind::Deleted => diff_view::REMOVED,
        ChangeKind::Renamed => Color32::from_rgb(0x55, 0xA2, 0xF7),
        ChangeKind::TypeChange => Color32::from_rgb(0xB0, 0x8E, 0xF5),
        ChangeKind::Conflicted => Color32::from_rgb(0xFF, 0x6B, 0x6B),
        _ => Color32::from_rgb(0xF2, 0x9D, 0x38),
    }
}

/// Devuelve el fichero pulsado, si hay alguno.
pub fn show(
    ui: &mut egui::Ui,
    detail: Option<&CommitDetail>,
    selected_file: Option<&str>,
    diff: Option<&FileDiff>,
) -> Option<String> {
    let Some(detail) = detail else {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(RichText::new("Selecciona un commit del grafo").weak());
        });
        return None;
    };

    let mut clicked = None;

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(&detail.short_id).monospace().weak());
        if detail.is_merge() {
            ui.label(
                RichText::new("fusión")
                    .small()
                    .color(Color32::from_rgb(0x9A, 0xA6, 0xB2)),
            );
        }
    });

    ui.label(RichText::new(&detail.summary).strong().size(14.0));

    if !detail.body.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(&detail.body).monospace().color(Color32::from_gray(175)));
    }

    ui.add_space(6.0);
    let when = Local
        .timestamp_opt(detail.time, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    ui.label(
        RichText::new(format!("{} <{}> · {}", detail.author, detail.email, when))
            .small()
            .weak(),
    );

    ui.add_space(8.0);
    ui.separator();

    let (added, modified, deleted) = detail.counts();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} ficheros", detail.files.len())).small().strong());
        if added > 0 {
            ui.label(RichText::new(format!("{added} nuevos")).small().color(diff_view::ADDED));
        }
        if modified > 0 {
            ui.label(
                RichText::new(format!("{modified} modificados"))
                    .small()
                    .color(Color32::from_rgb(0xF2, 0x9D, 0x38)),
            );
        }
        if deleted > 0 {
            ui.label(RichText::new(format!("{deleted} borrados")).small().color(diff_view::REMOVED));
        }
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
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(file.kind.tag())
                            .monospace()
                            .color(kind_color(file.kind)),
                    );
                    let is_selected = selected_file == Some(file.path.as_str());
                    if ui
                        .selectable_label(is_selected, RichText::new(&file.path).small())
                        .on_hover_text(&file.path)
                        .clicked()
                    {
                        clicked = Some(file.path.clone());
                    }
                });
            }
        });

    ui.separator();

    match diff {
        None => {
            ui.add_space(8.0);
            ui.label(RichText::new("Selecciona un fichero para ver su diff").weak().small());
        }
        Some(diff) if diff.binary => {
            ui.add_space(8.0);
            ui.label(RichText::new("Fichero binario").weak().small());
        }
        Some(diff) => {
            egui::ScrollArea::both()
                .id_salt("diff-commit")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for hunk in &diff.hunks {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&hunk.header)
                                    .monospace()
                                    .color(diff_view::HUNK_HEADER),
                            );
                            let (added, removed) = hunk.counts();
                            diff_view::counts(ui, added, removed);
                        });
                        diff_view::lines(ui, hunk);
                        ui.add_space(6.0);
                    }
                });
        }
    }

    clicked
}
