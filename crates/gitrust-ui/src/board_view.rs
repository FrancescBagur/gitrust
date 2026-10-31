//! El tablero: las ramas repartidas en columnas según en qué punto están.
//!
//! No hay nada que arrastrar todavía ni nada que guardar. Lo que se ve sale
//! entero del grafo, así que no puede quedarse desfasado: en cuanto commiteas,
//! la tarjeta cambia de columna sola.

use chrono::{Local, TimeZone};
use eframe::egui::{self, Color32, RichText, Stroke};
use gitrust_core::{Board, Card, Lane};

use crate::refs_view::Pick;
use crate::theme;

/// La tarjeta que se está arrastrando ahora mismo.
#[derive(Clone)]
struct Dragged {
    branch: String,
    lane: Lane,
}

/// Lo que se ha pedido desde el tablero.
pub enum Act {
    /// Lo mismo que desde la lateral: ver un commit, o cambiar de rama.
    Pick(Pick),
    /// Llevar esa rama al remoto.
    Publish(String),
    /// Traerla a la principal.
    Land(String),
}

/// Si soltar una tarjeta de una columna en otra tiene una operación detrás.
///
/// Solo dos saltos la tienen, y por el mismo motivo los dos: son los únicos
/// en los que uno puede decir qué comando haría lo que el arrastre promete.
/// Una rama sin commits no se puede publicar, y de «hecho» no se vuelve
/// arrastrando: eso sería deshacer una fusión, que es otra cosa y más seria.
fn allows(from: Lane, to: Lane) -> bool {
    matches!(
        (from, to),
        (Lane::InProgress, Lane::InReview)
            | (Lane::InProgress, Lane::Done)
            | (Lane::InReview, Lane::Done)
    )
}

fn transition(dragged: &Dragged, to: Lane) -> Option<Act> {
    if !allows(dragged.lane, to) {
        return None;
    }
    Some(match to {
        Lane::InReview => Act::Publish(dragged.branch.clone()),
        _ => Act::Land(dragged.branch.clone()),
    })
}

/// El rótulo de cada columna y el color de su filete.
///
/// Los tres colores salen de los carriles del grafo, que es de donde sale todo
/// el color saturado de la ventana. El ámbar no está: es de HEAD, y aquí
/// compite con la rama puesta, que ya lo lleva en su tarjeta.
fn column(lane: Lane) -> (&'static str, Color32) {
    match lane {
        Lane::Todo => ("To do", theme::MUTED),
        Lane::InProgress => ("In progress", theme::lane(0)),
        Lane::InReview => ("In review", theme::lane(6)),
        Lane::Done => ("Done", theme::lane(1)),
    }
}

/// Devuelve lo que se haya pedido sobre una rama del tablero.
pub fn show(
    ui: &mut egui::Ui,
    board: &Board,
    project: &mut Option<String>,
    busy: bool,
) -> Option<Act> {
    if board.cards.is_empty() {
        theme::empty(ui, "Only the main branch so far: make a branch and it will show up here");
        return None;
    }

    let mut picked = None;

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Measured against {}", board.trunk))
                .font(theme::body(11.5))
                .color(theme::FAINT),
        );
    });

    // Los proyectos son las carpetas de los nombres de rama: `tema/colores` y
    // `tema/tipografia` son la misma faena, y la gente ya las nombra así.
    let projects = board.projects();
    if !projects.is_empty() {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            ui.spacing_mut().item_spacing.x = 4.0;

            if ui.selectable_label(project.is_none(), chip("All", project.is_none())).clicked() {
                *project = None;
            }
            for name in projects {
                let on = project.as_deref() == Some(name);
                if ui.selectable_label(on, chip(name, on)).clicked() {
                    *project = (!on).then(|| name.to_string());
                }
            }
        });
    }

    theme::rule(ui);

    let filter = project.clone();
    ui.columns(Lane::ALL.len(), |columns| {
        for (ui, lane) in columns.iter_mut().zip(Lane::ALL) {
            let cards: Vec<&Card> = board
                .lane(lane)
                .filter(|card| match &filter {
                    Some(name) => card.project.as_deref() == Some(name.as_str()),
                    None => true,
                })
                .collect();

            header(ui, lane, cards.len());

            // La columna entera recibe lo que se suelte encima.
            let mut dropped = None;
            ui.scope(|ui| {
                paint_drop_zone(ui, lane);

                let frame = egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(2, 4))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS));

                let (_, payload) = ui.dnd_drop_zone::<Dragged, _>(frame, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(column(lane).0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if cards.is_empty() {
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new("Nothing here")
                                        .font(theme::body(11.5))
                                        .color(theme::FAINT),
                                );
                                return;
                            }
                            for card in cards {
                                if let Some(act) = draw_card(ui, card, busy) {
                                    picked = Some(act);
                                }
                                ui.add_space(6.0);
                            }
                        });
                });
                dropped = payload;
            });

            if let Some(dragged) = dropped {
                if let Some(act) = transition(&dragged, lane) {
                    picked = Some(act);
                }
            }
        }
    });

    picked
}

fn chip(text: &str, on: bool) -> RichText {
    RichText::new(text)
        .font(theme::body(11.5))
        .color(if on { theme::TEXT } else { theme::MUTED })
}

