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
//!
//! # Two kinds of colour on the same line
//!
//! A diff already says something with colour -- the band and the edge say
//! what changed -- and the syntax says something else on top of it. They are
//! kept apart by what carries each: the change is the background and the
//! stripe down the side, the code is the text. Lines that did not change get
//! their colour let down, because a diff where everything shouts equally is
//! a diff you have to search.
//!
//! A hunk is read for colour on its own, which is the one thing here that is
//! knowingly approximate: a comment or a string that opened before the hunk
//! began cannot be known from inside it. Reading the whole of both sides of
//! the file to get those few lines right is a great deal of work for a case
//! that looks, when it happens, like a few lines of the wrong colour.

use eframe::egui::{self, text::LayoutJob, Align2, Color32, Pos2, Rect, Stroke, TextFormat};
use gitrust_core::Hunk;

use crate::{syntax, theme};

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
///
/// `path` is only for the colour: it is what says whether `#` starts a
/// comment or an attribute.
pub fn lines(ui: &mut egui::Ui, hunk: &Hunk, path: &str) {
    let font = theme::mono(11.5);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, hunk.lines.len() as f32 * LINE_H),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);

    // Read once for the whole hunk rather than once a line, so that a comment
    // running down several of its lines is one comment. Everything after this
    // is read back by position, so a result that does not line up is not used
    // at all.
    let text: Vec<&str> = hunk
        .lines
        .iter()
        .map(|line| line.content.trim_end_matches(['\n', '\r']))
        .collect();
    let painted = syntax::read_lines(&text, path);
    let painted = (painted.len() == hunk.lines.len()).then_some(painted);

    for (index, line) in hunk.lines.iter().enumerate() {
        let top = rect.top() + index as f32 * LINE_H;
        let band = Rect::from_min_size(
            Pos2::new(rect.left(), top),
            egui::vec2(rect.width(), LINE_H),
        );
        let y = band.center().y;

        // A commit can be thousands of lines and none of this is virtualised.
        // What is off screen is not drawn, and laying out a line is the most
        // expensive thing on this path.
        if !painter.clip_rect().intersects(band) {
            continue;
        }

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

        let left = rect.left() + GUTTER;
        match painted.as_ref().and_then(|all| all.get(index)) {
            Some(pieces) if !pieces.is_empty() => {
                let mut job = LayoutJob::default();
                for piece in pieces {
                    job.append(
                        &piece.text,
                        0.0,
                        TextFormat {
                            font_id: font.clone(),
                            color: shade(piece.kind.color(), line.origin),
                            ..Default::default()
                        },
                    );
                }
                let galley = painter.layout_job(job);
                let top = y - galley.size().y / 2.0;
                painter.galley(Pos2::new(left, top), galley, foreground);
            }
            _ => {
                painter.text(
                    Pos2::new(left, y),
                    Align2::LEFT_CENTER,
                    &line.content,
                    font.clone(),
                    foreground,
                );
            }
        }
    }

    // The gutter is set off from the code with a hair, not with a box.
    painter.vline(
        rect.left() + GUTTER - 6.0,
        rect.top()..=rect.bottom(),
        Stroke::new(1.0, theme::HAIRLINE),
    );
}

/// A syntax colour, as strong as the line it is on deserves.
///
/// Full on a line that changed, let down on one that did not. The lines
/// around a change are there to place it, and a diff where every line is as
/// loud as every other is a diff you have to search rather than read.
fn shade(color: Color32, origin: char) -> Color32 {
    match origin {
        '+' | '-' => color,
        _ => theme::tint(color, 170),
    }
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
