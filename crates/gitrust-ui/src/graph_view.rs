//! Pintado del grafo de commits.
//!
//! Dibuja solo las filas visibles. Con un repositorio de decenas de miles de
//! commits el coste por fotograma depende del alto de la ventana, no del
//! tamaño de la historia.
//!
//! La fila es la unidad de lectura: el mensaje a la izquierda con todo el
//! contraste, y autor, fecha y hash alineados a la derecha en columnas de
//! ancho fijo. Son esas columnas las que dan la estructura, así que no hace
//! falta ni una sola línea vertical. Cuando la ventana se estrecha las
//! columnas se retiran por orden de importancia en vez de chocar entre sí.

use chrono::{Local, TimeZone};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Shape, Stroke};
use gitrust_core::{Graph, Oid, RefKind, Row, WorkingState};

use crate::theme;

const LANE_W: f32 = 17.0;
const DOT_R: f32 = 4.5;
const LINE_W: f32 = 2.0;
const GUTTER: f32 = 12.0;

/// Columnas de la derecha, de fuera hacia dentro.
const PAD_R: f32 = 14.0;
const HASH_W: f32 = 58.0;
const DATE_W: f32 = 100.0;
const AUTHOR_W: f32 = 116.0;
const COL_GAP: f32 = 16.0;
/// Por debajo de esto el mensaje queda tan recortado que la columna estorba
/// más de lo que informa, y se retira.
const SUMMARY_MIN: f32 = 200.0;
/// Carriles que se reservan como mucho antes de la columna del mensaje.
///
/// Un repositorio puede tener veinte ramas vivas a la vez en algún punto de su
/// historia, pero reservar sitio para las veinte deja una franja vacía en las
/// miles de filas donde solo hay dos. Ocho cubre lo que se ve en la práctica;
/// lo que pase de ahí se recorta contra la columna del mensaje.
const MAX_LANES: usize = 8;
/// Trazo de la línea que baja de los cambios sin confirmar: raya y hueco.
const DASH: (f32, f32) = (3.0, 3.5);

