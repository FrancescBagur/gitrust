//! The visual system: color, typography and spacing.
//!
//! One saturated family rules the screen, the one of the graph's lanes,
//! because the shape of the branches is what people come to read. Everything
//! else is grey with a steady cold cast, and only HEAD is allowed an amber.
//! If something new needs color, it comes from here or it does not come.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke};
use gitrust_core::Appearance;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// ── Surfaces ───────────────────────────────────────────────────────────────
// A blue-green slate, not black. The graph's canvas is the deepest, so that
// the lanes stand out against it.

static LIGHT: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

fn choose(dark: Color32, light: Color32) -> Color32 {
    if LIGHT.load(Ordering::Relaxed) { light } else { dark }
}

/// Whether the currently installed palette is the light one.
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// Syntax colours stay more vivid than the surrounding chrome in light mode:
/// code is where the eye needs hue to distinguish meaning quickly.
pub fn code_keyword() -> Color32 { choose(lane(6), rgb(0xC2, 0x00, 0xC2)) }
pub fn code_declaration() -> Color32 { choose(lane(5), rgb(0xE5, 0x14, 0x14)) }
pub fn code_type() -> Color32 { choose(lane(4), rgb(0x7B, 0x3F, 0xF2)) }
pub fn code_call() -> Color32 { choose(lane(0), rgb(0x7B, 0x3F, 0xF2)) }
pub fn code_quoted() -> Color32 { choose(lane(7), rgb(0x00, 0x2B, 0x72)) }
pub fn code_number() -> Color32 { choose(lane(3), rgb(0x00, 0x8A, 0x4A)) }

pub fn canvas() -> Color32 { choose(rgb(0x0F, 0x14, 0x16), rgb(0xFF, 0xFF, 0xFF)) }
/// File and code surfaces stay white in Sol, separate from the surrounding
/// tool chrome, just as an editor does.
pub fn file_surface() -> Color32 { choose(panel(), rgb(0xFF, 0xFF, 0xFF)) }
pub fn panel() -> Color32 { choose(rgb(0x14, 0x1A, 0x1C), rgb(0xF5, 0xF5, 0xF5)) }
pub fn raised() -> Color32 { choose(rgb(0x1B, 0x23, 0x26), rgb(0xF5, 0xF5, 0xF5)) }
pub fn sunken() -> Color32 { choose(rgb(0x0A, 0x0E, 0x10), rgb(0xE7, 0xE8, 0xEC)) }
pub fn hover() -> Color32 { choose(rgb(0x1A, 0x22, 0x26), rgb(0xC9, 0xDE, 0xF5)) }
pub fn chosen() -> Color32 { choose(rgb(0x23, 0x2E, 0x33), rgb(0xE7, 0xE8, 0xEC)) }
pub fn hairline() -> Color32 { choose(rgb(0x22, 0x2C, 0x30), rgb(0xEE, 0xEE, 0xF2)) }
pub fn edge() -> Color32 { choose(rgb(0x30, 0x3C, 0x41), rgb(0xCC, 0xCE, 0xDB)) }
pub fn text() -> Color32 { choose(rgb(0xDC, 0xE4, 0xE6), rgb(0x1E, 0x1E, 0x1E)) }
pub fn muted() -> Color32 { choose(rgb(0x8D, 0x9D, 0xA2), rgb(0x6B, 0x6B, 0x6B)) }
pub fn faint() -> Color32 { choose(rgb(0x5F, 0x70, 0x76), rgb(0x6F, 0x6F, 0x6F)) }
pub fn head() -> Color32 { choose(rgb(0xF0, 0xB4, 0x29), rgb(0x9A, 0x61, 0x00)) }
pub fn added() -> Color32 { choose(rgb(0x72, 0xC9, 0x8A), rgb(0x28, 0x7A, 0x45)) }
pub fn removed() -> Color32 { choose(rgb(0xE7, 0x67, 0x5E), rgb(0xB5, 0x35, 0x2B)) }
pub fn changed() -> Color32 { choose(rgb(0xE8, 0xA3, 0x3D), rgb(0xA4, 0x63, 0x00)) }
pub fn added_bg() -> Color32 { choose(rgb(0x12, 0x24, 0x1A), rgb(0xE7, 0xF5, 0xEB)) }
pub fn removed_bg() -> Color32 { choose(rgb(0x2A, 0x16, 0x16), rgb(0xFC, 0xE8, 0xE7)) }
pub fn added_fg() -> Color32 { choose(rgb(0xB6, 0xE2, 0xC2), rgb(0x1E, 0x63, 0x37)) }
pub fn removed_fg() -> Color32 { choose(rgb(0xF2, 0xB4, 0xAF), rgb(0x9B, 0x2B, 0x23)) }

