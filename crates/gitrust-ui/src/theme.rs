//! El sistema visual: color, tipografía y espaciado.
//!
//! Una sola familia saturada manda en la pantalla, la de los carriles del
//! grafo, porque la estructura de ramas es lo que se viene a leer. Todo lo
//! demás es gris con un matiz frío constante, y solo HEAD se permite un
//! ámbar. Si algo nuevo necesita color, sale de aquí o no sale.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// ── Superficies ────────────────────────────────────────────────────────────
// Pizarra azul verdosa, no negro. El lienzo del grafo es el más hondo para
// que los carriles destaquen sobre él.

/// El fondo del grafo.
pub const CANVAS: Color32 = rgb(0x0F, 0x14, 0x16);
/// Barras laterales.
pub const PANEL: Color32 = rgb(0x14, 0x1A, 0x1C);
/// Barra superior y cabeceras.
pub const RAISED: Color32 = rgb(0x1B, 0x23, 0x26);
/// Cajas de texto, que se leen hundidas.
pub const SUNKEN: Color32 = rgb(0x0A, 0x0E, 0x10);
/// Fondo de la fila bajo el cursor.
pub const HOVER: Color32 = rgb(0x1A, 0x22, 0x26);
/// Fondo de la fila elegida.
pub const CHOSEN: Color32 = rgb(0x23, 0x2E, 0x33);

/// Líneas de separación: solo donde hacen falta de verdad.
pub const HAIRLINE: Color32 = rgb(0x22, 0x2C, 0x30);
/// Borde de lo que se puede pulsar.
pub const EDGE: Color32 = rgb(0x30, 0x3C, 0x41);

// ── Texto ──────────────────────────────────────────────────────────────────
// Tres niveles. Lo secundario se aparta por contraste, no por tamaño.

pub const TEXT: Color32 = rgb(0xDC, 0xE4, 0xE6);
pub const MUTED: Color32 = rgb(0x8D, 0x9D, 0xA2);
pub const FAINT: Color32 = rgb(0x5F, 0x70, 0x76);

/// El ámbar de HEAD. Es la única marca de este color en toda la ventana.
pub const HEAD: Color32 = rgb(0xF0, 0xB4, 0x29);

// ── Diff ───────────────────────────────────────────────────────────────────

pub const ADDED: Color32 = rgb(0x72, 0xC9, 0x8A);
pub const REMOVED: Color32 = rgb(0xE7, 0x67, 0x5E);
pub const CHANGED: Color32 = rgb(0xE8, 0xA3, 0x3D);
pub const ADDED_BG: Color32 = rgb(0x12, 0x24, 0x1A);
pub const REMOVED_BG: Color32 = rgb(0x2A, 0x16, 0x16);
pub const ADDED_FG: Color32 = rgb(0xB6, 0xE2, 0xC2);
pub const REMOVED_FG: Color32 = rgb(0xF2, 0xB4, 0xAF);

// ── Carriles ───────────────────────────────────────────────────────────────
// Ocho tonos de luminancia pareja, para que ningún carril grite más que otro,
// y separables tanto en una línea de dos píxeles como en un punto de cinco.

const LANES: [Color32; 8] = [
    rgb(0x5B, 0xA8, 0xF5), // azur
    rgb(0x72, 0xC9, 0x8A), // jade
    rgb(0xE8, 0xA3, 0x3D), // ámbar
    rgb(0xE2, 0x73, 0x9C), // rosa
    rgb(0x4F, 0xC7, 0xCB), // turquesa
    rgb(0xE7, 0x67, 0x5E), // coral
    rgb(0xA9, 0x8C, 0xF0), // violeta
    rgb(0xC9, 0xB2, 0x4E), // latón
];

pub fn lane(index: usize) -> Color32 {
    LANES[index % LANES.len()]
}

/// El mismo color rebajado a un fondo, para las pastillas de rama.
pub fn tint(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

// ── Medidas ────────────────────────────────────────────────────────────────

/// Alto de una fila del grafo y de las listas de ficheros.
pub const ROW_H: f32 = 28.0;
/// Radio de esquina. Uno solo, y pequeño: esto es una herramienta.
pub const RADIUS: u8 = 4;

// ── Tipografía ─────────────────────────────────────────────────────────────
// Fira Sans para la prosa y Fira Mono para los datos. Son hermanas, con el
// mismo esqueleto, así que los hashes y las fechas se sientan junto a los
// mensajes sin dar el salto que daría una mono cualquiera.
//
// La mono no es decoración: los hashes se comparan carácter a carácter y las
// fechas tienen que formar columna.

/// Fira Sans Medium, para lo que debe sostenerse sin gritar.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

/// Fira Sans SemiBold, para los pocos títulos que hay.
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("strong".into()))
}

pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Rutas donde suele vivir Fira en las distribuciones de escritorio.
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

/// Instala Fira si está en el sistema. Si no, egui se queda con las suyas y
/// la aplicación sigue funcionando: la tipografía es una mejora, no un
/// requisito.
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

    // Los pesos van en familias propias porque egui no sabe sintetizarlos, y
    // un SemiBold de verdad se lee mucho mejor que el gris más claro con el
    // que `RichText::strong` finge negrita.
    //
    // Cada una parte de la cadena de respaldo que ya trae egui. Si se monta a
    // mano con un par de fuentes, los símbolos que Fira no cubre —el punto de
    // HEAD, la flecha de recargar— salen como cuadros vacíos.
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

/// Deja el contexto con el tema completo: fuentes, colores y espaciado.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut style = (*ctx.style()).clone();

    // Una escala corta y deliberada. Trece para leer, once para los datos.
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

    // La selección toma el azur del primer carril: el mismo azul que ya está
    // en pantalla, en vez de uno nuevo traído de fuera.
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

/// Un rótulo de sección: minúsculas, no versales, y con su cuenta al lado.
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

/// Una línea de separación discreta, con aire a los dos lados.
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

/// La misma línea, de pie, para separar grupos dentro de una barra.
///
/// Un punto más clara que la horizontal: sobre el gris de la barra, el de los
/// paneles no se llega a ver.
pub fn vrule(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 19.0), egui::Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.top()..=rect.bottom(), Stroke::new(1.0, EDGE));
    ui.add_space(6.0);
}

/// La punta de un botón que abre un desplegable.
///
/// Se pinta en vez de escribirse. El triángulo de Unicode no está en todas
/// las fuentes, y donde falta sale el cuadro vacío de los caracteres que
/// nadie sabe dibujar, que es peor que no poner nada.
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

/// Un mensaje centrado para cuando un panel no tiene nada que enseñar. Dice
/// qué hacer, no se disculpa.
pub fn empty(ui: &mut egui::Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(56.0);
        ui.label(egui::RichText::new(text).font(body(12.5)).color(FAINT));
    });
}

/// Una ruta con la carpeta apagada y el nombre del fichero al contraste
/// pleno, que es lo que se busca al recorrer una lista de cambios.
///
/// Sobre el relleno de la fila elegida la carpeta recupera el contraste: ese
/// gris apagado se pensó para el fondo del panel, y sobre el azul de la
/// selección no se lee.
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

/// `1 fichero`, `3 ficheros`.
pub fn files_label(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
}

/// «2 staged, 3 unstaged», callando el lado que esté vacío.
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
