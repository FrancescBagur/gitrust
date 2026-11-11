//! The word that something finished.
//!
//! The bottom bar has said it all along, and that turns out not to be enough:
//! it sits at the far edge of the window while the eye is wherever the work
//! was. A fetch that takes two seconds and then writes a line nobody looks at
//! is, as far as anyone can tell, a button that did nothing.
//!
//! So the sentence is said twice. The bar keeps it, because a record of the
//! last thing that happened is worth having. The toast carries it to where it
//! will be read, and then goes away on its own, which is the part that makes
//! it bearable: a notice that has to be dismissed is a second job handed to
//! somebody who only asked for the first.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, RichText};

use crate::theme;

/// How long it stays.
const LIFE: Duration = Duration::from_millis(4200);

/// How long a failure stays. Longer, because it has to be read, and often
/// read twice: it is the only place the reason is said in full.
const FAILED_LIFE: Duration = Duration::from_millis(9000);

/// The rise and the fade as it arrives, and the fade as it leaves.
const IN: f32 = 0.12;
const OUT: f32 = 0.5;

/// Something that has just happened, on its way out.
pub struct Toast {
    text: String,
    failed: bool,
    born: Instant,
}

impl Toast {
    pub fn new(text: String, failed: bool) -> Self {
        Self {
            text,
            failed,
            born: Instant::now(),
        }
    }

    fn life(&self) -> f32 {
        match self.failed {
            true => FAILED_LIFE.as_secs_f32(),
            false => LIFE.as_secs_f32(),
        }
    }

    /// Fades in quickly and out slowly: arriving should catch the eye, and
    /// leaving should not.
    fn opacity(&self) -> f32 {
        let age = self.born.elapsed().as_secs_f32();
        if age < IN {
            return age / IN;
        }
        let left = self.life() - age;
        match left < OUT {
            true => (left / OUT).max(0.0),
            false => 1.0,
        }
    }

    fn gone(&self) -> bool {
        self.born.elapsed().as_secs_f32() >= self.life()
    }
}

/// Paints it, and gives back whether it should stay.
///
/// It sits above the bottom bar on the right, which is where it covers least:
/// the graph and the columns are read from the left, and the one thing on that
/// side of the bar is the sentence this is repeating.
pub fn show(ctx: &egui::Context, toast: &Toast) -> bool {
    if toast.gone() {
        return false;
    }

    // Without this the fade would wait for somebody to move the mouse, and a
    // toast nobody touches would sit there for ever.
    ctx.request_repaint_after(Duration::from_millis(16));

    let color = match toast.failed {
        true => theme::removed(),
        false => theme::added(),
    };
    // A few pixels of rise as it comes in. It is the only movement in the
    // window, so it is enough to say "this is new" without being a show.
    let rise = 6.0 * (1.0 - (toast.born.elapsed().as_secs_f32() / IN).min(1.0));

    let mut keep = true;
    egui::Area::new(egui::Id::new("toast"))
        .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -46.0 + rise))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.multiply_opacity(toast.opacity());

            let frame = egui::Frame::NONE
                .fill(theme::raised())
                .inner_margin(egui::Margin::symmetric(14, 10))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                .stroke(egui::Stroke::new(1.0, theme::edge()));

            let painted = frame.show(ui, |ui| {
                ui.set_max_width(340.0);
                ui.label(
                    RichText::new(&toast.text)
                        .font(theme::body(12.0))
                        .color(theme::text()),
                );
            });

            // The bar down the edge says how it went before the sentence is
            // read: the same green and red the diffs use, and the same bar the
            // card you are on wears on the board.
            let rect = painted.response.rect;
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height()));
            ui.painter().rect_filled(bar, 0.0, color);

            let response = painted.response.interact(egui::Sense::click());
            if response.clicked() {
                keep = false;
            }
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        });

    keep
}