// ── Lanes ──────────────────────────────────────────────────────────────────
// Eight tones of even lightness, so that no lane shouts louder than another,
// and that tell apart both in a two-pixel line and in a five-pixel dot.

const LANES: [Color32; 8] = [
    rgb(0x5B, 0xA8, 0xF5), // azur
    rgb(0x72, 0xC9, 0x8A), // jade
    rgb(0xE8, 0xA3, 0x3D), // amber
    rgb(0xE2, 0x73, 0x9C), // rosa
    rgb(0x4F, 0xC7, 0xCB), // turquesa
    rgb(0xE7, 0x67, 0x5E), // coral
    rgb(0xA9, 0x8C, 0xF0), // violeta
    rgb(0xC9, 0xB2, 0x4E), // brass
];

pub fn lane(index: usize) -> Color32 {
    LANES[index % LANES.len()]
}

// ── The terminal ───────────────────────────────────────────────────────────
// The one place in the window that does not choose its own colours. A
// program says "red" by number and expects red, so there have to be sixteen
// answers, and eight of them have to be the eight a terminal has always
// had. They are the lanes: the same coral, jade, amber and azure the graph
// is drawn in, so that `git status` in the panel and the changes list
// beside it are the same red and the same green. The bright eight are the
// same eight a step lighter, which is all "bright" has meant since it
// stopped meaning "bold".
//
// On white the lanes are too pale to read as text, so the light table is
// the light theme's own darker set, and "white" there is a grey: a program
// that writes in white is asking to be read, not to be invisible.

/// What is behind the letters in the terminal: the deepest surface there
/// is, so that the panel reads as a place set into the window.
pub fn terminal_ground() -> Color32 {
    choose(sunken(), rgb(0xFF, 0xFF, 0xFF))
}

/// The sixteen colours a program in the terminal can ask for by name:
/// black, red, green, yellow, blue, magenta, cyan and white, and then the
/// same eight again, bright.
pub fn terminal() -> [Color32; 16] {
    if is_light() {
        return [
            rgb(0x1E, 0x1E, 0x1E),
            rgb(0xB5, 0x35, 0x2B),
            rgb(0x28, 0x7A, 0x45),
            rgb(0xA4, 0x63, 0x00),
            rgb(0x0B, 0x63, 0xC5),
            rgb(0x7B, 0x3F, 0xF2),
            rgb(0x00, 0x7C, 0x85),
            rgb(0x5A, 0x5F, 0x66),
            rgb(0x6F, 0x6F, 0x6F),
            rgb(0xD8, 0x45, 0x3A),
            rgb(0x2F, 0x96, 0x54),
            rgb(0xC2, 0x7A, 0x0A),
            rgb(0x2B, 0x7F, 0xE0),
            rgb(0x95, 0x62, 0xF7),
            rgb(0x00, 0x97, 0xA2),
            rgb(0x8A, 0x8F, 0x98),
        ];
    }
    let lighter = |color: Color32| {
        let lift = |channel: u8| channel + ((255 - channel) as f32 * 0.28) as u8;
        rgb(lift(color.r()), lift(color.g()), lift(color.b()))
    };
    let dull = [
        rgb(0x1B, 0x23, 0x26),
        LANES[5],
        LANES[1],
        LANES[2],
        LANES[0],
        LANES[6],
        LANES[4],
        rgb(0xC3, 0xCD, 0xD0),
    ];
    let mut all = [Color32::BLACK; 16];
    for (index, color) in dull.into_iter().enumerate() {
        all[index] = color;
        all[index + 8] = lighter(color);
    }
    // Bright black is the one that is not black a step lighter: it is what
    // programs write their asides in, and has to be readable on the ground.
    all[8] = rgb(0x5F, 0x70, 0x76);
    all[15] = rgb(0xF2, 0xF6, 0xF7);
    all
}

