//! Pintado compartido de las líneas de un diff.
//!
//! Lo usan el panel de cambios del árbol de trabajo y el de detalle de un
//! commit, que solo se diferencian en lo que rodea a las líneas.
//!
//! Cada línea se pinta a mano en vez de con una etiqueta: así el fondo de lo
//! añadido y lo quitado llega de borde a borde, en lugar de ceñirse al texto y
//! dejar un borde dentado a la derecha. De paso cabe una canal con los números
//! de línea de los dos lados, que es lo que hace falta para situar un cambio.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Stroke};
use gitrust_core::Hunk;

use crate::theme;

pub const ADDED: Color32 = theme::ADDED;
pub const REMOVED: Color32 = theme::REMOVED;

const LINE_H: f32 = 17.0;
const NUM_W: f32 = 34.0;
const GUTTER: f32 = NUM_W * 2.0 + 10.0;

/// La cabecera `@@ ... @@` de un hunk, con su recuento.
pub fn header(ui: &mut egui::Ui, hunk: &Hunk) {
    let (added, removed) = hunk.counts();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(hunk.header.trim())
                .font(theme::mono(11.0))
                .color(theme::FAINT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            counts(ui, added, removed);
        });
    });
}

/// Las líneas de un hunk, cada una con su banda de color a todo lo ancho.
pub fn lines(ui: &mut egui::Ui, hunk: &Hunk) {
    let font = theme::mono(11.5);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, hunk.lines.len() as f32 * LINE_H),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);

    for (index, line) in hunk.lines.iter().enumerate() {
        let top = rect.top() + index as f32 * LINE_H;
        let band = Rect::from_min_size(
            Pos2::new(rect.left(), top),
            egui::vec2(rect.width(), LINE_H),
        );
        let y = band.center().y;

        let (foreground, background) = match line.origin {
            '+' => (theme::ADDED_FG, theme::ADDED_BG),
            '-' => (theme::REMOVED_FG, theme::REMOVED_BG),
            _ => (theme::MUTED, Color32::TRANSPARENT),
        };

        if background != Color32::TRANSPARENT {
            painter.rect_filled(band, 0.0, background);
            // Un filo del color pleno marca el lado del cambio sin repetir el
            // signo a base de contraste.
            painter.rect_filled(
                Rect::from_min_size(band.min, egui::vec2(2.0, LINE_H)),
                0.0,
                if line.origin == '+' { ADDED } else { REMOVED },
            );
        }

        let number = |value: Option<u32>, right: f32| {
            if let Some(value) = value {
                painter.text(
                    Pos2::new(right, y),
                    Align2::RIGHT_CENTER,
                    value.to_string(),
                    theme::mono(10.5),
                    theme::tint(theme::FAINT, 190),
                );
            }
        };
        number(line.old_lineno, rect.left() + NUM_W);
        number(line.new_lineno, rect.left() + NUM_W * 2.0);

        painter.text(
            Pos2::new(rect.left() + GUTTER, y),
            Align2::LEFT_CENTER,
            &line.content,
            font.clone(),
            foreground,
        );
    }

    // La canal se separa del código con un pelo, no con una caja.
    painter.vline(
        rect.left() + GUTTER - 6.0,
        rect.top()..=rect.bottom(),
        Stroke::new(1.0, theme::HAIRLINE),
    );
}

/// `+n −n` del hunk o del fichero.
pub fn counts(ui: &mut egui::Ui, added: usize, removed: usize) {
    if removed > 0 {
        ui.label(
            egui::RichText::new(format!("−{removed}"))
                .font(theme::mono(10.5))
                .color(REMOVED),
        );
    }
    if added > 0 {
        ui.label(
            egui::RichText::new(format!("+{added}"))
                .font(theme::mono(10.5))
                .color(ADDED),
        );
    }
}
