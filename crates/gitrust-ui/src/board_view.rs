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

/// El rótulo de cada columna y el color de su filete.
///
/// Los tres colores salen de los carriles del grafo, que es de donde sale todo
/// el color saturado de la ventana. El ámbar no está: es de HEAD, y aquí
/// compite con la rama puesta, que ya lo lleva en su tarjeta.
fn column(lane: Lane) -> (&'static str, Color32) {
    match lane {
        Lane::Todo => ("Por hacer", theme::MUTED),
        Lane::InProgress => ("En marcha", theme::lane(0)),
        Lane::InReview => ("En revisión", theme::lane(6)),
        Lane::Done => ("Hecho", theme::lane(1)),
    }
}

/// Devuelve lo que se haya pedido sobre una rama del tablero.
pub fn show(
    ui: &mut egui::Ui,
    board: &Board,
    project: &mut Option<String>,
    busy: bool,
) -> Option<Pick> {
    if board.cards.is_empty() {
        theme::empty(ui, "Solo está la rama principal: crea una rama y aparecerá aquí");
        return None;
    }

    let mut picked = None;

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("Midiendo contra {}", board.trunk))
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

            if ui.selectable_label(project.is_none(), chip("Todo", project.is_none())).clicked() {
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

            egui::ScrollArea::vertical()
                .id_salt(column(lane).0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if cards.is_empty() {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Nada aquí").font(theme::body(11.5)).color(theme::FAINT),
                        );
                        return;
                    }
                    for card in cards {
                        if let Some(pick) = draw_card(ui, card, busy) {
                            picked = Some(pick);
                        }
                        ui.add_space(6.0);
                    }
                });
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

/// Una tarjeta: la rama, de qué proyecto es, cuánto lleva y de cuándo es.
fn draw_card(ui: &mut egui::Ui, card: &Card, busy: bool) -> Option<Pick> {
    let frame = egui::Frame::NONE
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .stroke(Stroke::new(1.0, theme::HAIRLINE));

    let inner = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 3.0;

        // La rama puesta lleva el ámbar de HEAD, igual que en el grafo y en
        // la barra: es la misma pregunta contestada en el mismo color.
        ui.label(
            RichText::new(card.leaf())
                .font(theme::medium(12.5))
                .color(if card.current { theme::HEAD } else { theme::TEXT }),
        );
        ui.label(RichText::new(note(card)).font(theme::body(11.0)).color(theme::MUTED));
    });

    let response = inner.response.interact(egui::Sense::click());

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
    let response = response.on_hover_text(if switchable {
        format!("{}\nDoble clic para cambiar a esta rama", card.branch)
    } else {
        card.branch.clone()
    });

    if switchable && response.double_clicked() {
        return Some(Pick::Switch { name: card.branch.clone(), remote: false });
    }
    response.clicked().then(|| Pick::Show(card.target))
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
        parts.push("sin empezar".to_string());
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
        return Some("ahora mismo".to_string());
    }
    if minutes < 60 {
        return Some(format!("hace {minutes} min"));
    }

    let hours = elapsed.num_hours();
    if hours < 24 {
        return Some(format!("hace {hours} h"));
    }

    let days = elapsed.num_days();
    match days {
        0..=1 => Some("ayer".to_string()),
        2..=30 => Some(format!("hace {days} días")),
        _ => Some(when.format("%d/%m/%Y").to_string()),
    }
}