/// The same color let down to a background, for the branch chips.
pub fn tint(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

// ── Measures ───────────────────────────────────────────────────────────────

/// The height of a graph row and of the file lists.
pub const ROW_H: f32 = 28.0;
/// The corner radius. One only, and small: this is a tool.
pub const RADIUS: u8 = 4;

// ── Typography ─────────────────────────────────────────────────────────────
// Fira Sans for the prose and Fira Mono for the data. They are sisters, with
// the same skeleton, so the hashes and the dates sit beside the messages
// without the jump any other mono would give.
//
// The mono is not decoration: hashes are compared character by character and
// dates have to line up in a column.

/// Fira Sans Medium, for what has to hold its own without shouting.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

/// Fira Sans SemiBold, for the few headings there are.
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("strong".into()))
}

pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Installs the same Fira faces on every platform.
///
/// Looking in the Linux font folders made the design depend on the machine
/// that happened to run it: macOS and Windows silently used egui's fallback,
/// whose smaller metrics changed the bars and buttons. These are bundled
/// under the SIL Open Font License in `assets/fonts/LICENSE`, so a release
/// always lays out with the typeface it was designed around.
fn install_fonts(ctx: &egui::Context) {
    use std::sync::Arc;

    let mut fonts = egui::FontDefinitions::default();
    let mut add = |key: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(
            key.to_string(),
            Arc::new(egui::FontData::from_static(bytes)),
        );
    };

    add("fira-book", include_bytes!("../../../assets/fonts/FiraSans-Regular.ttf"));
    add("fira-medium", include_bytes!("../../../assets/fonts/FiraSans-Medium.ttf"));
    add(
        "fira-semibold",
        include_bytes!("../../../assets/fonts/FiraSans-SemiBold.ttf"),
    );
    add("fira-mono", include_bytes!("../../../assets/fonts/FiraMono-Regular.ttf"));

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "fira-book".to_string());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "fira-mono".to_string());

    // The weights go in families of their own because egui cannot synthesise
    // them, and a real SemiBold reads far better than the lighter grey with
    // which `RichText::strong` fakes bold.
    //
    // Each one starts from the fallback chain egui already brings. Built by
    // hand out of a couple of fonts, the symbols Fira does not cover -- HEAD's
    // dot, the reload arrow -- come out as empty boxes.
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    for (family, key) in [("medium", "fira-medium"), ("strong", "fira-semibold")] {
        let mut list = fallback.clone();
        list.insert(0, key.to_string());
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }

    ctx.set_fonts(fonts);
}

/// Installs the fonts and the initial appearance.
pub fn install(ctx: &egui::Context, appearance: Appearance) {
    install_fonts(ctx);
    sync(ctx, appearance);
}

/// Updates the palette when the selection, or the system preference behind
/// Auto, changes.
pub fn sync(ctx: &egui::Context, appearance: Appearance) {
    let light = match appearance {
        Appearance::Light => true,
        Appearance::Dark => false,
        Appearance::Auto => matches!(ctx.system_theme(), Some(egui::Theme::Light)),
    };
    if LIGHT.swap(light, Ordering::Relaxed) != light || !INSTALLED.swap(true, Ordering::Relaxed) {
        ctx.set_theme(if light { egui::ThemePreference::Light } else { egui::ThemePreference::Dark });
    }
    apply(ctx, light);
}

