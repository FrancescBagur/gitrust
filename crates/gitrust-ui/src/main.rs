//! gitrust: cliente Git de escritorio.
//!
//! La ventana sigue el reparto de los clientes gráficos al uso: referencias a
//! la izquierda, grafo en el centro y el detalle de lo seleccionado a la
//! derecha, de modo que pinchar en el grafo enseña el diff sin cambiar de
//! pantalla.

mod action_bar;
mod changes_view;
mod detail_view;
mod diff_view;
mod graph_view;
mod refs_view;
mod theme;

use anyhow::Result;
use eframe::egui;
use gitrust_core::{
    compose, layout, layout_with_working, CommitDetail, FileDiff, Graph, Oid, RefEntry, RefKind,
    Repo, StashEntry, Tracking, WorkingState,
};

fn main() -> eframe::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 900.0])
            .with_min_inner_size([900.0, 480.0])
            .with_title("gitrust"),
        ..Default::default()
    };

    eframe::run_native(
        "gitrust",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, path)))),
    )
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    History,
    Changes,
}

struct App {
    path: String,
    head: String,
    /// Lo que hay en el repositorio: se queda fijo en la barra de abajo.
    summary: String,
    /// Lo último que ha pasado. Se vacía al recargar.
    status: String,
    status_failed: bool,
    tab: Tab,

    graph: Graph,
    limit: usize,
    refs: Vec<RefEntry>,
    ref_filter: String,

    // La barra de acciones y lo que necesita saber.
    tracking: Option<Tracking>,
    stashes: Vec<StashEntry>,
    draft: action_bar::Draft,
    job: Option<Job>,

    // Commit seleccionado en el grafo.
    selected_commit: Option<Oid>,
    detail: Option<CommitDetail>,
    detail_file: Option<String>,
    detail_diff: Option<FileDiff>,

    // Árbol de trabajo.
    work: WorkingState,
    selected: Option<(String, bool)>,
    diff: Option<FileDiff>,
    subject: String,
    body: String,
    /// Si lo escrito va a rehacer el último commit.
    amend: bool,
    /// Lo que se estaba escribiendo antes de poner el interruptor, para
    /// devolverlo si se quita.
    saved: Option<(String, String)>,
    /// La etiqueta que se esté escribiendo en el panel de detalle.
    tag: detail_view::TagDraft,
}

