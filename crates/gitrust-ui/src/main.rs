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

use anyhow::Result;
use eframe::egui;
use gitrust_core::{compose, layout, CommitDetail, FileDiff, Graph, Oid, RefEntry, Repo, WorkingState};

fn main() -> eframe::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 820.0])
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
    status: String,
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
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let mut app = Self {
            path,
            head: String::new(),
            status: String::new(),
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

        match repo.commits(self.limit) {
            Ok(commits) => {
                let total = commits.len();
                self.graph = layout(commits);
                self.status = format!(
                    "{total} commits · {} ramas · {} ms",
                    self.refs.len(),
                    started.elapsed().as_millis()
                );
            }
            Err(err) => return self.fail(err),
        }

        self.reload_work(&repo);
        self.reload_detail(&repo);
    }

    /// Recarga solo lo que cambia al tocar el índice.
    fn refresh(&mut self) {
        match self.repo() {
            Ok(repo) => {
                self.head = repo.head_name();
                self.reload_work(&repo);
            }
            Err(err) => self.fail(err),
        }
    }

    fn reload_work(&mut self, repo: &Repo) {
        match repo.status() {
            Ok(work) => self.work = work,
            Err(err) => {
                self.status = format!("error leyendo el estado: {err}");
                return;
            }
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
        let Some(oid) = self.selected_commit else {
            self.detail = None;
            self.detail_diff = None;
            return;
        };

        self.detail = repo.commit_detail(oid).ok();
        self.detail_diff = match (&self.detail, &self.detail_file) {
            (Some(_), Some(path)) => repo.commit_file_diff(oid, path).ok(),
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
                self.status = format!("error: {err}");
                false
            }
        }
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
        self.status = format!("error: {err}");
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
                if self.run(move |repo| repo.commit(&message).map(|_| ())) {
                    self.subject.clear();
                    self.body.clear();
                    // El commit cambia la historia: hay que redibujar el grafo.
                    self.load();
                    self.status = format!("commit creado en {}", self.head);
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("barra").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let entry = ui.add(
                    egui::TextEdit::singleline(&mut self.path)
                        .desired_width(320.0)
                        .hint_text("ruta a un repositorio Git"),
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

                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::History, "Historia");

                let pending = self.work.staged.len() + self.work.unstaged.len();
                let label = if pending > 0 {
                    format!("Cambios ({pending})")
                } else {
                    "Cambios".to_string()
                };
                ui.selectable_value(&mut self.tab, Tab::Changes, label);

                if !self.head.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(&self.head).strong());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("↻").on_hover_text("Recargar").clicked() {
                        self.load();
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("estado").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(&self.status).weak());
            ui.add_space(2.0);
        });

        // Las referencias acompañan siempre, se mire la historia o los cambios.
        let mut ref_clicked = None;
        egui::SidePanel::left("referencias")
            .resizable(true)
            .default_width(240.0)
            .show(ctx, |ui| {
                ref_clicked =
                    refs_view::show(ui, &self.refs, &mut self.ref_filter, self.selected_commit);
            });
        if let Some(oid) = ref_clicked {
            self.select_commit(oid);
            self.tab = Tab::History;
        }

        if self.tab == Tab::History {
            let mut file_clicked = None;
            egui::SidePanel::right("detalle")
                .resizable(true)
                .default_width(520.0)
                .show(ctx, |ui| {
                    file_clicked = detail_view::show(
                        ui,
                        self.detail.as_ref(),
                        self.detail_file.as_deref(),
                        self.detail_diff.as_ref(),
                    );
                });
            if let Some(path) = file_clicked {
                self.select_detail_file(path);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::History => {
                if let Some(oid) = graph_view::show(ui, &self.graph, self.selected_commit) {
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