/// Installs the application palette over egui's matching base theme.
fn apply(ctx: &egui::Context, light: bool) {
    let mut style = (*ctx.style()).clone();

    // A short, deliberate scale. Thirteen to read, eleven for the data.
    use egui::TextStyle::*;
    style.text_styles = [
        (Heading, strong(15.0)),
        (Body, body(13.0)),
        (Button, medium(13.0)),
        (Small, body(11.0)),
        (Monospace, mono(12.0)),
    ]
    .into();

    let v = &mut style.visuals;
    v.dark_mode = !light;
    v.panel_fill = panel();
    v.window_fill = panel();
    v.extreme_bg_color = sunken();
    v.faint_bg_color = hover();
    v.code_bg_color = sunken();
    v.window_stroke = Stroke::new(1.0, hairline());
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.warn_fg_color = changed();
    v.error_fg_color = removed();
    v.hyperlink_color = lane(0);

    // Selection is the one quiet colour in the light theme: Visual Studio's
    // pale-blue mark lets the current view, row, or toggle stand out while
    // all controls at rest remain neutral.
    v.selection.bg_fill = choose(tint(lane(0), 56), rgb(0xC9, 0xDE, 0xF5));
    v.selection.stroke = Stroke::new(1.0, choose(lane(0), rgb(0x00, 0x7A, 0xCC)));

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = panel();
    w.noninteractive.weak_bg_fill = panel();
    w.noninteractive.bg_stroke = Stroke::new(1.0, hairline());
    w.noninteractive.fg_stroke = Stroke::new(1.0, muted());
    w.noninteractive.corner_radius = CornerRadius::same(RADIUS);

    w.inactive.bg_fill = raised();
    w.inactive.weak_bg_fill = raised();
    w.inactive.bg_stroke = Stroke::new(1.0, hairline());
    w.inactive.fg_stroke = Stroke::new(1.0, text());
    w.inactive.corner_radius = CornerRadius::same(RADIUS);
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = choose(rgb(0x24, 0x2F, 0x33), hover());
    w.hovered.weak_bg_fill = w.hovered.bg_fill;
    w.hovered.bg_stroke = Stroke::new(1.0, choose(edge(), rgb(0x00, 0x7A, 0xCC)));
    w.hovered.fg_stroke = Stroke::new(1.0, text());
    w.hovered.corner_radius = CornerRadius::same(RADIUS);
    w.hovered.expansion = 0.0;

    w.active.bg_fill = choose(rgb(0x2C, 0x39, 0x3E), rgb(0x00, 0x7A, 0xCC));
    w.active.weak_bg_fill = w.active.bg_fill;
    w.active.bg_stroke = Stroke::new(1.0, choose(lane(0), rgb(0x00, 0x7A, 0xCC)));
    w.active.fg_stroke = Stroke::new(1.0, choose(text(), rgb(0xFF, 0xFF, 0xFF)));
    w.active.corner_radius = CornerRadius::same(RADIUS);
    w.active.expansion = 0.0;

    w.open.bg_fill = choose(raised(), rgb(0xC9, 0xDE, 0xF5));
    w.open.weak_bg_fill = w.open.bg_fill;
    w.open.bg_stroke = Stroke::new(1.0, choose(edge(), rgb(0x00, 0x7A, 0xCC)));
    w.open.fg_stroke = Stroke::new(1.0, text());
    w.open.corner_radius = CornerRadius::same(RADIUS);

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(10.0, 5.0);
    s.indent = 14.0;
    s.interact_size.y = 24.0;
    s.scroll.bar_width = 9.0;
    // Overlay bars do not make the code jump sideways as they appear.
    s.scroll.floating = true;

    ctx.set_style(style);
}

/// A section label: lower case, not small caps, and with its count beside it.
/// The little drawings beside a heading and a row.
///
/// Painted rather than typed. An icon font would be the sixth dependency of a
/// project that has five on purpose, and the fonts actually on the machine --
/// Fira Sans and Fira Mono -- have no folder, no cloud and no branch fork in
/// them. These are four lines and a couple of dots each, which is all those
/// shapes ever were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// Branches that are here.
    Folder,
    /// Branches that are somewhere else.
    Cloud,
    /// A name pinned to one commit for good.
    Tag,
    /// Another folder with this same repository in it.
    Trees,
    /// One branch, on its own row.
    Branch,
    /// A group of branches sharing a prefix.
    Group,
}

/// Paints one, taking up a fixed square so the names beside them line up.
pub fn glyph(ui: &mut egui::Ui, which: Glyph, color: Color32) {
    const SIDE: f32 = 13.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SIDE, SIDE), egui::Sense::hover());
    glyph_at(ui, which, color, rect);
}

