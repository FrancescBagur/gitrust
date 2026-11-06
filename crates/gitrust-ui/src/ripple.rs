//! The wave the pointer leaves behind.
//!
//! A row lighting up under the cursor is the oldest trick there is, and it
//! says only one thing: you are here. The graph can say more. A commit sits
//! on a lane with its parents above it and its children below, and that is a
//! line of connected things -- so the pointer landing on one is treated as a
//! tap on a taut string: the dot it hits rings, and the ring travels a couple
//! of rows up and down the history before it dies out.
//!
//! Everything in here is over in under a second and then stops asking for
//! frames. An effect that keeps breathing for as long as the mouse rests
//! somewhere stops being an answer to a movement and becomes a thing in the
//! corner of the eye that has to be ignored, which is worse than no effect at
//! all. Movement earns its place by being brief.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

use crate::theme;

/// How long the whole thing lasts. Past this nothing is moving any more, so
/// it is also how long frames keep being asked for.
pub const LIFE: f32 = 0.95;

// ── The rings going out ────────────────────────────────────────────────────

/// How many rings leave the dot, and how far apart they set off.
const RINGS: usize = 2;
const STAGGER: f32 = 0.20;
/// How long one ring takes to cross its reach and vanish.
const RING_LIFE: f32 = 0.62;
/// How far out a ring gets. Enough to clear the lane it is in and be read as
/// going somewhere, short enough that it is gone before it reaches the text.
const REACH: f32 = 21.0;
/// The ring's opacity as it leaves, out of 255. Half-there: it is an echo of
/// the dot, not a second dot.
const RING_ALPHA: f32 = 150.0;
/// How thick it starts. It thins as it widens, the way a real one does.
const RING_W: f32 = 1.9;

// ── The tremor running along the lane ──────────────────────────────────────

/// How fast the dot shakes, in cycles a second. Fast enough to read as a
/// ring rather than a wobble.
const TREMOR: f32 = 13.0;
/// How long the shake takes to die away.
const DAMPING: f32 = 0.16;
/// How far the struck dot swells and shrinks, as a share of its radius.
const AMPLITUDE: f32 = 0.42;
/// How long the wave takes to reach the next row up or down.
const DELAY: f32 = 0.05;
/// How much of the shake survives each row it travels through.
const CARRY: f32 = 0.45;
/// How many rows away it is still worth drawing. Past the fourth what is
/// left is under a tenth of a pixel.
pub const CARRIES: usize = 4;

// ── The wash across the row ────────────────────────────────────────────────

/// How fast the band fills, in pixels a second. The row is lit from the dot
/// outwards rather than all at once, so that the light reads as coming from
/// the commit and not from the cursor.
const WASH_SPEED: f32 = 4200.0;
/// How soft the front of the wash is.
const FEATHER: f32 = 110.0;
/// The crest riding the front, out of 255, in the lane's own color.
const CREST_ALPHA: f32 = 26.0;
/// How many strips the band is cut into to fake the gradient. Forty is past
/// the point where the steps can be seen on a row this short.
const STRIPS: usize = 40;

/// How long the pointer has been on `row`, or `None` if it is on no row.
///
/// The row it was on last frame and the clock reading when it got there are
/// kept in egui's scratch memory under `id`; moving to another row, or off
/// the list, starts the count again. Frames are asked for here, where we
/// know whether anything is still moving, so that a window nobody is
/// pointing at goes quiet.
pub fn age(ui: &egui::Ui, id: egui::Id, row: Option<usize>) -> Option<f32> {
    let now = ui.input(|input| input.time);
    let before: Option<(usize, f64)> = ui.data(|data| data.get_temp(id));

    // `usize::MAX` is no row: it is written rather than the entry removed so
    // that coming back to the row just left starts a new wave instead of
    // resuming the old one.
    let Some(row) = row else {
        ui.data_mut(|data| data.insert_temp(id, (usize::MAX, now)));
        return None;
    };

    let born = match before {
        Some((was, born)) if was == row => born,
        _ => {
            ui.data_mut(|data| data.insert_temp(id, (row, now)));
            now
        }
    };

    let age = (now - born) as f32;
    if age < LIFE {
        ui.ctx().request_repaint();
    }
    Some(age)
}

