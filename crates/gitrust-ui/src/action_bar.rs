//! Lo que se le hace al repositorio entero, en una sola fila.
//!
//! Son cuatro cosas y las cuatro miran a la rama actual: traer lo que falta,
//! llevar lo que sobra, abrir una rama nueva donde uno está, y apartar lo que
//! estorba. Las dos primeras llevan su cuenta al lado, porque saber cuánto se
//! debe es la mitad de decidir si pulsar.

use chrono::{Local, TimeZone};
use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat},
    Color32, PopupCloseBehavior, RichText,
};
use gitrust_core::{PullMode, StashEntry, Tracking};

use crate::theme;

/// Lo que ocupa un apartado en la lista, con su holgura. Se usa para darle
/// el alto a la lista, que no sabe medirse sola ahí dentro.
const STASH_ROW: f32 = 52.0;

pub enum Action {
    /// Traer y juntar, de la manera que se diga.
    Pull(PullMode),
    /// Traer de todos los remotos y no juntar nada.
    Fetch,
    Push,
    PushForce,
    PushTags,
    Branch(String),
    StashSave(String),
    StashApply(usize),
    StashPop(usize),
    StashDrop(usize),
}

/// Lo que se está escribiendo en los desplegables. Vive en la aplicación
/// porque tiene que seguir ahí al fotograma siguiente.
#[derive(Default)]
pub struct Draft {
    branch: String,
    stash: String,
}

/// Pinta el grupo y devuelve lo que se haya pulsado.
///
/// Con `busy` en cierto hay una operación en marcha: los botones se apagan
/// enteros en lugar de dejar encolar una segunda mientras la primera va por
/// la red.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    tracking: Option<&Tracking>,
    stashes: &[StashEntry],
    mode: &mut PullMode,
    busy: bool,
    draft: &mut Draft,
) -> Option<Action> {
    let mut action = None;

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;

        let behind = tracking.map(|t| t.behind).unwrap_or(0);
        let ahead = tracking.map(|t| t.ahead).unwrap_or(0);

        // Sin rama remota a la que mirar no hay de dónde traer; llevar sí se
        // puede, y es justamente así como se crea.
        let pull = count_button(ui, "↓ Pull", behind, tracking.is_some() && !busy);
        if pull.on_hover_text(pull_hint(tracking, *mode)).clicked() {
            action = Some(Action::Pull(*mode));
        }

        ui.spacing_mut().item_spacing.x = 1.0;
        if let Some(other) = pull_menu(ui, busy, mode) {
            action = Some(other);
        }
        ui.spacing_mut().item_spacing.x = 4.0;

        let push = count_button(ui, "↑ Push", ahead, !busy);
        if push.on_hover_text(push_hint(tracking)).clicked() {
            action = Some(Action::Push);
        }

        // La punta va pegada al botón: es el mismo mando con dos posturas,
        // y lo que hay detrás no se pulsa de pasada.
        ui.spacing_mut().item_spacing.x = 1.0;
        if let Some(other) = push_menu(ui, busy) {
            action = Some(other);
        }
        ui.spacing_mut().item_spacing.x = 4.0;

        if let Some(name) = branch_menu(ui, busy, &mut draft.branch) {
            action = Some(Action::Branch(name));
        }
        if let Some(done) = stash_menu(ui, busy, stashes, &mut draft.stash) {
            action = Some(done);
        }
    });

    action
}

/// Un botón con su cuenta pegada, en la misma pastilla ámbar que lleva la
/// pestaña de cambios: significa lo mismo, que hay algo tuyo pendiente.
fn count_button(ui: &mut egui::Ui, label: &str, count: usize, enabled: bool) -> egui::Response {
    let ink = if enabled { theme::TEXT } else { theme::FAINT };

    let mut job = LayoutJob::default();
    job.append(label, 0.0, TextFormat::simple(theme::medium(12.5), ink));

    if count > 0 {
        job.append(
            &format!(" {count} "),
            6.0,
            TextFormat {
                font_id: theme::mono(10.5),
                color: if enabled { theme::CHANGED } else { theme::FAINT },
                background: if enabled {
                    theme::tint(theme::CHANGED, 32)
                } else {
                    Color32::TRANSPARENT
                },
                ..Default::default()
            },
        );
    }

    ui.add_enabled(enabled, egui::Button::new(job))
}

fn pull_hint(tracking: Option<&Tracking>, mode: PullMode) -> String {
    let how = match mode {
        PullMode::Merge => "merging if it has to",
        PullMode::FastForward => "fast-forward only",
        PullMode::Rebase => "rebasing",
    };
    match tracking {
        Some(t) if t.behind > 0 => format!("pull {} commits from {}, {how}", t.behind, t.remote),
        Some(t) => format!("{} has nothing new", t.remote),
        None => "this branch tracks no remote one".to_string(),
    }
}

