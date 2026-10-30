//! gitrust: cliente Git de escritorio.
//!
//! La ventana sigue el reparto de los clientes gráficos al uso: referencias a
//! la izquierda, grafo en el centro y el detalle de lo seleccionado a la
//! derecha, de modo que pinchar en el grafo enseña el diff sin cambiar de
//! pantalla.

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
    Repo, WorkingState,
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
            selected_commit: None,
            detail: None,
            detail_file: None,
            detail_diff: None,
            work: WorkingState::default(),
            selected: None,
            diff: None,
            subject: String::new(),
            body: String::new(),
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

        let repo = match self.repo() {
            Ok(repo) => repo,
            Err(err) => return self.fail(err),
        };

        self.head = repo.head_name();
        self.path = repo.workdir().display().to_string();
        self.refs = repo.references().unwrap_or_default();

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

    /// Recarga solo lo que cambia al tocar el índice.
    fn refresh(&mut self) {
        let repo = match self.repo() {
            Ok(repo) => repo,
            Err(err) => return self.fail(err),
        };

        self.head = repo.head_name();
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
    fn run(&mut self, op: impl FnOnce(&Repo) -> Result<()>) -> bool {
        match self.repo().and_then(|repo| op(&repo)) {
            Ok(()) => {
                self.refresh();
                true
            }
            Err(err) => {
                self.note_error(err.to_string());
                false
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
        self.work = WorkingState::default();
        self.selected = None;
        self.diff = None;
        self.detail = None;
        self.detail_diff = None;
        self.head.clear();
        self.summary.clear();
        self.note_error(err.to_string());
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
                    Err(err) => self.note_error(err.to_string()),
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                    let selected = self
                        .selected
                        .as_ref()
                        .map(|(path, staged)| (path.as_str(), *staged));

                    let action = changes_view::show(
                        ui,
                        &self.work,
                        selected,
                        self.diff.as_ref(),
                        &mut self.subject,
                        &mut self.body,
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
                    let entry = ui.add(
                        egui::TextEdit::singleline(&mut self.path)
                            .desired_width(300.0)
                            .margin(egui::vec2(8.0, 5.0))
                            .font(theme::mono(11.5))
                            .hint_text(
                                egui::RichText::new("ruta a un repositorio")
                                    .color(theme::FAINT),
                            ),
                    );
                    let reload = ui.button("Abrir").clicked();
                    let submitted =
                        entry.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if reload || submitted {
                        self.selected = None;
                        self.diff = None;
                        self.selected_commit = None;
                        self.detail = None;
                        self.load();
                    }

                    ui.add_space(8.0);
                    self.tabs(ui);

                    // Dónde está HEAD, con el único ámbar de la ventana.
                    if !self.head.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(&self.head)
                                .font(theme::medium(12.0))
                                .color(theme::HEAD)
                                .background_color(theme::tint(theme::HEAD, 28)),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("↻").on_hover_text("Recargar").clicked() {
                            self.load();
                        }
                    });
                });
            });
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
        let mut clicked = None;

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
                clicked =
                    refs_view::show(ui, &self.refs, &mut self.ref_filter, self.selected_commit);
            });

        if let Some(oid) = clicked {
            self.select_commit(oid);
            self.tab = Tab::History;
        }
    }

    fn detail_panel(&mut self, ctx: &egui::Context) {
        // Con la fila de cambios elegida, el panel derecho pasa a ser el árbol
        // de trabajo entero: listas, cuadro de commit y diff. Así se prepara y
        // se commitea sin salir del grafo, que es lo que hace que las dos
        // vistas se sientan una sola.
        let working = self.selected_commit.is_some_and(|oid| oid.is_zero());
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

                    action = changes_view::compact(
                        ui,
                        &self.work,
                        selected,
                        self.diff.as_ref(),
                        &mut self.subject,
                        &mut self.body,
                    );
                } else {
                    clicked = detail_view::show(
                        ui,
                        self.detail.as_ref(),
                        self.detail_file.as_deref(),
                        self.detail_diff.as_ref(),
                    );
                }
            });

        if let Some(action) = action {
            self.handle(action);
        }
        if let Some(path) = clicked {
            self.select_detail_file(path);
        }
    }
}