/// How much a dot `away` rows from the struck one is swollen at `age`,
/// as a share of its radius: positive while it is fat, negative while it is
/// thin, and settling on zero.
///
/// A struck string's note, which is the shape this borrows, is a sine that
/// loses height on a curve. Starting it at the zero crossing means the dot
/// grows into the first swing instead of jumping to it.
pub fn swell(age: f32, away: usize) -> f32 {
    let age = age - away as f32 * DELAY;
    if age < 0.0 {
        return 0.0;
    }
    let height = AMPLITUDE * CARRY.powi(away as i32) * (-age / DAMPING).exp();
    height * (std::f32::consts::TAU * TREMOR * age).sin()
}

/// The rings leaving a dot, which is the part that is actually a wave.
///
/// `from` is where the first one starts: just clear of the dot's own halo,
/// or the ring would be born already covered.
pub fn rings(painter: &egui::Painter, center: Pos2, age: f32, color: Color32, from: f32) {
    for ring in 0..RINGS {
        let through = (age - ring as f32 * STAGGER) / RING_LIFE;
        if !(0.0..1.0).contains(&through) {
            continue;
        }

        // It leaves fast and crawls to its end, which is what a ring on water
        // looks like and what a ring at a steady speed does not.
        let out = 1.0 - (1.0 - through).powi(3);
        let left = 1.0 - through;
        let fade = left * left;

        painter.circle_stroke(
            center,
            from + out * REACH,
            Stroke::new(
                (RING_W * fade).max(0.4),
                theme::tint(color, (RING_ALPHA * fade) as u8),
            ),
        );
    }
}

/// The row's background, poured out from the dot rather than switched on.
pub fn wash(painter: &egui::Painter, band: Rect, from: f32, age: f32, color: Color32) {
    // Once the wave is done the band is the flat hover color the rest of the
    // window uses. Said here rather than left to the arithmetic, because on a
    // window wider than the front ever travels the arithmetic would leave a
    // dark strip at the far end for ever.
    if age >= LIFE {
        painter.rect_filled(band, 0.0, theme::HOVER);
        return;
    }

    let front = age * WASH_SPEED;

    let mut mesh = egui::Mesh::default();
    for strip in 0..=STRIPS {
        let x = band.left() + (strip as f32 / STRIPS as f32) * band.width();

        // How far the wash has got past this point, from nothing to all of it.
        let filled = ((front - (x - from).abs()) / FEATHER).clamp(0.0, 1.0);
        // And the crest, which only exists where the front is right now: it
        // peaks halfway through the feather and is gone once it has passed.
        let crest = (1.0 - (filled * 2.0 - 1.0).abs()).max(0.0);

        let shade = blend(
            theme::tint(theme::HOVER, (255.0 * filled) as u8),
            theme::tint(color, (CREST_ALPHA * crest) as u8),
        );

        mesh.colored_vertex(Pos2::new(x, band.top()), shade);
        mesh.colored_vertex(Pos2::new(x, band.bottom()), shade);
        if strip > 0 {
            let corner = 2 * strip as u32;
            mesh.add_triangle(corner - 2, corner - 1, corner);
            mesh.add_triangle(corner - 1, corner, corner + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// `over` laid on `under`, both of them see-through.
///
/// The two have to be one color before they are handed to the mesh: a strip
/// painted twice would need its own pair of triangles for each layer, and
/// forty strips is already as many as this is worth.
///
/// `Color32` keeps its channels already multiplied by the alpha, which is
/// what makes this one line per channel instead of three.
fn blend(under: Color32, over: Color32) -> Color32 {
    let rest = 1.0 - over.a() as f32 / 255.0;
    let mix = |under: u8, over: u8| (under as f32 * rest + over as f32) as u8;
    Color32::from_rgba_premultiplied(
        mix(under.r(), over.r()),
        mix(under.g(), over.g()),
        mix(under.b(), over.b()),
        mix(under.a(), over.a()),
    )
}
