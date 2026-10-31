//! Barra lateral de referencias: locales, remotas y etiquetas.
//!
//! Las ramas se agrupan por su carpeta (`feat/`, `fix/`…) igual que hace el
//! árbol de ramas de los clientes gráficos, porque una convención de nombres
//! con barras es lo normal en cuanto el repositorio crece.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText};
use gitrust_core::{Oid, RefEntry, RefKind};

use crate::theme;

/// Lo que se ha pedido desde la barra lateral.
pub enum Pick {
    /// Enseñar en el grafo el commit al que apunta la referencia.
    Show(Oid),
    /// Cambiarse a esa rama.
    Switch { name: String, remote: bool },
    /// Quitar esa etiqueta de aquí.
    DeleteTag(String),
    /// Borrar esa rama: la de aquí, o la del remoto.
    DeleteBranch { name: String, remote: bool },
}

/// Devuelve lo que se haya pedido sobre una referencia.
///
/// Con `busy` en cierto hay una operación en marcha y las ramas dejan de
/// poder pulsarse: cambiar de rama mientras se está trayendo del remoto es
/// pedir dos cosas a la vez sobre los mismos ficheros.
pub fn show(
    ui: &mut egui::Ui,
    refs: &[RefEntry],
    filter: &mut String,
    selected: Option<Oid>,
    busy: bool,
) -> Option<Pick> {
    let mut picked = None;

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
            for (title, kind) in [
                ("Ramas", RefKind::LocalBranch),
                ("Remotas", RefKind::RemoteBranch),
                ("Etiquetas", RefKind::Tag),
            ] {
                section(ui, title, kind, &matching, selected, busy, &mut picked);
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
    busy: bool,
    picked: &mut Option<Pick>,
) {
    let entries: Vec<&RefEntry> = refs.iter().copied().filter(|e| e.kind == kind).collect();
    if entries.is_empty() {
        return;
    }

    theme::section(ui, title, entries.len());

    // El gesto no se ve, así que se dice. Y solo donde sirve: con una sola
    // rama local, la que tienes puesta, no hay adónde cambiar.
    if kind == RefKind::LocalBranch && entries.len() > 1 {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            ui.label(
                RichText::new("Doble clic para cambiar de rama")
                    .font(theme::body(10.5))
                    .color(theme::FAINT),
            );
        });
    }

    ui.add_space(2.0);

    // Las que no tienen carpeta van arriba; el resto, en su grupo.
    let mut folders: BTreeMap<&str, Vec<&RefEntry>> = BTreeMap::new();
    for entry in &entries {
        match entry.folder() {
            Some(folder) => folders.entry(folder).or_default().push(entry),
            None => leaf(ui, entry, selected, busy, picked),
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
                    leaf(ui, entry, selected, busy, picked);
                }
            });
    }

    ui.add_space(10.0);
}

fn leaf(
    ui: &mut egui::Ui,
    entry: &RefEntry,
    selected: Option<Oid>,
    busy: bool,
    picked: &mut Option<Pick>,
) {
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

    // A una rama se puede ir. A la que ya tienes puesta no hay adónde ir, y a
    // una etiqueta todavía no: eso deja HEAD desacoplado, que es un sitio del
    // que hay que saber salir y aquí no se enseña cómo.
    let branch = matches!(entry.kind, RefKind::LocalBranch | RefKind::RemoteBranch);
    let switchable = branch && !entry.is_head && !busy;

    let response = response.on_hover_text(if switchable {
        format!("{}\nDoble clic para cambiar a esta rama", entry.name)
    } else {
        entry.name.clone()
    });

    // Una etiqueta no lleva a ningún sitio, pero se puede quitar de en medio.
    let removable = entry.kind == RefKind::Tag && !busy;

    if switchable || removable {
        let remote = entry.kind == RefKind::RemoteBranch;
        response.context_menu(|ui| {
            if switchable {
                let button = ui.button("Cambiar a esta rama");
                let button = if remote {
                    button.on_hover_text("crea aquí la rama local, si todavía no está")
                } else {
                    button
                };
                if button.clicked() {
                    *picked = Some(Pick::Switch { name: entry.name.clone(), remote });
                    ui.close_menu();
                }
            }

            if switchable {
                let label = if remote {
                    "Borrar la rama del remoto"
                } else {
                    "Borrar la rama"
                };
                let hint = if remote {
                    "se borra en el servidor: lo verá todo el mundo"
                } else {
                    "solo la de aquí; lo que esté en un remoto se queda"
                };
                if ui.button(label).on_hover_text(hint).clicked() {
                    *picked = Some(Pick::DeleteBranch { name: entry.name.clone(), remote });
                    ui.close_menu();
                }
            }

            if removable
                && ui
                    .button("Borrar la etiqueta")
                    .on_hover_text("solo la de aquí; la del remoto se queda donde está")
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