/// Devuelve el commit pulsado, si lo hay.
///
/// El estado del árbol de trabajo hace falta para rotular la fila de cambios
/// sin confirmar, cuando el grafo la trae.
pub fn show(
    ui: &mut egui::Ui,
    graph: &Graph,
    selected: Option<Oid>,
    work: &WorkingState,
) -> Option<Oid> {
    if graph.rows.is_empty() {
        theme::empty(ui, "Abre un repositorio para ver su historia");
        return None;
    }

    // Hasta dónde baja la línea de los cambios sin confirmar: de su fila a la
    // de HEAD, que es su padre. Lo que pase por ese carril antes de llegar
    // sigue siendo esa misma línea, y se dibuja a trazos.
    let pending = graph.rows.first().filter(|row| row.commit.is_working()).map(|row| {
        let head = row.commit.parents[0];
        let until = graph.rows.iter().position(|r| r.commit.id == head).unwrap_or(0);
        (row.lane, until)
    });
    let pending_label = theme::pending_label(work.staged.len(), work.unstaged.len());

    // Con una historia muy ramificada los carriles se comerían la columna
    // del mensaje, así que a partir de cierto punto se dejan solapar.
    let lanes = graph.lane_count.clamp(1, MAX_LANES);
    let graph_w = GUTTER * 2.0 + lanes as f32 * LANE_W;
    let mut clicked = None;

    egui::ScrollArea::vertical()
        .id_salt("grafo")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let row_h = theme::ROW_H;
            let total_h = graph.rows.len() as f32 * row_h;
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), total_h),
                egui::Sense::click(),
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme::CANVAS);

            // Los carriles se pintan contra su propia franja: así una historia
            // más ancha que el tope se recorta en seco en vez de cruzar por
            // debajo de los mensajes.
            let lanes_painter = painter.with_clip_rect(Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + graph_w, rect.max.y),
            ));

            // Solo las filas que caen dentro del recorte visible.
            let clip = ui.clip_rect();
            let first = (((clip.top() - rect.top()) / row_h).floor() as isize).max(0) as usize;
            let last = ((((clip.bottom() - rect.top()) / row_h).ceil() as usize) + 1)
                .min(graph.rows.len());

            let row_at = |y: f32| {
                let index = ((y - rect.top()) / row_h).floor();
                (index >= 0.0 && (index as usize) < graph.rows.len()).then(|| index as usize)
            };

            let hovered = response.hover_pos().and_then(|pos| row_at(pos.y));
            if response.clicked() {
                if let Some(index) = response.interact_pointer_pos().and_then(|p| row_at(p.y)) {
                    clicked = Some(graph.rows[index].commit.id);
                }
            }

            let lane_x = |lane: usize| rect.left() + GUTTER + lane as f32 * LANE_W + LANE_W / 2.0;
            let row_y = |row: usize| rect.top() + row as f32 * row_h + row_h / 2.0;

            let columns = Columns::for_width(rect.left() + graph_w, rect.right());

            let band_of = |index: usize| {
                Rect::from_min_size(
                    egui::pos2(rect.left(), rect.top() + index as f32 * row_h),
                    egui::vec2(rect.width(), row_h),
                )
            };

            // Los fondos van todos primero. Si cada fila pintara el suyo justo
            // antes de sus líneas, la banda de la fila elegida taparía la mitad
            // de la línea que baja hasta ella desde la de arriba.
            for index in first..last {
                let row = &graph.rows[index];

                // La fila elegida se marca con su propio color de carril en el
                // canto izquierdo, no con un azul traído de fuera: así se ve de
                // un vistazo a qué rama pertenece lo que se está mirando.
                if selected == Some(row.commit.id) {
                    let band = band_of(index);
                    let edge = if row.commit.is_working() {
                        theme::MUTED
                    } else {
                        theme::lane(row.color)
                    };
                    painter.rect_filled(band, 0.0, theme::CHOSEN);
                    painter.rect_filled(
                        Rect::from_min_size(band.min, egui::vec2(2.0, band.height())),
                        0.0,
                        edge,
                    );
                } else if hovered == Some(index) {
                    painter.rect_filled(band_of(index), 0.0, theme::HOVER);
                }
            }

            for index in first..last {
                let row = &graph.rows[index];
                let band = band_of(index);

                // Las líneas de esta fila bajan hasta la siguiente.
                for link in &row.links {
                    // La que une los cambios con HEAD va a trazos y sin color:
                    // todavía no es historia, y no debe leerse como una rama.
                    // Solo la que baja recta por ese carril: una rama que se
                    // fusione en HEAD también acaba ahí, y esa sí es historia.
                    let dashed = pending.is_some_and(|(lane, until)| {
                        index < until && link.from == lane && link.to == lane
                    });
                    let stroke = if dashed {
                        Stroke::new(LINE_W, theme::MUTED)
                    } else {
                        Stroke::new(LINE_W, theme::lane(link.color))
                    };
                    let top = Pos2::new(lane_x(link.from), row_y(index));
                    let bottom = Pos2::new(lane_x(link.to), row_y(index + 1));

                    if dashed {
                        lanes_painter
                            .extend(Shape::dashed_line(&[top, bottom], stroke, DASH.0, DASH.1));
                    } else if link.from == link.to {
                        lanes_painter.line_segment([top, bottom], stroke);
                    } else {
                        // Curva en S para que el salto de carril se lea bien.
                        let c1 = Pos2::new(top.x, top.y + row_h * 0.5);
                        let c2 = Pos2::new(bottom.x, bottom.y - row_h * 0.5);
                        lanes_painter.add(egui::Shape::CubicBezier(
                            egui::epaint::CubicBezierShape::from_points_stroke(
                                [top, c1, c2, bottom],
                                false,
                                Color32::TRANSPARENT,
                                stroke,
                            ),
                        ));
                    }
                }

                draw_commit(&lanes_painter, row, lane_x(row.lane), row_y(index));

                if row.commit.is_working() {
                    draw_working(&painter, rect.left() + graph_w, row_y(index), &pending_label);
                    // Un pelo deja claro dónde empieza lo que ya está escrito.
                    painter.hline(
                        band.x_range(),
                        band.bottom(),
                        Stroke::new(1.0, theme::HAIRLINE),
                    );
                } else {
                    draw_row(&painter, ui, row, rect.left() + graph_w, row_y(index), &columns);
                }
            }
        });

    clicked
}

fn draw_commit(painter: &egui::Painter, row: &Row, x: f32, y: f32) {
    let color = theme::lane(row.color);
    let center = Pos2::new(x, y);

    // Los cambios sin confirmar van huecos y en gris: no son un commit, y
    // darles el color de la rama sería decir que ya forman parte de ella. El
    // cerco se queda al ras del punto para no comerse los trazos de su línea,
    // que es corta y tiene pocos que gastar.
    if row.commit.is_working() {
        painter.circle_filled(center, DOT_R + 1.0, theme::CANVAS);
        painter.circle_stroke(center, DOT_R - 0.5, Stroke::new(1.6, theme::MUTED));
        return;
    }

    // Un cerco del color del lienzo despega el punto de las líneas que pasan
    // por detrás; sin él el grafo se emborrona en las confluencias.
    painter.circle_filled(center, DOT_R + 2.5, theme::CANVAS);

    // Un merge se marca con un punto hueco; un commit normal, macizo.
    if row.commit.parents.len() > 1 {
        painter.circle_filled(center, DOT_R, theme::CANVAS);
        painter.circle_stroke(center, DOT_R - 0.5, Stroke::new(2.0, color));
    } else {
        painter.circle_filled(center, DOT_R, color);
    }

    // Donde está HEAD se ve desde lejos.
    if row.commit.refs.iter().any(|r| r.kind == RefKind::Head) {
        painter.circle_stroke(center, DOT_R + 3.5, Stroke::new(1.5, theme::HEAD));
    }
}

