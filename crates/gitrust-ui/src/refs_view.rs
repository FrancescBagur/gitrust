//! Barra lateral de referencias: locales, remotas y etiquetas.
//!
//! Las ramas se agrupan por su carpeta (`feat/`, `fix/`…) igual que hace el
//! árbol de ramas de los clientes gráficos, porque una convención de nombres
//! con barras es lo normal en cuanto el repositorio crece.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use gitrust_core::{Oid, RefEntry, RefKind};

use crate::theme;

/// Devuelve el commit al que apunta la referencia pulsada.
pub fn show(
    ui: &mut egui::Ui,
    refs: &[RefEntry],
    filter: &mut String,
    selected: Option<Oid>,
) -> Option<Oid> {
    let mut clicked = None;

    ui.add_space(8.0);
    ui.add(
        egui::TextEdit::singleline(filter)
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(8.0, 5.0))
            .font(theme::body(12.5))
            .hint_text(RichText::new("Buscar una rama").color(theme::FAINT)),
    );
    ui.add_space(8.0);

    let needle = filter.trim().to_lowercase();
    let matching: Vec<&RefEntry> = refs
        .iter()
        .filter(|entry| needle.is_empty() || entry.name.to_lowercase().contains(&needle))
        .collect();

    if refs.is_empty() {
        theme::empty(ui, "Sin referencias");
        return None;
    }

    if matching.is_empty() {
        theme::empty(ui, "Ninguna rama con ese nombre");
        return None;
    }

    egui::ScrollArea::vertical()
        .id_salt("refs")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            section(ui, "Ramas", RefKind::LocalBranch, &matching, selected, &mut clicked);
            section(ui, "Remotas", RefKind::RemoteBranch, &matching, selected, &mut clicked);
            section(ui, "Etiquetas", RefKind::Tag, &matching, selected, &mut clicked);
            ui.add_space(12.0);
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
    if entries.is_empty() {
        return;
    }

    theme::section(ui, title, entries.len());
    ui.add_space(2.0);

    // Las que no tienen carpeta van arriba; el resto, en su grupo.
    let mut folders: BTreeMap<&str, Vec<&RefEntry>> = BTreeMap::new();
    for entry in &entries {
        match entry.folder() {
            Some(folder) => folders.entry(folder).or_default().push(entry),
            None => leaf(ui, entry, selected, clicked),
        }
    }

    for (folder, group) in folders {
        let head = egui::RichText::new(folder)
            .font(theme::body(12.0))
            .color(theme::MUTED);
        egui::CollapsingHeader::new(head)
            .id_salt((title, folder))
            .default_open(group.len() <= 8)
            .show(ui, |ui| {
                for entry in group {
                    leaf(ui, entry, selected, clicked);
                }
            });
    }

    ui.add_space(10.0);
}

fn leaf(ui: &mut egui::Ui, entry: &RefEntry, selected: Option<Oid>, clicked: &mut Option<Oid>) {
    // El color dice qué clase de referencia es: las locales en el azur del
    // grafo, las remotas apagadas porque suelen duplicar a una local, y las
    // etiquetas en el violeta de su pastilla.
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

    if response.on_hover_text(&entry.name).clicked() {
        *clicked = Some(entry.target);
    }
}