/// The same, somewhere you have already made room for.
pub fn glyph_at(ui: &egui::Ui, which: Glyph, color: Color32, rect: egui::Rect) {
    let painter = ui.painter();
    let thin = Stroke::new(1.2, color);

    // Everything is drawn inside a square a little smaller than the one
    // allocated, so two glyphs of different shapes still look the same size.
    let box_ = rect.shrink(1.5);
    let (left, right, top, bottom) = (box_.left(), box_.right(), box_.top(), box_.bottom());
    let middle = box_.center();

    match which {
        Glyph::Folder | Glyph::Group => {
            // A tab along the top third of the left half, then the body.
            let lip = top + box_.height() * 0.22;
            painter.line_segment([egui::pos2(left, lip), egui::pos2(left, bottom)], thin);
            painter.line_segment([egui::pos2(left, bottom), egui::pos2(right, bottom)], thin);
            painter.line_segment([egui::pos2(right, bottom), egui::pos2(right, lip)], thin);
            painter.line_segment([egui::pos2(right, lip), egui::pos2(middle.x, lip)], thin);
            painter.line_segment(
                [egui::pos2(middle.x, lip), egui::pos2(middle.x - 2.0, top)],
                thin,
            );
            painter.line_segment([egui::pos2(middle.x - 2.0, top), egui::pos2(left, top)], thin);
            painter.line_segment([egui::pos2(left, top), egui::pos2(left, lip)], thin);
        }
        Glyph::Cloud => {
            // Three overlapping circles with their bottoms cut off by a line.
            let y = middle.y + 1.5;
            painter.circle_stroke(egui::pos2(left + 3.2, y - 1.0), 3.2, thin);
            painter.circle_stroke(egui::pos2(middle.x + 1.0, y - 3.2), 3.8, thin);
            painter.circle_stroke(egui::pos2(right - 2.6, y - 1.2), 2.8, thin);
            // Painted over the circles in the panel's own colour, which is
            // what turns three circles into one outline. It has to sit just
            // below their centres, or there is no cloud left to see.
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left - 1.0, y),
                    egui::pos2(right + 1.0, bottom + 2.0),
                ),
                0,
                panel(),
            );
            painter.line_segment([egui::pos2(left + 0.6, y), egui::pos2(right - 0.6, y)], thin);
        }
        Glyph::Tag => {
            // A label with the hole the string goes through.
            painter.line_segment([egui::pos2(left, top), egui::pos2(middle.x, top)], thin);
            painter.line_segment([egui::pos2(middle.x, top), egui::pos2(right, middle.y)], thin);
            painter.line_segment([egui::pos2(right, middle.y), egui::pos2(middle.x, bottom)], thin);
            painter.line_segment([egui::pos2(middle.x, bottom), egui::pos2(left, bottom)], thin);
            painter.line_segment([egui::pos2(left, bottom), egui::pos2(left, top)], thin);
            painter.circle_filled(egui::pos2(left + 3.0, middle.y), 1.1, color);
        }
        Glyph::Trees => {
            // One square behind another: the same repository, twice over.
            let back = egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(right - 3.0, bottom - 3.0),
            );
            let front = egui::Rect::from_min_max(
                egui::pos2(left + 3.0, top + 3.0),
                egui::pos2(right, bottom),
            );
            painter.rect_stroke(back, 1, thin, egui::StrokeKind::Inside);
            painter.rect_filled(front, 1, panel());
            painter.rect_stroke(front, 1, thin, egui::StrokeKind::Inside);
        }
        Glyph::Branch => {
            // A line going on, and something leaving it: the whole of what a
            // branch is.
            let x = left + 2.5;
            painter.line_segment([egui::pos2(x, top + 1.0), egui::pos2(x, bottom - 1.0)], thin);
            painter.circle_filled(egui::pos2(x, top + 1.5), 1.7, color);
            painter.circle_filled(egui::pos2(x, bottom - 1.5), 1.7, color);
            painter.line_segment(
                [egui::pos2(x, middle.y), egui::pos2(right - 1.5, middle.y)],
                thin,
            );
            painter.circle_filled(egui::pos2(right - 1.5, middle.y), 1.7, color);
        }
    }
}

/// A folder of branches: everything before the last slash in their names.
///
/// Built the same way as a heading rather than with a plain collapsing
/// header, because a header's label is one string and this row is a triangle,
/// a folder and a name.
pub fn group(ui: &mut egui::Ui, name: &str, forced: bool, inside: impl FnOnce(&mut egui::Ui)) {
    let id = ui.make_persistent_id(("group", name));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
    if forced {
        state.set_open(true);
    }

    state
        .show_header(ui, |ui| {
            glyph(ui, Glyph::Group, muted());
            ui.add_space(2.0);
            ui.label(egui::RichText::new(name).font(body(12.0)).color(muted()));
        })
        .body(inside);
}

/// A heading with how many are under it, and what is under it, foldable.
///
/// `forced` opens it whatever it was last left at. A filter needs that: a
/// match sitting inside a folded section is, as far as anybody looking can
/// tell, a search that found nothing.
///
/// What is under it is indented, and the folders inside it again. The step is
/// what says which heading a name belongs to: without it a long list of
/// branches is a long list of branches, and whether you are looking at the
/// ones here or the ones on a remote is something you have to remember.
pub fn section(
    ui: &mut egui::Ui,
    title: &str,
    mark: Glyph,
    count: usize,
    forced: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    let id = ui.make_persistent_id(("section", title));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
    if forced {
        state.set_open(true);
    }

    rule(ui);
    state
        .show_header(ui, |ui| {
            glyph(ui, mark, muted());
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(title)
                    .font(medium(11.5))
                    .color(muted()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(count.to_string())
                        .font(mono(10.5))
                        .color(faint()),
                );
            });
        })
        .body(body);

    ui.add_space(8.0);
}