/// La fila de los cambios sin confirmar.
///
/// No lleva ni autor ni fecha ni hash, porque todavía no tiene ninguna de las
/// tres cosas. En su lugar dice cuánto hay a cada lado del índice, que es lo
/// único que se puede saber de ella.
fn draw_working(painter: &egui::Painter, left: f32, y: f32, pending: &str) {
    let title = painter.text(
        Pos2::new(left + 2.0, y),
        Align2::LEFT_CENTER,
        "Cambios sin confirmar",
        theme::medium(13.0),
        theme::TEXT,
    );

    if !pending.is_empty() {
        painter.text(
            Pos2::new(title.right() + 10.0, y),
            Align2::LEFT_CENTER,
            pending,
            theme::body(11.5),
            theme::MUTED,
        );
    }
}

/// Dónde cae cada columna de la derecha con el ancho que hay.
struct Columns {
    author: Option<f32>,
    date: Option<f32>,
    hash: Option<f32>,
    summary_right: f32,
}

impl Columns {
    fn for_width(left: f32, right: f32) -> Self {
        let mut edge = right - PAD_R;
        let mut columns = Self {
            author: None,
            date: None,
            hash: None,
            summary_right: right - PAD_R,
        };

        // De fuera hacia dentro, y cada una solo si al mensaje le queda sitio.
        for (width, slot) in [
            (HASH_W, &mut columns.hash),
            (DATE_W, &mut columns.date),
            (AUTHOR_W, &mut columns.author),
        ] {
            if edge - width - COL_GAP - left < SUMMARY_MIN {
                break;
            }
            *slot = Some(edge);
            edge -= width + COL_GAP;
        }

        columns.summary_right = edge - 8.0;
        columns
    }
}

fn draw_row(
    painter: &egui::Painter,
    ui: &egui::Ui,
    row: &Row,
    left: f32,
    y: f32,
    columns: &Columns,
) {
    let mut x = left + 2.0;

    for label in &row.commit.refs {
        x = draw_badge(painter, label, theme::lane(row.color), x, y, columns.summary_right);
    }

    let summary_font = theme::body(13.0);
    let summary = elide(ui, &row.commit.summary, columns.summary_right - x, &summary_font);
    painter.text(
        Pos2::new(x, y),
        Align2::LEFT_CENTER,
        summary,
        summary_font,
        theme::TEXT,
    );

    if let Some(edge) = columns.author {
        let font = theme::body(11.5);
        painter.text(
            Pos2::new(edge, y),
            Align2::RIGHT_CENTER,
            elide(ui, &row.commit.author, AUTHOR_W, &font),
            font,
            theme::MUTED,
        );
    }

    if let Some(edge) = columns.date {
        let when = Local
            .timestamp_opt(row.commit.time, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        painter.text(
            Pos2::new(edge, y),
            Align2::RIGHT_CENTER,
            when,
            theme::mono(11.0),
            theme::FAINT,
        );
    }

    if let Some(edge) = columns.hash {
        painter.text(
            Pos2::new(edge, y),
            Align2::RIGHT_CENTER,
            &row.commit.short_id,
            theme::mono(11.0),
            theme::FAINT,
        );
    }
}

/// Una pastilla de referencia, teñida del carril en el que vive el commit, de
/// modo que el nombre de la rama y su línea se lean como una misma hebra.
///
/// Las remotas van en hueco porque casi siempre duplican a una local, y lo que
/// interesa es que no compitan con ella.
fn draw_badge(
    painter: &egui::Painter,
    label: &gitrust_core::RefLabel,
    lane: Color32,
    x: f32,
    y: f32,
    limit: f32,
) -> f32 {
    let (text, color, filled) = match label.kind {
        RefKind::Head => (label.name.clone(), theme::HEAD, true),
        RefKind::LocalBranch => (label.name.clone(), lane, true),
        RefKind::RemoteBranch => (label.name.clone(), theme::MUTED, false),
        RefKind::Tag => (label.name.clone(), theme::lane(6), true),
    };

    let galley = painter.layout_no_wrap(text, theme::medium(11.0), color);
    let width = galley.size().x + 12.0;

    // Si la pastilla se come el mensaje, mejor no ponerla.
    if x + width > limit - 60.0 {
        return x;
    }

    let chip = Rect::from_min_size(Pos2::new(x, y - 9.0), egui::vec2(width, 18.0));
    let radius = CornerRadius::same(3);
    if filled {
        painter.rect_filled(chip, radius, theme::tint(color, 38));
    } else {
        painter.rect_stroke(
            chip,
            radius,
            Stroke::new(1.0, theme::HAIRLINE),
            egui::StrokeKind::Inside,
        );
    }

    let text_pos = Pos2::new(chip.left() + 6.0, y - galley.size().y / 2.0);
    painter.galley(text_pos, galley, color);

    chip.right() + 6.0
}

/// Recorta el texto con puntos suspensivos si no cabe en `max_w`.
fn elide(ui: &egui::Ui, text: &str, max_w: f32, font: &FontId) -> String {
    let width = |s: &str| {
        ui.fonts(|f| s.chars().map(|c| f.glyph_width(font, c)).sum::<f32>())
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
