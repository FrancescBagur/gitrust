//! The wave the pointer leaves behind.
//!
//! A row lighting up under the cursor is the oldest trick there is, and it
//! says only one thing: you are here. The graph can say more. A commit sits
//! on a lane with its parents above it and its children below, and that is a
//! line of connected things -- so the pointer landing on one is treated as a
//! tap on a taut string. The lane bends and runs away up and down the
//! history as a snake, smallest the further it gets from the hand; the dot
//! it was struck at rings; and the dots the bend passes through ride it.
//!
//! Taps add up: moving on to another row does not cut the last one off, the
//! lane carries the sum of whatever is still ringing, and each dies on its
//! own schedule.
//!
//! Everything in here is over in a second and a half and then stops asking
//! for frames. An effect that keeps breathing for as long as the mouse rests
//! somewhere stops being an answer to a movement and becomes a thing in the
//! corner of the eye that has to be ignored, which is worse than no effect at
//! all. Movement earns its place by being brief.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

use crate::theme;

/// How long the whole thing lasts. Past this nothing is moving any more, so
/// it is also how long frames keep being asked for. The snake is what sets
/// it: it is the last thing still going.
pub const LIFE: f32 = 1.5;

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

// ── The snake running down the lane ───────────────────────────────────────

/// How far sideways the lane is pushed where it was struck, in pixels. A
/// lane is seventeen wide, so this is a bend of about a lane from side to
/// side: as much as can be taken before the lane reads as having moved
/// rather than as having been pushed.
const SWAY: f32 = 8.0;
/// How far up and down the history the snake runs, in rows. This is the
/// "more to less": it stands tallest at the hand and tapers to nothing at
/// the end of its reach, which is what makes it look like something being
/// shaken from one end rather than a decoration laid over the lane.
const LENGTH: f32 = 14.0;
/// How long one bend is, in pixels: six rows, so three dots lean one way
/// and the next three the other.
///
/// This is set against the row height, not picked for looks. A bend of four
/// rows is close enough to the spacing of the dots that consecutive ones
/// land on opposite sides of the lane, and a column of dots alternating
/// left and right is not read as a wave at all -- it is read as a jagged
/// line. Six rows puts three dots on each side of each bend, which is the
/// fewest that still looks like one thing moving.
const BEND: f32 = 168.0;
/// How many times a second a point on the lane swings. Lowered along with
/// the bend above, so that the crests still travel down the lane at about
/// the speed they did when the bends were shorter.
const UNDULATION: f32 = 2.4;
/// How fast the snake lays itself out from the dot, in pixels a second, and
/// how soft its far end is while it does. It is not a pulse crawling away --
/// the whole length is there inside a third of a second -- but it does not
/// arrive everywhere in the same frame either, which would read as a switch
/// being thrown rather than as something let loose.
const ONSET: f32 = 1800.0;
const ONSET_SOFT: f32 = 110.0;
/// How far the lane may be pushed in the end, in pixels, however many taps
/// are adding up, and the most a dot may swell, as a share of its radius.
///
/// Set close to what a single tap asks for, on purpose. A pile of taps is
/// meant to read as more waves in more places, not as one enormous one:
/// dragging the pointer down a lane gets a couple of pixels more bend than
/// tapping it once, and everything past that is swallowed.
const CEILING: f32 = 8.0;
const SWELL_CEILING: f32 = 0.5;
/// How many points a row's worth of lane is cut into while it is bending.
/// A bend is four rows long, so this is near thirty points to a bend: past
/// the number where another would show.
pub const JOINTS: usize = 7;

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

/// One tap that has not finished yet: the row it landed on, and how long
/// ago it did.
pub type Live = (usize, f32);

/// How many taps are carried at once.
///
/// Running the pointer down the list is a second's work and lands a tap a
/// row, so there has to be a cap or a drag would be paying for fifty waves.
/// The one dropped is the oldest, which is the one with least left in it.
///
/// The cap is set loose rather than tight, because dropping a tap is the
/// very thing this is here to avoid. At half a dozen, a quick drag threw
/// away waves that were still half alive -- the same cut, moved rather than
/// mended. Sixteen takes a drag fast enough that the sum is against its
/// ceiling anyway, and against the ceiling losing the oldest moves the lane
/// by a tenth of a pixel.
const KEPT: usize = 16;

