//! Barra lateral de referencias: locales, remotas y etiquetas.
//!
//! Las ramas se agrupan por su carpeta (`feat/`, `fix/`…) igual que hace el
//! árbol de ramas de los clientes gráficos, porque una convención de nombres
//! con barras es lo normal en cuanto el repositorio crece.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText};
use gitrust_core::{Oid, RefEntry, RefKind};

const HEAD_COLOR: Color32 = Color32::from_rgb(0xFF, 0xD1, 0x4A);
const LOCAL_COLOR: Color32 = Color32::from_rgb(0x7A, 0xC6, 0xFF);
const REMOTE_COLOR: Color32 = Color32::from_gray(150);
const TAG_COLOR: Color32 = Color32::from_rgb(0xC7, 0xA6, 0xF5);

/// Devuelve el commit al que apunta la referencia pulsada.
pub fn show(
    ui: &mut egui::Ui,
    refs: &[RefEntry],
    filter: &mut String,
    selected: Option<Oid>,
) -> Option<Oid> {
    let mut clicked = None;

    ui.add_space(6.0);
    ui.add(
        egui::TextEdit::singleline(filter)
            .desired_width(f32::INFINITY)
            .hint_text("Filtrar ramas"),
    );
    ui.add_space(6.0);

    let needle = filter.trim().to_lowercase();
    let matching: Vec<&RefEntry> = refs
        .iter()
        .filter(|entry| needle.is_empty() || entry.name.to_lowercase().contains(&needle))
        .collect();

    egui::ScrollArea::vertical()
        .id_salt("refs")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            section(ui, "LOCAL", RefKind::LocalBranch, &matching, selected, &mut clicked);
            section(ui, "REMOTAS", RefKind::RemoteBranch, &matching, selected, &mut clicked);
            section(ui, "ETIQUETAS", RefKind::Tag, &matching, selected, &mut clicked);
        });

    clicked
}

fn section(
    ui: &mut egui::Ui,
    title: &str,
    kind: RefKind,
    refs: &[&RefEntry],
    selected: Option<Oid>,
    clicked: &mut Option<Oid>,
) {
    let entries: Vec<&RefEntry> = refs.iter().copied().filter(|e| e.kind == kind).collect();

    ui.horizontal(|ui| {
        ui.label(RichText::new(title).small().strong());
        ui.label(RichText::new(entries.len().to_string()).small().weak());
    });

    if entries.is_empty() {
        ui.label(RichText::new("—").weak().small());
        ui.add_space(8.0);
        return;
    }

    // Las que no tienen carpeta van arriba; el resto, en su grupo.
    let mut folders: BTreeMap<&str, Vec<&RefEntry>> = BTreeMap::new();
    for entry in &entries {
        match entry.folder() {
            Some(folder) => folders.entry(folder).or_default().push(entry),
            None => leaf(ui, entry, selected, clicked),
        }
    }

    for (folder, group) in folders {
        egui::CollapsingHeader::new(RichText::new(folder).small())
            .id_salt((title, folder))
            .default_open(group.len() <= 8)
            .show(ui, |ui| {
                for entry in group {
                    leaf(ui, entry, selected, clicked);
                }
            });
    }

    ui.add_space(8.0);
}

fn leaf(ui: &mut egui::Ui, entry: &RefEntry, selected: Option<Oid>, clicked: &mut Option<Oid>) {
    let color = if entry.is_head {
        HEAD_COLOR
    } else {
        match entry.kind {
            RefKind::LocalBranch => LOCAL_COLOR,
            RefKind::RemoteBranch => REMOTE_COLOR,
            RefKind::Tag => TAG_COLOR,
            RefKind::Head => HEAD_COLOR,
        }
    };

    let label = if entry.is_head {
        format!("● {}", entry.leaf())
    } else {
        entry.leaf().to_string()
    };

    let is_selected = selected == Some(entry.target);
    let response = ui.selectable_label(is_selected, RichText::new(label).color(color));

    if response.on_hover_text(&entry.name).clicked() {
        *clicked = Some(entry.target);
    }
}
