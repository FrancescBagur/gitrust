//! Pintado del grafo de commits.
//!
//! Dibuja solo las filas visibles. Con un repositorio de decenas de miles de
//! commits el coste por fotograma depende del alto de la ventana, no del
//! tamaño de la historia.

use chrono::{Local, TimeZone};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Stroke};
use gitrust_core::{Graph, Oid, RefKind, Row};

const ROW_H: f32 = 26.0;
const LANE_W: f32 = 16.0;
const DOT_R: f32 = 4.5;
const LINE_W: f32 = 1.8;
const GUTTER: f32 = 10.0;

const SELECTED_BG: Color32 = Color32::from_rgb(0x27, 0x3A, 0x55);

/// Colores de carril. Se recorren en ciclo según nacen las ramas.
const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x55, 0xA2, 0xF7),
    Color32::from_rgb(0x6D, 0xC9, 0x51),
    Color32::from_rgb(0xF2, 0x9D, 0x38),
    Color32::from_rgb(0xD9, 0x63, 0xD9),
    Color32::from_rgb(0x40, 0xC8, 0xC0),
    Color32::from_rgb(0xE5, 0x5F, 0x5F),
    Color32::from_rgb(0xB0, 0x8E, 0xF5),
    Color32::from_rgb(0xD6, 0xC4, 0x45),
];

fn lane_color(index: usize) -> Color32 {
    PALETTE[index % PALETTE.len()]
}

/// Devuelve el commit pulsado, si lo hay.
pub fn show(ui: &mut egui::Ui, graph: &Graph, selected: Option<Oid>) -> Option<Oid> {
    if graph.rows.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(egui::RichText::new("Sin commits que mostrar").weak());
        });
        return None;
    }

    let lanes = graph.lane_count.max(1);
    let graph_w = GUTTER * 2.0 + lanes as f32 * LANE_W;
    let mut clicked = None;

    egui::ScrollArea::vertical()
        .id_salt("grafo")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let total_h = graph.rows.len() as f32 * ROW_H;
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), total_h),
                egui::Sense::click(),
            );
            let painter = ui.painter_at(rect);

            // Solo las filas que caen dentro del recorte visible.
            let clip = ui.clip_rect();
            let first = (((clip.top() - rect.top()) / ROW_H).floor() as isize).max(0) as usize;
            let last = ((((clip.bottom() - rect.top()) / ROW_H).ceil() as usize) + 1)
                .min(graph.rows.len());

            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let index = ((pos.y - rect.top()) / ROW_H).floor();
                    if index >= 0.0 && (index as usize) < graph.rows.len() {
                        clicked = Some(graph.rows[index as usize].commit.id);
                    }
                }
            }

            let lane_x = |lane: usize| rect.left() + GUTTER + lane as f32 * LANE_W + LANE_W / 2.0;
            let row_y = |row: usize| rect.top() + row as f32 * ROW_H + ROW_H / 2.0;

            for index in first..last {
                let row = &graph.rows[index];

                if selected == Some(row.commit.id) {
                    let band = egui::Rect::from_min_size(
                        egui::pos2(rect.left(), rect.top() + index as f32 * ROW_H),
                        egui::vec2(rect.width(), ROW_H),
                    );
                    painter.rect_filled(band, 0.0, SELECTED_BG);
                }

                // Las líneas de esta fila bajan hasta la siguiente.
                for link in &row.links {
                    let stroke = Stroke::new(LINE_W, lane_color(link.color));
                    let top = Pos2::new(lane_x(link.from), row_y(index));
                    let bottom = Pos2::new(lane_x(link.to), row_y(index + 1));

                    if link.from == link.to {
                        painter.line_segment([top, bottom], stroke);
                    } else {
                        // Curva en S para que el salto de carril se lea bien.
                        let c1 = Pos2::new(top.x, top.y + ROW_H * 0.5);
                        let c2 = Pos2::new(bottom.x, bottom.y - ROW_H * 0.5);
                        painter.add(egui::Shape::CubicBezier(
                            egui::epaint::CubicBezierShape::from_points_stroke(
                                [top, c1, c2, bottom],
                                false,
                                Color32::TRANSPARENT,
                                stroke,
                            ),
                        ));
                    }
                }

                draw_commit(&painter, row, lane_x(row.lane), row_y(index));
                draw_text(&painter, ui, row, rect.left() + graph_w, row_y(index), rect.right());
            }
        });

    clicked
}

fn draw_commit(painter: &egui::Painter, row: &Row, x: f32, y: f32) {
    let color = lane_color(row.color);
    let center = Pos2::new(x, y);

    // Un merge se marca con un punto hueco; un commit normal, macizo.
    if row.commit.parents.len() > 1 {
        painter.circle_filled(center, DOT_R, Color32::from_gray(24));
        painter.circle_stroke(center, DOT_R, Stroke::new(2.0, color));
    } else {
        painter.circle_filled(center, DOT_R, color);
    }
}

fn draw_text(
    painter: &egui::Painter,
    ui: &egui::Ui,
    row: &Row,
    left: f32,
    y: f32,
    right: f32,
) {
    let mut x = left + 6.0;
    let label_font = FontId::monospace(11.0);

    for label in &row.commit.refs {
        let (color, text) = match label.kind {
            RefKind::Head => (Color32::from_rgb(0xFF, 0xD1, 0x4A), format!("⌂ {}", label.name)),
            RefKind::LocalBranch => (Color32::from_rgb(0x7A, 0xC6, 0xFF), label.name.clone()),
            RefKind::RemoteBranch => (Color32::from_rgb(0x9A, 0xA6, 0xB2), label.name.clone()),
            RefKind::Tag => (Color32::from_rgb(0xC7, 0xA6, 0xF5), format!("⚑ {}", label.name)),
        };
        let bounds = painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            text,
            label_font.clone(),
            color,
        );
        x = bounds.right() + 10.0;
    }

    // Reservamos el ancho de las columnas de la derecha.
    let meta_w = 300.0;
    let summary_right = (right - meta_w).max(x + 40.0);

    let summary = elide(ui, &row.commit.summary, summary_right - x, 13.0);
    painter.text(
        Pos2::new(x, y),
        Align2::LEFT_CENTER,
        summary,
        FontId::proportional(13.0),
        Color32::from_gray(222),
    );

    let when = Local
        .timestamp_opt(row.commit.time, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();

    painter.text(
        Pos2::new(right - 8.0, y),
        Align2::RIGHT_CENTER,
        &row.commit.short_id,
        FontId::monospace(11.0),
        Color32::from_gray(130),
    );
    painter.text(
        Pos2::new(right - 78.0, y),
        Align2::RIGHT_CENTER,
        when,
        FontId::monospace(11.0),
        Color32::from_gray(130),
    );
    painter.text(
        Pos2::new(right - 200.0, y),
        Align2::RIGHT_CENTER,
        elide(ui, &row.commit.author, 110.0, 11.0),
        FontId::proportional(11.0),
        Color32::from_gray(160),
    );
}

/// Recorta el texto con puntos suspensivos si no cabe en `max_w`.
fn elide(ui: &egui::Ui, text: &str, max_w: f32, size: f32) -> String {
    let font = FontId::proportional(size);
    let width = |s: &str| {
        ui.fonts(|f| {
            s.chars()
                .map(|c| f.glyph_width(&font, c))
                .sum::<f32>()
        })
    };

    if width(text) <= max_w {
        return text.to_string();
    }

    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        if width(&out) > max_w - 12.0 {
            out.pop();
            break;
        }
    }
    out.push('…');
    out
}
