//! Pintado compartido de las líneas de un diff.
//!
//! Lo usan el panel de cambios del árbol de trabajo y el de detalle de un
//! commit, que solo se diferencian en lo que rodea a las líneas.

use eframe::egui::{self, Color32, RichText};
use gitrust_core::Hunk;

pub const ADDED: Color32 = Color32::from_rgb(0x6D, 0xC9, 0x51);
pub const REMOVED: Color32 = Color32::from_rgb(0xE5, 0x5F, 0x5F);
pub const HUNK_HEADER: Color32 = Color32::from_rgb(0x7A, 0xA6, 0xD6);

/// Las líneas de un hunk, con fondo verde en las añadidas y rojo en las
/// quitadas.
pub fn lines(ui: &mut egui::Ui, hunk: &Hunk) {
    for line in &hunk.lines {
        let (foreground, background) = match line.origin {
            '+' => (
                Color32::from_rgb(0xBC, 0xE8, 0xA8),
                Color32::from_rgb(0x1C, 0x2E, 0x1C),
            ),
            '-' => (
                Color32::from_rgb(0xF0, 0xAE, 0xAE),
                Color32::from_rgb(0x33, 0x1C, 0x1C),
            ),
            _ => (Color32::from_gray(185), Color32::TRANSPARENT),
        };

        ui.label(
            RichText::new(format!("{}{}", line.origin, line.content))
                .monospace()
                .color(foreground)
                .background_color(background),
        );
    }
}

/// `+n −n` del hunk o del fichero.
pub fn counts(ui: &mut egui::Ui, added: usize, removed: usize) {
    ui.label(RichText::new(format!("+{added}")).small().color(ADDED));
    ui.label(RichText::new(format!("−{removed}")).small().color(REMOVED));
}
