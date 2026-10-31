//! The visual system: color, typography and spacing.
//!
//! One saturated family rules the screen, the one of the graph's lanes,
//! because the shape of the branches is what people come to read. Everything
//! else is grey with a steady cold cast, and only HEAD is allowed an amber.
//! If something new needs color, it comes from here or it does not come.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// ── Surfaces ───────────────────────────────────────────────────────────────
// A blue-green slate, not black. The graph's canvas is the deepest, so that
// the lanes stand out against it.

/// The graph's background.
pub const CANVAS: Color32 = rgb(0x0F, 0x14, 0x16);
/// Side bars.
pub const PANEL: Color32 = rgb(0x14, 0x1A, 0x1C);
/// The top bar and headers.
pub const RAISED: Color32 = rgb(0x1B, 0x23, 0x26);
/// Text boxes, which read as sunken.
pub const SUNKEN: Color32 = rgb(0x0A, 0x0E, 0x10);
/// The background of the row under the cursor.
pub const HOVER: Color32 = rgb(0x1A, 0x22, 0x26);
/// The background of the chosen row.
pub const CHOSEN: Color32 = rgb(0x23, 0x2E, 0x33);

/// Dividing lines: only where they are really needed.
pub const HAIRLINE: Color32 = rgb(0x22, 0x2C, 0x30);
/// The border of what can be pressed.
pub const EDGE: Color32 = rgb(0x30, 0x3C, 0x41);

// ── Text ───────────────────────────────────────────────────────────────────
// Three levels. What is secondary steps back by contrast, not by size.

pub const TEXT: Color32 = rgb(0xDC, 0xE4, 0xE6);
pub const MUTED: Color32 = rgb(0x8D, 0x9D, 0xA2);
pub const FAINT: Color32 = rgb(0x5F, 0x70, 0x76);

/// HEAD's amber. It is the only mark of this color in the whole window.
pub const HEAD: Color32 = rgb(0xF0, 0xB4, 0x29);

// ── Diff ───────────────────────────────────────────────────────────────────

pub const ADDED: Color32 = rgb(0x72, 0xC9, 0x8A);
pub const REMOVED: Color32 = rgb(0xE7, 0x67, 0x5E);
pub const CHANGED: Color32 = rgb(0xE8, 0xA3, 0x3D);
pub const ADDED_BG: Color32 = rgb(0x12, 0x24, 0x1A);
pub const REMOVED_BG: Color32 = rgb(0x2A, 0x16, 0x16);
pub const ADDED_FG: Color32 = rgb(0xB6, 0xE2, 0xC2);
pub const REMOVED_FG: Color32 = rgb(0xF2, 0xB4, 0xAF);

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