/// Una operación que tarda, corriendo aparte.
///
/// Traer o llevar tarda lo que tarde la red, y hacerlo en el hilo que pinta
/// dejaría la ventana colgada sin explicar por qué. El hilo abre su propio
/// repositorio —`git2::Repository` no viaja entre hilos— y manda de vuelta
/// una sola cosa: la frase que toque poner en la barra de abajo.
struct Job {
    done: std::sync::mpsc::Receiver<Result<String>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, path: String) -> Self {
        theme::install(&cc.egui_ctx);

        let mut app = Self {
            path,
            head: String::new(),
            summary: String::new(),
            status: String::new(),
            status_failed: false,
            tab: Tab::History,
            graph: Graph::default(),
            limit: 5_000,
            refs: Vec::new(),
            ref_filter: String::new(),
            tracking: None,
            stashes: Vec::new(),
            draft: action_bar::Draft::default(),
            job: None,
            selected_commit: None,
            detail: None,
            detail_file: None,
            detail_diff: None,
            work: WorkingState::default(),
            selected: None,
            diff: None,
            subject: String::new(),
            body: String::new(),
            amend: false,
            saved: None,
            tag: detail_view::TagDraft::default(),
        };
        app.load();
        app
    }

    fn repo(&self) -> Result<Repo> {
        Repo::open(&self.path)
    }

    /// Recarga todo: historia, referencias y estado del árbol de trabajo.
    fn load(&mut self) {
        let started = std::time::Instant::now();
        // Una recarga cambia cuál es el último commit, así que el mensaje que
        // se hubiera traído de él ya no vale para nada.
        self.drop_amend();

        let mut repo = match self.repo() {
            Ok(repo) => repo,
            Err(err) => return self.fail(err),
        };

        self.head = repo.head_name();
        self.path = repo.workdir().display().to_string();
        self.refs = repo.references().unwrap_or_default();
        self.tracking = repo.tracking();
        self.stashes = repo.stashes().unwrap_or_default();

        // El estado del árbol va antes que el grafo: de él depende que haya o
        // no una fila de cambios sin confirmar encima de la historia.
        self.read_work(&repo);
        if let Err(err) = self.rebuild_graph(&repo) {
            return self.fail(err);
        }
        self.preselect();

        self.status = format!("leído en {} ms", started.elapsed().as_millis());
        self.status_failed = false;
        self.reload_detail(&repo);
    }

    /// Traza el grafo, con la fila de cambios sin confirmar si el árbol de
    /// trabajo tiene algo que contar.
    fn rebuild_graph(&mut self, repo: &Repo) -> Result<()> {
        let commits = repo.commits(self.limit)?;
        let total = commits.len();

        self.graph = match repo.head_id() {
            Some(head) if !self.work.is_clean() => layout_with_working(commits, head),
            _ => layout(commits),
        };

        let refs = self.refs.len();
        self.summary = format!(
            "{total} {} en {refs} {}",
            if total == 1 { "commit" } else { "commits" },
            if refs == 1 { "referencia" } else { "referencias" },
        );
        Ok(())
    }

    /// Al abrir, el panel derecho enseña dónde está HEAD en vez de recibir con
    /// un hueco: lo primero que uno quiere saber de un repositorio es qué fue
    /// lo último que pasó en él.
    fn preselect(&mut self) {
        if self.selected_commit.is_some() {
            return;
        }
        self.selected_commit = self
            .graph
            .rows
            .iter()
            .find(|row| row.commit.refs.iter().any(|r| r.kind == RefKind::Head))
            .or_else(|| self.graph.rows.first())
            .map(|row| row.commit.id);
    }

    fn has_working_row(&self) -> bool {
        self.graph.rows.first().is_some_and(|row| row.commit.is_working())
    }

    /// Quita el modo enmienda y devuelve lo que se estuviera escribiendo.
    fn drop_amend(&mut self) {
        self.amend = false;
        if let Some((subject, body)) = self.saved.take() {
            self.subject = subject;
            self.body = body;
        }
    }

    /// Si hay un commit que enmendar, y el remoto que ya lo tiene.
    ///
    /// El nombre del remoto vuelve en propiedad aposta. Prestado desde
    /// `&self`, el préstamo duraría lo que durase el valor, y el cuadro de
    /// commit no podría coger a la vez lo que se está escribiendo.
    fn amend_state(&self) -> (bool, Option<String>) {
        let amendable = self.graph.rows.iter().any(|row| !row.commit.is_working());

        // Sin nada por delante del remoto, el último commit ya está allí.
        let pushed = self
            .tracking
            .as_ref()
            .filter(|tracking| tracking.ahead == 0)
            .map(|tracking| tracking.remote.clone());

        (amendable, pushed)
    }

    /// Recarga solo lo que cambia al tocar el índice.
    fn refresh(&mut self) {
        let mut repo = match self.repo() {
            Ok(repo) => repo,
            Err(err) => return self.fail(err),
        };

        self.head = repo.head_name();
        self.stashes = repo.stashes().unwrap_or_default();
        let had_working = self.has_working_row();
        self.read_work(&repo);

        // La fila de cambios entra y sale del grafo con el árbol de trabajo:
        // si el árbol acaba de ensuciarse, o de quedarse limpio, hay que
        // volver a trazarlo para que la fila aparezca o se vaya.
        let wants_working = !self.work.is_clean();
        if had_working != wants_working {
            let _ = self.rebuild_graph(&repo);
        }

        // Y si lo elegido era esa fila y ya no existe, el cursor se va a HEAD
        // en lugar de quedarse señalando a un hueco.
        if !wants_working && self.selected_commit.is_some_and(|oid| oid.is_zero()) {
            self.selected_commit = None;
            self.preselect();
            self.reload_detail(&repo);
        }
    }

    fn read_work(&mut self, repo: &Repo) {
        match repo.status() {
            Ok(work) => self.work = work,
            Err(err) => return self.note_error(format!("no se pudo leer el estado: {err}")),
        }

        // Si el fichero seleccionado ya no está en su lado, soltamos la selección.
        if let Some((path, staged)) = self.selected.clone() {
            let list = if staged { &self.work.staged } else { &self.work.unstaged };
            if list.iter().any(|file| file.path == path) {
                self.diff = repo.file_diff(&path, staged).ok();
            } else {
                self.selected = None;
                self.diff = None;
            }
        }
    }

    fn reload_detail(&mut self, repo: &Repo) {
        // La fila de cambios sin confirmar no tiene detalle que leer: el panel
        // derecho le enseña el árbol de trabajo en su lugar.
        let Some(oid) = self.selected_commit.filter(|oid| !oid.is_zero()) else {
            self.detail = None;
            self.detail_diff = None;
            return;
        };

        self.detail = repo.commit_detail(oid).ok();

        // Si no hay fichero elegido, se abre el primero del commit. Un commit
        // de un solo fichero no debería obligar a un clic más para ver lo
        // único que trae.
        if self.detail_file.is_none() {
            self.detail_file = self
                .detail
                .as_ref()
                .and_then(|detail| detail.files.first())
                .map(|file| file.path.clone());
        }

        self.detail_diff = match &self.detail_file {
            Some(path) if self.detail.is_some() => repo.commit_file_diff(oid, path).ok(),
            _ => None,
        };
    }

    fn select_commit(&mut self, oid: Oid) {
        self.selected_commit = Some(oid);
        self.detail_file = None;
        self.detail_diff = None;

        match self.repo() {
            Ok(repo) => self.reload_detail(&repo),
            Err(err) => self.fail(err),
        }
    }

    fn select_detail_file(&mut self, path: String) {
        let Some(oid) = self.selected_commit else {
            return;
        };
        self.detail_diff = self
            .repo()
            .and_then(|repo| repo.commit_file_diff(oid, &path))
            .ok();
        self.detail_file = Some(path);
    }

    /// Ejecuta una operación sobre el repositorio y recarga el estado.
    fn run(&mut self, op: impl FnOnce(&mut Repo) -> Result<()>) -> bool {
        match self.repo().and_then(|mut repo| op(&mut repo)) {
            Ok(()) => {
                self.refresh();
                true
            }
            Err(err) => {
                self.note_error(format!("{err:#}"));
                false
            }
        }
    }

    /// Manda una operación lenta a su propio hilo.
    ///
    /// Mientras dure, la barra de acciones se apaga entera: encolar un push
    /// detrás de un pull cuando ninguno ha terminado es pedir un lío que
    /// luego hay que explicar.
    fn spawn(
        &mut self,
        doing: &str,
        work: impl FnOnce(&mut Repo) -> Result<String> + Send + 'static,
    ) {
        let (send, done) = std::sync::mpsc::channel();
        let path = self.path.clone();

        std::thread::spawn(move || {
            let _ = send.send(Repo::open(&path).and_then(|mut repo| work(&mut repo)));
        });

        self.status = format!("{doing}…");
        self.status_failed = false;
        self.job = Some(Job { done });
    }

    /// Mira si la operación de fondo ha terminado.
    ///
    /// Mientras no lo haya hecho pide que se repinte: sin eso egui se duerme
    /// en cuanto nadie toca el ratón, y el resultado se quedaría esperando a
    /// que alguien moviera la ventana.
    fn poll(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;

        let Some(job) = &self.job else {
            return;
        };

        match job.done.try_recv() {
            Ok(result) => {
                self.job = None;
                match result {
                    Ok(note) => {
                        self.load();
                        self.status = note;
                        self.status_failed = false;
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(80));
            }
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.note_error("la operación se cortó por el camino".to_string());
            }
        }
    }

    /// Deja un aviso en la barra de abajo, marcado como fallo.
    fn note_error(&mut self, message: String) {
        self.status = message;
        self.status_failed = true;
    }

    fn fail(&mut self, err: anyhow::Error) {
        self.graph = Graph::default();
        self.refs.clear();
        self.tracking = None;
        self.stashes.clear();
        self.work = WorkingState::default();
        self.selected = None;
        self.diff = None;
        self.detail = None;
        self.detail_diff = None;
        self.head.clear();
        self.summary.clear();
        self.note_error(format!("{err:#}"));
    }

    fn handle(&mut self, action: changes_view::Action) {
        use changes_view::Action;

        match action {
            Action::Select { path, staged } => {
                self.selected = Some((path.clone(), staged));
                self.diff = self.repo().and_then(|r| r.file_diff(&path, staged)).ok();
            }
            Action::Stage(path) => {
                self.run(|repo| repo.stage(&path));
            }
            Action::Unstage(path) => {
                self.run(|repo| repo.unstage(&path));
            }
            Action::StageHunk { path, hunk } => {
                self.run(|repo| repo.stage_hunk(&path, hunk));
            }
            Action::UnstageHunk { path, hunk } => {
                self.run(|repo| repo.unstage_hunk(&path, hunk));
            }
            Action::StageAll => {
                let paths: Vec<String> = self.work.unstaged.iter().map(|f| f.path.clone()).collect();
                self.run(|repo| repo.stage_all(&paths));
            }
            Action::UnstageAll => {
                let paths: Vec<String> = self.work.staged.iter().map(|f| f.path.clone()).collect();
                self.run(|repo| repo.unstage_all(&paths));
            }
            Action::AmendToggled => {
                if self.amend {
                    // Entra el mensaje del último commit, y lo que hubiera
                    // escrito se guarda: quitar el interruptor tiene que
                    // devolverlo, no haberlo perdido por el camino.
                    self.saved = Some((self.subject.clone(), self.body.clone()));
                    if let Some((subject, body)) =
                        self.repo().ok().and_then(|repo| repo.head_message())
                    {
                        self.subject = subject;
                        self.body = body;
                    }
                } else {
                    self.drop_amend();
                }
            }
            Action::Amend => {
                let message = compose(&self.subject, &self.body);
                match self.repo().and_then(|repo| repo.amend(&message)) {
                    Ok(oid) => {
                        self.saved = None;
                        self.subject.clear();
                        self.body.clear();
                        // El viejo ya no existe: el cursor va al que lo
                        // sustituye, no a un hueco.
                        self.selected_commit = Some(oid);
                        self.detail_file = None;
                        self.load();
                        self.status = "último commit rehecho".to_string();
                        self.status_failed = false;
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            Action::Commit => {
                let message = compose(&self.subject, &self.body);
                match self.repo().and_then(|repo| repo.commit(&message)) {
                    Ok(oid) => {
                        self.subject.clear();
                        self.body.clear();
                        // El cursor se queda en lo recién escrito: la fila de
                        // cambios donde estaba acaba de dejar de existir.
                        self.selected_commit = Some(oid);
                        self.detail_file = None;
                        // El commit cambia la historia: hay que redibujar el grafo.
                        self.load();
                        self.status = format!("commit creado en {}", self.head);
                        self.status_failed = false;
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
        }
    }
}

impl App {
    fn act(&mut self, action: action_bar::Action) {
        use action_bar::Action;

        match action {
            // Las dos que salen a la red se van a un hilo aparte. El resto
            // son locales y terminan antes de que se note.
            Action::Pull => self.spawn("trayendo", |repo| repo.pull()),
            Action::Push => self.spawn("llevando", |repo| repo.push()),

            Action::Branch(name) => {
                match self.repo().and_then(|repo| repo.branch_here(&name)) {
                    Ok(()) => {
                        self.load();
                        self.status = format!("ahora estás en {name}");
                        self.status_failed = false;
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }

            Action::StashSave(message) => self.local(|repo| repo.stash_save(&message)),
            Action::StashPop(index) => self.local(|repo| repo.stash_pop(index)),
            Action::StashApply(index) => self.local(|repo| repo.stash_apply(index)),
            Action::StashDrop(index) => self.local(|repo| repo.stash_drop(index)),
        }
    }

    /// Una operación local que deja dicho cómo fue.
    fn local(&mut self, op: impl FnOnce(&mut Repo) -> Result<String>) {
        match self.repo().and_then(|mut repo| op(&mut repo)) {
            Ok(note) => {
                self.refresh();
                self.status = note;
                self.status_failed = false;
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.top_bar(ctx);
        self.status_bar(ctx);
        self.refs_panel(ctx);

        if self.tab == Tab::History {
            self.detail_panel(ctx);
        }

        let frame = match self.tab {
            // El grafo se pinta sobre el fondo más hondo de la ventana para
            // que los carriles sean lo que más luz tiene en pantalla.
            Tab::History => egui::Frame::NONE.fill(theme::CANVAS),
            Tab::Changes => egui::Frame::NONE.fill(theme::PANEL),
        };

        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| match self.tab {
                Tab::History => {
                    let clicked =
                        graph_view::show(ui, &self.graph, self.selected_commit, &self.work);
                    if let Some(oid) = clicked {
                        self.select_commit(oid);
                    }
                }
                Tab::Changes => {
                    let (amendable, pushed) = self.amend_state();
                    let selected = self
                        .selected
                        .as_ref()
                        .map(|(path, staged)| (path.as_str(), *staged));

                    let mut draft = changes_view::Draft {
                        subject: &mut self.subject,
                        body: &mut self.body,
                        amend: &mut self.amend,
                        amendable,
                        pushed: pushed.as_deref(),
                    };

                    let action = changes_view::show(
                        ui,
                        &self.work,
                        selected,
                        self.diff.as_ref(),
                        &mut draft,
                    );

                    if let Some(action) = action {
                        self.handle(action);
                    }
                }
            });
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("barra")
            .frame(
                egui::Frame::NONE
                    .fill(theme::RAISED)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.repo_menu(ui);
                    theme::vrule(ui);
                    self.tabs(ui);
                    theme::vrule(ui);

                    // Dónde está HEAD, con el único ámbar de la ventana. Va
                    // pegado a las acciones porque todas van a parar a él.
                    if !self.head.is_empty() {
                        ui.label(
                            egui::RichText::new(&self.head)
                                .font(theme::medium(12.0))
                                .color(theme::HEAD)
                                .background_color(theme::tint(theme::HEAD, 28)),
                        );
                        // Un respiro antes de las acciones: el ámbar de HEAD y
                        // el de las cuentas son casi el mismo, y pegados se
                        // leerían como una sola mancha de color.
                        ui.add_space(11.0);
                    }

                    let action = action_bar::show(
                        ui,
                        self.tracking.as_ref(),
                        &self.stashes,
                        self.job.is_some(),
                        &mut self.draft,
                    );

                    let mut reload = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        reload = ui
                            .add_enabled(self.job.is_none(), egui::Button::new("↻"))
                            .on_hover_text("Recargar")
                            .clicked();
                    });

                    if let Some(action) = action {
                        self.act(action);
                    } else if reload {
                        self.load();
                    }
                });
            });
    }

    /// Qué repositorio está abierto, y cómo cambiarlo.
    ///
    /// A diario basta con el nombre de la carpeta. La ruta entera, que se
    /// comía un tercio de la barra para no decir casi nada, se ha ido al
    /// desplegable donde de verdad se usa: al abrir otro repositorio.
    fn repo_menu(&mut self, ui: &mut egui::Ui) {
        let name = std::path::Path::new(self.path.trim_end_matches('/'))
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repositorio".to_string());

        let button =
            ui.button(egui::RichText::new(format!("{name}     ")).font(theme::medium(12.5)));
        theme::caret(ui, &button);
        let hint = self.path.clone();
        let button = button.on_hover_text(hint);
        let id = ui.make_persistent_id("menu-repo");

        if button.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(id));
        }

        let open = egui::popup::popup_below_widget(
            ui,
            id,
            &button,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_min_width(430.0);
                let mut go = false;

                ui.horizontal(|ui| {
                    let entry = ui.add(
                        egui::TextEdit::singleline(&mut self.path)
                            .desired_width(ui.available_width() - 66.0)
                            .margin(egui::vec2(8.0, 5.0))
                            .font(theme::mono(11.5))
                            .hint_text(
                                egui::RichText::new("ruta a un repositorio")
                                    .color(theme::FAINT),
                            ),
                    );
                    go = ui.button("Abrir").clicked()
                        || (entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                });

                if go {
                    ui.memory_mut(|memory| memory.close_popup());
                }
                go
            },
        )
        .unwrap_or(false);

        if open {
            self.selected = None;
            self.diff = None;
            self.selected_commit = None;
            self.detail = None;
            self.load();
        }
    }

    /// Las dos vistas, pegadas como un mando de dos posiciones.
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let pending = self.work.staged.len() + self.work.unstaged.len();

        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.selectable_value(&mut self.tab, Tab::History, "Historia");

            let changes = ui.selectable_value(&mut self.tab, Tab::Changes, "Cambios");
            if pending > 0 {
                ui.label(
                    egui::RichText::new(pending.to_string())
                        .font(theme::mono(10.5))
                        .color(theme::CHANGED)
                        .background_color(theme::tint(theme::CHANGED, 32)),
                )
                .on_hover_text("cambios sin commitear");
            }
            let _ = changes;
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("estado")
            .frame(
                egui::Frame::NONE
                    .fill(theme::RAISED)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&self.summary)
                            .font(theme::body(11.5))
                            .color(theme::MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if self.status_failed {
                            theme::REMOVED
                        } else {
                            theme::FAINT
                        };
                        ui.label(
                            egui::RichText::new(&self.status)
                                .font(theme::body(11.5))
                                .color(color),
                        );
                    });
                });
            });
    }

    /// Las referencias acompañan siempre, se mire la historia o los cambios.
    fn refs_panel(&mut self, ctx: &egui::Context) {
        let mut picked = None;

        egui::SidePanel::left("referencias")
            .resizable(true)
            .default_width(222.0)
            .width_range(170.0..=360.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ctx, |ui| {
                picked = refs_view::show(
                    ui,
                    &self.refs,
                    &mut self.ref_filter,
                    self.selected_commit,
                    self.job.is_some(),
                );
            });

        match picked {
            Some(refs_view::Pick::Show(oid)) => {
                self.select_commit(oid);
                self.tab = Tab::History;
            }
            Some(refs_view::Pick::Switch { name, remote }) => self.switch(name, remote),
            Some(refs_view::Pick::DeleteTag(name)) => {
                match self.repo().and_then(|repo| repo.delete_tag(&name)) {
                    Ok(()) => {
                        self.load();
                        self.status = format!("etiqueta {name} borrada");
                        self.status_failed = false;
                    }
                    Err(err) => self.note_error(format!("{err:#}")),
                }
            }
            None => {}
        }
    }

    /// Se cambia de rama en un hilo aparte.
    ///
    /// Es local, pero es la operación que más ficheros puede escribir de todas
    /// las que hay aquí: entre dos ramas lejanas se reescribe medio árbol, y
    /// eso con la ventana congelada se lee como que la aplicación ha muerto.
    fn switch(&mut self, name: String, remote: bool) {
        // La pestaña se queda donde esté: si uno estaba en los cambios es que
        // estaba trabajando en ficheros, y cambiar de rama no es motivo para
        // mandarlo a otra pantalla.
        //
        // El cursor sí se suelta antes de salir: al llegar a la rama nueva lo
        // que uno quiere ver es su último commit, no el sitio de la rama
        // vieja donde se había quedado mirando.
        self.selected_commit = None;
        self.detail_file = None;

        let doing = format!("cambiando a {name}");
        self.spawn(&doing, move |repo| {
            if remote {
                repo.checkout_remote(&name)
            } else {
                repo.checkout_branch(&name)
            }
        });
    }

    fn detail_panel(&mut self, ctx: &egui::Context) {
        // Con la fila de cambios elegida, el panel derecho pasa a ser el árbol
        // de trabajo entero: listas, cuadro de commit y diff. Así se prepara y
        // se commitea sin salir del grafo, que es lo que hace que las dos
        // vistas se sientan una sola.
        let working = self.selected_commit.is_some_and(|oid| oid.is_zero());
        let (amendable, pushed) = self.amend_state();
        let mut clicked = None;
        let mut action = None;

        egui::SidePanel::right("detalle")
            .resizable(true)
            .default_width(420.0)
            // Con tope: sin él, un commit de mensaje largo ensancha el panel
            // hasta dejar la columna del grafo en nada.
            .width_range(300.0..=520.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ctx, |ui| {
                if working {
                    let selected = self
                        .selected
                        .as_ref()
                        .map(|(path, staged)| (path.as_str(), *staged));

                    let mut draft = changes_view::Draft {
                        subject: &mut self.subject,
                        body: &mut self.body,
                        amend: &mut self.amend,
                        amendable,
                        pushed: pushed.as_deref(),
                    };

                    action = changes_view::compact(
                        ui,
                        &self.work,
                        selected,
                        self.diff.as_ref(),
                        &mut draft,
                    );
                } else {
                    clicked = detail_view::show(
                        ui,
                        self.detail.as_ref(),
                        self.detail_file.as_deref(),
                        self.detail_diff.as_ref(),
                        &mut self.tag,
                    );
                }
            });

        if let Some(action) = action {
            self.handle(action);
        }
        match clicked {
            Some(detail_view::Pick::File(path)) => self.select_detail_file(path),
            Some(detail_view::Pick::Tag { name, message }) => self.create_tag(name, message),
            None => {}
        }
    }

    /// Etiqueta el commit que se está mirando.
    fn create_tag(&mut self, name: String, message: String) {
        let Some(oid) = self.selected_commit.filter(|oid| !oid.is_zero()) else {
            return self.note_error("no hay ningún commit elegido".to_string());
        };

        match self.repo().and_then(|repo| repo.tag(&name, oid, &message)) {
            Ok(()) => {
                // La etiqueta nueva tiene que salir en el grafo y en la
                // lateral, y eso solo pasa releyendo las referencias.
                self.load();
                self.status = format!("etiqueta {name} creada");
                self.status_failed = false;
            }
            Err(err) => self.note_error(format!("{err:#}")),
        }
    }
}