/// A quiet dividing line, with air on both sides.
pub fn rule(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.center().y,
        Stroke::new(1.0, hairline()),
    );
    ui.add_space(6.0);
}

/// The same hair, but you can take hold of it and move it.
///
/// Gives back how far it was dragged this frame, in pixels, downwards
/// positive. Whoever called it decides what that means; this only reports.
///
/// A line one pixel tall is not something anybody can hit with a mouse, so
/// the whole band a rule sits in -- the line and the breathing space above
/// and below it -- is what takes the drag. It is drawn exactly like a rule
/// until the pointer is on it, and thickens then: a panel full of handles
/// announcing themselves is a panel you cannot read.
///
/// `name` has to be the same every frame and different from the other grips
/// in the window. It cannot be left to egui, which names a widget by where
/// it is -- and where this one is, is the thing being dragged.
pub fn grip(ui: &mut egui::Ui, name: &'static str) -> f32 {
    let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), 13.0));
    let response = ui.interact(rect, egui::Id::new(("grip", name)), egui::Sense::drag());

    let live = response.hovered() || response.dragged();
    if live {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.center().y,
        match live {
            true => Stroke::new(2.0, chosen()),
            false => Stroke::new(1.0, hairline()),
        },
    );

    response.drag_delta().y
}

/// The same line, standing, to part groups inside a bar.
///
/// A shade lighter than the horizontal one: against the bar's grey, the
/// panels' grey cannot be made out.
pub fn vrule(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 19.0), egui::Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.top()..=rect.bottom(), Stroke::new(1.0, edge()));
    ui.add_space(6.0);
}

/// The caret of a button that opens a popup.
///
/// It is painted rather than written. The Unicode triangle is not in every
/// font, and where it is missing out comes the empty box of the characters
/// nobody knows how to draw, which is worse than nothing at all.
pub fn caret(ui: &egui::Ui, response: &egui::Response) {
    let x = response.rect.right() - 11.0;
    let y = response.rect.center().y + 1.0;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(x - 3.6, y - 2.0),
            egui::pos2(x + 3.6, y - 2.0),
            egui::pos2(x, y + 2.4),
        ],
        muted(),
        Stroke::NONE,
    ));
}

/// A centred message for when a panel has nothing to show. It says what to
/// do, and does not apologise.
pub fn empty(ui: &mut egui::Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(56.0);
        ui.label(egui::RichText::new(text).font(body(12.5)).color(faint()));
    });
}

/// A path with the folder muted and the file's name at full contrast, which
/// is what you are after when running down a list of changes.
///
/// Over the fill of the chosen row the folder gets its contrast back: that
/// muted grey was meant for the panel's background, and over the blue of the
/// selection it cannot be read.
pub fn path_text(path: &str, size: f32, selected: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let (folder, name) = match path.rsplit_once('/') {
        Some((folder, name)) => (format!("{folder}/"), name),
        None => (String::new(), path),
    };

    let dim = if selected { muted() } else { faint() };
    if !folder.is_empty() {
        job.append(&folder, 0.0, egui::TextFormat::simple(body(size), dim));
    }
    job.append(name, 0.0, egui::TextFormat::simple(body(size), text()));
    job
}

/// `1 file`, `3 files`.
pub fn files_label(count: usize) -> String {
    count_label(count, "file")
}

/// `1 pull request`, `4 pull requests`, `no pull requests`.
///
/// Zero gets a word rather than a digit. "0 pull requests" reads like a
/// measurement that came out empty; "no pull requests" reads like an answer.
pub fn count_label(count: usize, thing: &str) -> String {
    match count {
        0 => format!("no {thing}s"),
        1 => format!("1 {thing}"),
        n => format!("{n} {thing}s"),
    }
}

/// "2 staged, 3 unstaged", saying nothing of the side that is empty.
pub fn pending_label(staged: usize, unstaged: usize) -> String {
    let mut parts = Vec::new();
    if staged > 0 {
        parts.push(format!("{staged} staged"));
    }
    if unstaged > 0 {
        parts.push(format!("{unstaged} unstaged"));
    }
    parts.join(", ")
}
