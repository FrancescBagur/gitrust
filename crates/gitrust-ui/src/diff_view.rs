//! The shared painting of a diff's lines.
//!
//! The working tree's changes panel and a commit's detail panel both use it;
//! they differ only in what surrounds the lines.
//!
//! Every line is painted by hand rather than with a label: that way the
//! background of what was added and taken away runs edge to edge, instead of
//! hugging the text and leaving a ragged border on the right. It also leaves
//! room for a gutter with both sides' line numbers, which is what you need in
//! order to place a change.

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Stroke};
use gitrust_core::Hunk;

use crate::theme;

pub const ADDED: Color32 = theme::ADDED;
pub const REMOVED: Color32 = theme::REMOVED;

const LINE_H: f32 = 17.0;
const NUM_W: f32 = 34.0;
const GUTTER: f32 = NUM_W * 2.0 + 10.0;

/// A hunk's `@@ ... @@` header, with its count.
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

/// A hunk's lines, each with its band of color running the full width.
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
            // An edge of the full color marks the side of the change without
            // repeating the sign through contrast.
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

    // The gutter is set off from the code with a hair, not with a box.
    painter.vline(
        rect.left() + GUTTER - 6.0,
        rect.top()..=rect.bottom(),
        Stroke::new(1.0, theme::HAIRLINE),
    );
}

/// The hunk's or the file's `+n -n`.
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