/// Every tap still ringing, oldest first, after landing a new one if the
/// pointer has just arrived somewhere.
///
/// The taps add up rather than replace each other. Moving to a new row while
/// the last row is still ringing used to cut it off, and cutting a wave off
/// mid-swing is the one thing that makes the whole effect read as a trick:
/// real things that are moving do not stop because you looked away. So the
/// lane carries the sum of whatever is still going, and each tap dies on its
/// own schedule.
///
/// They are kept in egui's scratch memory under `id`, along with the row the
/// pointer was on last frame -- which is what says a tap has happened, and
/// is held apart from the list so that leaving a row and coming back to it
/// taps it again instead of finding it already there.
pub fn waves(ui: &egui::Ui, id: egui::Id, row: Option<usize>) -> Vec<Live> {
    let now = ui.input(|input| input.time);
    let on = id.with("on");

    let mut struck: Vec<(usize, f64)> = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    struck.retain(|(_, born)| ((now - born) as f32) < LIFE);

    let before: usize = ui.data(|data| data.get_temp(on)).unwrap_or(usize::MAX);
    // `usize::MAX` is no row, so that moving off the list and back on again
    // counts as arriving.
    let arrived = row.unwrap_or(usize::MAX);
    if arrived != before {
        if let Some(row) = row {
            struck.push((row, now));
        }
    }
    if struck.len() > KEPT {
        struck.drain(..struck.len() - KEPT);
    }

    ui.data_mut(|data| {
        data.insert_temp(id, struck.clone());
        data.insert_temp(on, arrived);
    });

    // Frames are asked for here, where we know whether anything is still
    // moving, so that a window nobody is pointing at goes quiet.
    if !struck.is_empty() {
        ui.ctx().request_repaint();
    }

    struck
        .into_iter()
        .map(|(row, born)| (row, (now - born) as f32))
        .collect()
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

/// How far sideways the lane is pushed, `along` pixels from where it was
/// struck, at `age`.
///
/// Four things multiplied, and each of them is one sentence of the effect.
/// The taper is the shape: tall at the hand, nothing at the far end. The
/// laying-out is the snake arriving instead of being switched on. The
/// fading is it giving up. And the sine is the snake itself -- its
/// phase turns with time and lags with distance, which is the whole trick:
/// every point swings the same way, each a little behind the one before it,
/// and a line of swings out of step is a snake.
pub fn sway(age: f32, along: f32) -> f32 {
    let taper = 1.0 - along / (LENGTH * theme::ROW_H);
    if taper <= 0.0 {
        return 0.0;
    }
    let laid = ((age * ONSET - along) / ONSET_SOFT).clamp(0.0, 1.0);
    let phase = std::f32::consts::TAU * (age * UNDULATION - along / BEND);

    // Squared, and measured against the life rather than being an
    // exponential's half-life, because this is the one thing that has to be
    // exactly nothing by the time the last frame is asked for. An
    // exponential is never quite zero, and a snake frozen mid-swing is just
    // a lane drawn crooked.
    let fading = (1.0 - age / LIFE).max(0.0);

    SWAY * taper * laid * fading * fading * phase.sin()
}

/// The sum of what several taps are asking for, held to what the graph can
/// take: `bound` for how far the lane is pushed, `bound_swell` for how far
/// a dot is blown up.
///
/// Waves add, and a pointer dragged down a lane lands a tap a row. Six of
/// them nearly in step come to some twenty-six pixels, which is not a bend
/// any more, it is the lane somewhere else -- and a dot asked to grow by
/// two and a half of its radius is asked, half a swing later, to have a
/// negative one.
///
/// Neither is a clamp. A clamp flattens the crest, and a flat crest reads
/// as a drawing fault rather than as a big wave. This is a curve that is
/// itself for the first few pixels and bends over towards the ceiling
/// without ever arriving: one tap passes through untouched, a pile of them
/// is caught.
pub fn bound(sway: f32) -> f32 {
    CEILING * (sway / CEILING).tanh()
}

pub fn bound_swell(swell: f32) -> f32 {
    SWELL_CEILING * (swell / SWELL_CEILING).tanh()
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