/// Las otras maneras de traer.
///
/// Lo que se elige aquí se hace al momento y se queda de postura del botón:
/// el botón repite lo último que pediste, que es lo que uno espera de un
/// mando con memoria y no hay que explicarlo en ninguna parte.
fn pull_menu(ui: &mut egui::Ui, busy: bool, mode: &mut PullMode) -> Option<Action> {
    let button = ui.add_enabled(!busy, egui::Button::new("").min_size(egui::vec2(20.0, 0.0)));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-pull");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }

    egui::popup::popup_below_widget(ui, id, &button, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(248.0);
        let mut action = None;

        // Traer no junta nada, así que no cambia lo que hace el botón.
        if ui
            .button("Fetch all")
            .on_hover_text("from every remote, touching nothing here")
            .clicked()
        {
            action = Some(Action::Fetch);
        }

        theme::rule(ui);

        for (label, what) in [
            ("Pull (fast-forward if possible)", PullMode::Merge),
            ("Pull (fast-forward only)", PullMode::FastForward),
            ("Pull (rebase)", PullMode::Rebase),
        ] {
            if ui.selectable_label(*mode == what, label).clicked() {
                *mode = what;
                action = Some(Action::Pull(what));
            }
        }

        if action.is_some() {
            ui.memory_mut(|memory| memory.close_popup());
        }
        action
    })
    .flatten()
}

fn push_hint(tracking: Option<&Tracking>) -> String {
    match tracking {
        Some(t) if t.ahead > 0 => format!("push {} commits to {}", t.ahead, t.remote),
        Some(t) => format!("{} already has everything", t.remote),
        None => "push this branch for the first time".to_string(),
    }
}

/// Las dos maneras de llevar que no son la de todos los días.
fn push_menu(ui: &mut egui::Ui, busy: bool) -> Option<Action> {
    // Sin texto: el ancho sale del relleno del botón, y así la punta cae en
    // su centro en vez de dejar medio botón vacío a la izquierda.
    let button = ui.add_enabled(!busy, egui::Button::new("").min_size(egui::vec2(20.0, 0.0)));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-push");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }

    egui::popup::popup_below_widget(ui, id, &button, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(234.0);
        let mut action = None;

        if ui
            .button("Force the push")
            .on_hover_text("refuses if anyone has touched the branch since you fetched")
            .clicked()
        {
            action = Some(Action::PushForce);
        }
        if ui
            .button("Send the tags")
            .on_hover_text("the ones the remote does not have yet")
            .clicked()
        {
            action = Some(Action::PushTags);
        }

        if action.is_some() {
            ui.memory_mut(|memory| memory.close_popup());
        }
        action
    })
    .flatten()
}

/// El desplegable de rama: solo crea, y crea donde uno está.
///
/// Cambiar a una rama que ya existe reescribe ficheros en disco, y eso merece
/// su propio sitio con su aviso. Esto no toca nada: la rama nueva apunta al
/// mismo commit, así que lo que haya a medias sigue exactamente igual.
fn branch_menu(ui: &mut egui::Ui, busy: bool, name: &mut String) -> Option<String> {
    let button = ui.add_enabled(!busy, egui::Button::new(label("Branch")));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-branch");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }

    egui::popup::popup_below_widget(ui, id, &button, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(250.0);
        ui.label(
            RichText::new("New branch right here")
                .font(theme::medium(12.0))
                .color(theme::TEXT),
        );
        ui.add_space(5.0);

        let entry = ui.add(
            egui::TextEdit::singleline(name)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(8.0, 5.0))
                .font(theme::mono(12.0))
                .hint_text(RichText::new("branch name").color(theme::FAINT)),
        );
        ui.add_space(5.0);

        let entered = entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let pressed = ui
            .add_enabled(!name.trim().is_empty(), egui::Button::new("Create and switch"))
            .clicked();

        ui.add_space(2.0);
        ui.label(
            RichText::new("The working tree is left alone")
                .font(theme::body(11.0))
                .color(theme::FAINT),
        );

        (entered || pressed).then(|| {
            ui.memory_mut(|memory| memory.close_popup());
            std::mem::take(name)
        })
    })
    .flatten()
}