/// El rótulo de una columna, con su cuenta y su filete de color.
fn header(ui: &mut egui::Ui, lane: Lane, count: usize) {
    let (title, color) = column(lane);

    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(RichText::new(title).font(theme::medium(12.0)).color(color));
        ui.label(
            RichText::new(count.to_string()).font(theme::mono(10.5)).color(theme::FAINT),
        );
    });
    ui.add_space(4.0);

    let width = ui.available_width() - 8.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 2.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, theme::tint(color, 110));
    ui.add_space(8.0);
}

/// Deja la zona de soltar del color de la columna, pero solo si lo que se
/// arrastra puede caer ahí. Una columna que se enciende para cualquier cosa
/// no está diciendo nada.
fn paint_drop_zone(ui: &mut egui::Ui, lane: Lane) {
    let accepts = egui::DragAndDrop::payload::<Dragged>(ui.ctx())
        .is_some_and(|dragged| allows(dragged.lane, lane));
    let color = column(lane).1;

    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.active.bg_fill = if accepts { theme::tint(color, 20) } else { Color32::TRANSPARENT };
    widgets.active.bg_stroke = if accepts {
        Stroke::new(1.0, color)
    } else {
        Stroke::NONE
    };
}

/// Una tarjeta: la rama, de qué proyecto es, cuánto lleva y de cuándo es.
fn draw_card(ui: &mut egui::Ui, card: &Card, busy: bool) -> Option<Act> {
    let frame = egui::Frame::NONE
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .stroke(Stroke::new(1.0, theme::HAIRLINE));

    let id = egui::Id::new(("card", &card.branch));
    let dragging = ui.ctx().is_being_dragged(id);
    let payload = Dragged { branch: card.branch.clone(), lane: card.lane };

    let inner = ui.dnd_drag_source(id, payload, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;

            // La rama puesta lleva el ámbar de HEAD, igual que en el grafo y
            // en la barra: es la misma pregunta contestada en el mismo color.
            ui.label(
                RichText::new(card.leaf())
                    .font(theme::medium(12.5))
                    .color(if card.current { theme::HEAD } else { theme::TEXT }),
            );
            ui.label(RichText::new(note(card)).font(theme::body(11.0)).color(theme::MUTED));
        });
    });

    let response = inner.response.interact(egui::Sense::click());

    // Mientras va por el aire la tarjeta se pinta en otra capa y movida, así
    // que lo que se añada aquí encima caería en el hueco que dejó.
    if dragging {
        return None;
    }

    // El realce va por el borde, no por el relleno: un relleno encima taparía
    // lo que la tarjeta dice.
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            egui::CornerRadius::same(theme::RADIUS),
            Stroke::new(1.0, theme::EDGE),
            egui::StrokeKind::Inside,
        );
    }
    if card.current {
        let bar = egui::Rect::from_min_size(
            response.rect.min,
            egui::vec2(2.0, response.rect.height()),
        );
        ui.painter().rect_filled(bar, 0.0, theme::HEAD);
    }

    let switchable = !card.current && !busy;
    let hint = match (switchable, allows(card.lane, Lane::Done)) {
        (true, true) => format!(
            "{}\nDouble-click to switch to it, or drag it to another column",
            card.branch
        ),
        (true, false) => format!("{}\nDouble-click to switch to this branch", card.branch),
        _ => card.branch.clone(),
    };
    let response = response.on_hover_text(hint);

    if switchable && response.double_clicked() {
        return Some(Act::Pick(Pick::Switch { name: card.branch.clone(), remote: false }));
    }
    response.clicked().then(|| Act::Pick(Pick::Show(card.target)))
}

/// «tema · 3 commits · hace 2 h», callando lo que no haya.
fn note(card: &Card) -> String {
    let mut parts = Vec::new();

    if let Some(project) = &card.project {
        parts.push(project.clone());
    }

    // Sin nada propio, la fecha es la del commit donde nació la rama, que no
    // cuenta nada de la rama. Se calla y se dice lo único que se sabe.
    if card.lane == Lane::Todo {
        parts.push("not started".to_string());
        return parts.join(" · ");
    }

    match card.ahead {
        0 => {}
        1 => parts.push("1 commit".to_string()),
        n => parts.push(format!("{n} commits")),
    }
    if let Some(when) = ago(card.time) {
        parts.push(when);
    }

    parts.join(" · ")
}

/// Cuánto hace, redondeado a lo que uno diría en voz alta.
fn ago(seconds: i64) -> Option<String> {
    let when = Local.timestamp_opt(seconds, 0).single()?;
    let elapsed = Local::now().signed_duration_since(when);

    let minutes = elapsed.num_minutes();
    if minutes < 1 {
        return Some("just now".to_string());
    }
    if minutes < 60 {
        return Some(format!("{minutes} min ago"));
    }

    let hours = elapsed.num_hours();
    if hours < 24 {
        return Some(format!("{hours} h ago"));
    }

    let days = elapsed.num_days();
    match days {
        0..=1 => Some("yesterday".to_string()),
        2..=30 => Some(format!("{days} days ago")),
        _ => Some(when.format("%d/%m/%Y").to_string()),
    }
}