/// The paths where Fira usually lives on desktop distributions.
fn find(names: &[&str]) -> Option<Vec<u8>> {
    const DIRS: [&str; 4] = [
        "/usr/share/fonts/opentype/fira",
        "/usr/share/fonts/truetype/fira",
        "/usr/share/fonts/truetype/firasans",
        "/usr/share/fonts/TTF",
    ];

    for dir in DIRS {
        for name in names {
            for ext in ["otf", "ttf"] {
                let path = format!("{dir}/{name}.{ext}");
                if let Ok(bytes) = std::fs::read(&path) {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

/// Installs Fira if it is on the system. If not, egui keeps its own and the
/// app carries on working: the typeface is an improvement, not a
/// requirement.
fn install_fonts(ctx: &egui::Context) {
    use std::sync::Arc;

    let mut fonts = egui::FontDefinitions::default();
    let mut add = |key: &str, names: &[&str]| -> bool {
        match find(names) {
            Some(bytes) => {
                fonts.font_data.insert(
                    key.to_string(),
                    Arc::new(egui::FontData::from_owned(bytes)),
                );
                true
            }
            None => false,
        }
    };

    let _book = add("fira-book", &["FiraSans-Book", "FiraSans-Regular"]);
    let medium = add("fira-medium", &["FiraSans-Medium"]);
    let semibold = add("fira-semibold", &["FiraSans-SemiBold"]);
    let code = add("fira-mono", &["FiraMono-Regular", "FiraMono"]);

    if _book {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "fira-book".to_string());
    }
    if code {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "fira-mono".to_string());
    }

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

    for (family, key, present) in [
        ("medium", "fira-medium", medium),
        ("strong", "fira-semibold", semibold),
    ] {
        let mut list = fallback.clone();
        if present {
            list.insert(0, key.to_string());
        }
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }

    ctx.set_fonts(fonts);
}

/// Leaves the context with the whole theme: fonts, colors and spacing.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

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
    v.dark_mode = true;
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = SUNKEN;
    v.faint_bg_color = HOVER;
    v.code_bg_color = SUNKEN;
    v.window_stroke = Stroke::new(1.0, HAIRLINE);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.warn_fg_color = CHANGED;
    v.error_fg_color = REMOVED;
    v.hyperlink_color = lane(0);

    // The selection takes the first lane's azure: the same blue already on
    // screen, instead of a new one brought in from outside.
    v.selection.bg_fill = tint(lane(0), 56);
    v.selection.stroke = Stroke::new(1.0, lane(0));

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    w.noninteractive.corner_radius = CornerRadius::same(RADIUS);

    w.inactive.bg_fill = RAISED;
    w.inactive.weak_bg_fill = RAISED;
    w.inactive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = CornerRadius::same(RADIUS);
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = rgb(0x24, 0x2F, 0x33);
    w.hovered.weak_bg_fill = rgb(0x24, 0x2F, 0x33);
    w.hovered.bg_stroke = Stroke::new(1.0, EDGE);
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.corner_radius = CornerRadius::same(RADIUS);
    w.hovered.expansion = 0.0;

    w.active.bg_fill = rgb(0x2C, 0x39, 0x3E);
    w.active.weak_bg_fill = rgb(0x2C, 0x39, 0x3E);
    w.active.bg_stroke = Stroke::new(1.0, lane(0));
    w.active.fg_stroke = Stroke::new(1.0, TEXT);
    w.active.corner_radius = CornerRadius::same(RADIUS);
    w.active.expansion = 0.0;

    w.open.bg_fill = RAISED;
    w.open.weak_bg_fill = RAISED;
    w.open.bg_stroke = Stroke::new(1.0, EDGE);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);
    w.open.corner_radius = CornerRadius::same(RADIUS);

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(10.0, 5.0);
    s.indent = 14.0;
    s.interact_size.y = 24.0;
    s.scroll.bar_width = 9.0;
    s.scroll.floating = false;

    ctx.set_style(style);
}

/// A section label: lower case, not small caps, and with its count beside it.
pub fn section(ui: &mut egui::Ui, title: &str, count: usize) {
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(title)
                .font(medium(11.5))
                .color(MUTED),
        );
        ui.label(
            egui::RichText::new(count.to_string())
                .font(mono(10.5))
                .color(FAINT),
        );
    });
}

/// A quiet dividing line, with air on both sides.
pub fn rule(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.center().y,
        Stroke::new(1.0, HAIRLINE),
    );
    ui.add_space(6.0);
}

/// The same line, standing, to part groups inside a bar.
///
/// A shade lighter than the horizontal one: against the bar's grey, the
/// panels' grey cannot be made out.
pub fn vrule(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 19.0), egui::Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.top()..=rect.bottom(), Stroke::new(1.0, EDGE));
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
        MUTED,
        Stroke::NONE,
    ));
}

/// A centred message for when a panel has nothing to show. It says what to
/// do, and does not apologise.
pub fn empty(ui: &mut egui::Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(56.0);
        ui.label(egui::RichText::new(text).font(body(12.5)).color(FAINT));
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

    let dim = if selected { MUTED } else { FAINT };
    if !folder.is_empty() {
        job.append(&folder, 0.0, egui::TextFormat::simple(body(size), dim));
    }
    job.append(name, 0.0, egui::TextFormat::simple(body(size), TEXT));
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