fn stash_menu(
    ui: &mut egui::Ui,
    busy: bool,
    stashes: &[StashEntry],
    message: &mut String,
) -> Option<Action> {
    let mut job = LayoutJob::default();
    job.append("Stash", 0.0, TextFormat::simple(theme::medium(12.5), theme::TEXT));
    if !stashes.is_empty() {
        job.append(
            &format!(" {} ", stashes.len()),
            6.0,
            TextFormat {
                font_id: theme::mono(10.5),
                color: theme::MUTED,
                background: theme::tint(theme::MUTED, 26),
                ..Default::default()
            },
        );
    }
    job.append("     ", 0.0, TextFormat::simple(theme::body(11.0), theme::MUTED));

    let button = ui.add_enabled(!busy, egui::Button::new(job));
    theme::caret(ui, &button);
    let id = ui.make_persistent_id("menu-stash");

    if button.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(id));
    }

    egui::popup::popup_below_widget(ui, id, &button, PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(370.0);
        let mut action = None;

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(message)
                    .desired_width(ui.available_width() - 76.0)
                    .margin(egui::vec2(8.0, 5.0))
                    .font(theme::body(12.0))
                    .hint_text(RichText::new("what you are setting aside").color(theme::FAINT)),
            );
            if ui.button("Stash").clicked() {
                action = Some(Action::StashSave(std::mem::take(message)));
            }
        });

        if stashes.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Nothing stashed")
                    .font(theme::body(11.5))
                    .color(theme::FAINT),
            );
        } else {
            theme::rule(ui);

            // El alto va dicho, no pedido. Dentro de un desplegable que se
            // dimensiona solo, un ScrollArea que pregunta cuánto sitio hay
            // recibe menos del que hay y recorta la última fila.
            let height = stashes.len().min(6) as f32 * STASH_ROW;
            egui::ScrollArea::vertical()
                .id_salt("pila-stash")
                .max_height(height)
                .min_scrolled_height(height)
                .show(ui, |ui| {
                    for entry in stashes {
                        if let Some(done) = stash_row(ui, entry) {
                            action = Some(done);
                        }
                    }
                });
        }

        if action.is_some() {
            ui.memory_mut(|memory| memory.close_popup());
        }
        action
    })
    .flatten()
}

/// Un apartado: de qué es, de dónde y de cuándo, y lo que se le puede hacer.
fn stash_row(ui: &mut egui::Ui, entry: &StashEntry) -> Option<Action> {
    let mut action = None;
    let (branch, text) = parts(&entry.message);

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add(egui::Label::new(RichText::new(text).font(theme::body(12.0)).color(theme::TEXT)))
            .on_hover_text(&entry.message);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // De qué rama salió: apartar en una y sacar en otra es fácil de
        // hacer sin querer, y esto es lo único que avisa.
        ui.label(RichText::new(branch).font(theme::body(11.0)).color(theme::MUTED));
        if let Some(when) = when(entry.time) {
            ui.label(RichText::new("·").font(theme::body(11.0)).color(theme::FAINT));
            ui.label(RichText::new(when).font(theme::mono(10.5)).color(theme::FAINT));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);

            if ui.button(small("Drop", theme::MUTED)).on_hover_text("there is no getting it back").clicked() {
                action = Some(Action::StashDrop(entry.index));
            }
            if ui.button(small("Apply", theme::MUTED)).on_hover_text("and it stays stashed").clicked()
            {
                action = Some(Action::StashApply(entry.index));
            }
            if ui.button(small("Pop", theme::TEXT)).on_hover_text("and it leaves the stack").clicked() {
                action = Some(Action::StashPop(entry.index));
            }
        });
    });
    });
    ui.add_space(4.0);

    action
}

/// Parte «On master: medir el carril» en la rama y lo que uno escribió.
///
/// El prefijo que pone Git es la misma frase en todos, así que leído en una
/// lista no distingue nada: lo que separa a un apartado de otro va detrás.
fn parts(message: &str) -> (&str, &str) {
    let Some((head, text)) = message.split_once(": ") else {
        return ("", message);
    };
    let branch = head.rsplit(' ').next().unwrap_or("");

    // El mensaje automático trae el hash delante del asunto del commit, que
    // sobra: lo que interesa es de qué iba lo apartado.
    let text = text
        .split_once(' ')
        .filter(|(first, _)| first.len() >= 7 && first.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|(_, rest)| rest)
        .unwrap_or(text);

    (branch, text)
}

fn small(text: &str, color: Color32) -> RichText {
    RichText::new(text).font(theme::body(11.0)).color(color)
}

/// El texto de un botón que abre un desplegable, con hueco al final para la
/// punta que pinta `theme::caret`.
fn label(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat::simple(theme::medium(12.5), theme::TEXT));
    job.append("     ", 0.0, TextFormat::simple(theme::body(11.0), theme::MUTED));
    job
}

fn when(seconds: i64) -> Option<String> {
    (seconds > 0)
        .then(|| Local.timestamp_opt(seconds, 0).single())
        .flatten()
        .map(|dt| dt.format("%d/%m %H:%M").to_string())
}
