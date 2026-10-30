//! Apertura del repositorio, lectura de commits y operaciones sobre el índice.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{
    ApplyLocation, ApplyOptions, Delta, Diff, DiffFormat, DiffHunk, DiffOptions, ObjectType, Oid,
    Repository, Sort, StatusOptions,
};

use crate::detail::{CommitDetail, CommitFile, RefEntry};
use crate::diff::{DiffLine, FileDiff, Hunk, HunkId};
use crate::model::{Commit, RefKind, RefLabel};
use crate::status::{staged_kind, unstaged_kind, ChangeKind, FileChange, WorkingState};

pub struct Repo {
    inner: Repository,
}

impl Repo {
    /// Abre el repositorio que contiene `path`, subiendo por los directorios
    /// padre igual que hace `git` desde una subcarpeta.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let inner = Repository::discover(path)
            .with_context(|| format!("no hay un repositorio Git en {}", path.display()))?;
        Ok(Self { inner })
    }

    /// Ruta del árbol de trabajo, o del propio repositorio si es bare.
    pub fn workdir(&self) -> PathBuf {
        self.inner
            .workdir()
            .unwrap_or_else(|| self.inner.path())
            .to_path_buf()
    }

    /// Nombre de la rama actual, o el hash corto si HEAD está desacoplado.
    pub fn head_name(&self) -> String {
        match self.inner.head() {
            Ok(head) => head
                .shorthand()
                .map(str::to_string)
                .unwrap_or_else(|| "HEAD desacoplado".to_string()),
            Err(_) => "sin commits".to_string(),
        }
    }

    /// El commit al que apunta HEAD, si el repositorio tiene alguno.
    pub fn head_id(&self) -> Option<Oid> {
        self.inner.head().ok()?.peel(ObjectType::Commit).ok().map(|object| object.id())
    }

    // ---------------------------------------------------------------- historia

    /// Recorre el historial de todas las referencias y devuelve hasta `limit`
    /// commits en orden topológico y por fecha, del más reciente al más antiguo.
    pub fn commits(&self, limit: usize) -> Result<Vec<Commit>> {
        let labels = self.ref_labels()?;

        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;

        // Un glob falla si no hay ninguna referencia que encaje, cosa normal en
        // repositorios recién creados o sin remotos. No es un error para nosotros.
        let _ = walk.push_glob("refs/heads/*");
        let _ = walk.push_glob("refs/remotes/*");
        let _ = walk.push_glob("refs/tags/*");
        if let Ok(head) = self.inner.head() {
            if let Some(oid) = head.target() {
                let _ = walk.push(oid);
            }
        }

        let mut out = Vec::new();
        for oid in walk.take(limit) {
            let oid = oid?;
            let commit = self.inner.find_commit(oid)?;
            let author = commit.author();

            out.push(Commit {
                id: oid,
                short_id: oid.to_string().chars().take(7).collect(),
                summary: commit.summary().unwrap_or("(sin mensaje)").to_string(),
                author: author.name().unwrap_or("(desconocido)").to_string(),
                time: commit.time().seconds(),
                parents: commit.parent_ids().collect(),
                refs: labels.get(&oid).cloned().unwrap_or_default(),
            });
        }

        Ok(out)
    }

    /// Mapa de commit a las referencias que lo señalan.
    fn ref_labels(&self) -> Result<HashMap<Oid, Vec<RefLabel>>> {
        let mut map: HashMap<Oid, Vec<RefLabel>> = HashMap::new();

        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(name) = reference.name() else {
                continue;
            };
            // Un tag anotado apunta a un objeto tag, no al commit: hay que pelarlo.
            let Ok(object) = reference.peel(ObjectType::Commit) else {
                continue;
            };

            let label = if let Some(branch) = name.strip_prefix("refs/heads/") {
                RefLabel { name: branch.to_string(), kind: RefKind::LocalBranch }
            } else if let Some(branch) = name.strip_prefix("refs/remotes/") {
                RefLabel { name: branch.to_string(), kind: RefKind::RemoteBranch }
            } else if let Some(tag) = name.strip_prefix("refs/tags/") {
                RefLabel { name: tag.to_string(), kind: RefKind::Tag }
            } else {
                continue;
            };

            map.entry(object.id()).or_default().push(label);
        }

        if let Ok(head) = self.inner.head() {
            if let Ok(object) = head.peel(ObjectType::Commit) {
                map.entry(object.id())
                    .or_default()
                    .insert(0, RefLabel { name: "HEAD".to_string(), kind: RefKind::Head });
            }
        }

        Ok(map)
    }

    // ------------------------------------------------------- árbol de trabajo

    /// Ficheros cambiados, separados entre índice y árbol de trabajo. Un mismo
    /// fichero puede aparecer en ambos lados si tiene cambios a medio preparar.
    pub fn status(&self) -> Result<WorkingState> {
        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);

        let statuses = self.inner.statuses(Some(&mut options))?;
        let mut state = WorkingState::default();

        for entry in statuses.iter() {
            let Some(path) = entry.path() else {
                continue;
            };
            let status = entry.status();

            if let Some(kind) = staged_kind(status) {
                state.staged.push(FileChange { path: path.to_string(), kind, staged: true });
            }
            if let Some(kind) = unstaged_kind(status) {
                state.unstaged.push(FileChange { path: path.to_string(), kind, staged: false });
            }
        }

        state.staged.sort_by(|a, b| a.path.cmp(&b.path));
        state.unstaged.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(state)
    }

    /// Diff de un fichero. Con `staged` compara HEAD contra el índice; sin él,
    /// el índice contra el árbol de trabajo.
    pub fn file_diff(&self, path: &str, staged: bool) -> Result<FileDiff> {
        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3).include_untracked(true).recurse_untracked_dirs(true);

        let diff = if staged {
            let tree = match self.inner.head() {
                Ok(head) => Some(head.peel_to_tree()?),
                Err(_) => None,
            };
            self.inner.diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?
        } else {
            self.inner.diff_index_to_workdir(None, Some(&mut options))?
        };

        collect_file_diff(&diff, path)
    }

    // ------------------------------------------------------------- un commit

    /// Mensaje, autoría y ficheros tocados por un commit, comparándolo con su
    /// primer padre. Un commit raíz se compara con el árbol vacío.
    pub fn commit_detail(&self, oid: Oid) -> Result<CommitDetail> {
        let commit = self.inner.find_commit(oid)?;
        let author = commit.author();

        let tree = commit.tree()?;
        let parent_tree = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };
        let diff = self
            .inner
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;

        let mut files: Vec<CommitFile> = diff
            .deltas()
            .filter_map(|delta| {
                let path = delta.new_file().path().or_else(|| delta.old_file().path())?;
                Some(CommitFile {
                    path: path.display().to_string(),
                    kind: delta_kind(delta.status()),
                })
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));

        // El mensaje es «asunto\n\ncuerpo»; guardamos las dos partes aparte.
        let message = commit.message().unwrap_or("");
        let body = message
            .split_once("\n\n")
            .map(|(_, body)| body.trim_end().to_string())
            .unwrap_or_default();

        Ok(CommitDetail {
            id: oid,
            short_id: oid.to_string().chars().take(7).collect(),
            summary: commit.summary().unwrap_or("(sin mensaje)").to_string(),
            body,
            author: author.name().unwrap_or("(desconocido)").to_string(),
            email: author.email().unwrap_or_default().to_string(),
            time: commit.time().seconds(),
            parents: commit.parent_ids().collect(),
            files,
        })
    }

    /// Diff de un fichero dentro de un commit, contra su primer padre.
    pub fn commit_file_diff(&self, oid: Oid, path: &str) -> Result<FileDiff> {
        let commit = self.inner.find_commit(oid)?;
        let tree = commit.tree()?;
        let parent_tree = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };

        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3);

        let diff = self.inner.diff_tree_to_tree(
            parent_tree.as_ref(),
            Some(&tree),
            Some(&mut options),
        )?;
        collect_file_diff(&diff, path)
    }

    /// Todas las referencias, para la barra lateral. Se omite el `HEAD` de
    /// cada remoto, que es un alias y no una rama más.
    pub fn references(&self) -> Result<Vec<RefEntry>> {
        let head = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string));

        let mut out = Vec::new();
        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(name) = reference.name() else {
                continue;
            };
            if name.ends_with("/HEAD") {
                continue;
            }
            let Ok(object) = reference.peel(ObjectType::Commit) else {
                continue;
            };

            let (short, kind) = if let Some(branch) = name.strip_prefix("refs/heads/") {
                (branch.to_string(), RefKind::LocalBranch)
            } else if let Some(branch) = name.strip_prefix("refs/remotes/") {
                (branch.to_string(), RefKind::RemoteBranch)
            } else if let Some(tag) = name.strip_prefix("refs/tags/") {
                (tag.to_string(), RefKind::Tag)
            } else {
                continue;
            };

            let is_head = kind == RefKind::LocalBranch && head.as_deref() == Some(short.as_str());
            out.push(RefEntry { name: short, kind, target: object.id(), is_head });
        }

        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Añade un fichero al índice. Si ya no existe en disco lo marca borrado.
    pub fn stage(&self, path: &str) -> Result<()> {
        let mut index = self.inner.index()?;
        let exists = self.inner.workdir().map(|dir| dir.join(path).exists()).unwrap_or(false);

        if exists {
            index.add_path(Path::new(path))?;
        } else {
            index.remove_path(Path::new(path))?;
        }
        index.write()?;
        Ok(())
    }

    /// Devuelve un fichero del índice al estado que tiene en HEAD.
    pub fn unstage(&self, path: &str) -> Result<()> {
        match self.inner.head() {
            Ok(head) => {
                let object = head.peel(ObjectType::Commit)?;
                self.inner.reset_default(Some(&object), [path])?;
            }
            // Sin HEAD todavía no hay nada a lo que volver: basta con sacarlo.
            Err(_) => {
                let mut index = self.inner.index()?;
                index.remove_path(Path::new(path))?;
                index.write()?;
            }
        }
        Ok(())
    }

    /// Prepara un solo hunk, dejando el resto del fichero sin preparar.
    ///
    /// Aplica al índice el diff del árbol de trabajo filtrado a ese hunk, que
    /// es lo que hace `git add -p` por dentro.
    pub fn stage_hunk(&self, path: &str, hunk: HunkId) -> Result<()> {
        let mut options = DiffOptions::new();
        options
            .pathspec(path)
            .context_lines(3)
            .include_untracked(true)
            .recurse_untracked_dirs(true);

        let diff = self.inner.diff_index_to_workdir(None, Some(&mut options))?;
        self.apply_hunk(&diff, hunk)
    }

    /// Devuelve un solo hunk del índice al árbol de trabajo.
    ///
    /// El diff se calcula invertido, de modo que aplicarlo al índice deshace
    /// el cambio. En un diff invertido los lados están intercambiados, así que
    /// el hunk se busca con sus rangos también invertidos.
    pub fn unstage_hunk(&self, path: &str, hunk: HunkId) -> Result<()> {
        let mut options = DiffOptions::new();
        options.pathspec(path).context_lines(3).reverse(true);

        let tree = match self.inner.head() {
            Ok(head) => Some(head.peel_to_tree()?),
            Err(_) => None,
        };
        let diff = self
            .inner
            .diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?;

        self.apply_hunk(&diff, hunk.reversed())
    }

    fn apply_hunk(&self, diff: &Diff<'_>, wanted: HunkId) -> Result<()> {
        let mut applied = false;
        {
            let mut options = ApplyOptions::new();
            options.hunk_callback(|hunk| match hunk {
                Some(hunk) => {
                    let matches = hunk_id(&hunk) == wanted;
                    applied |= matches;
                    matches
                }
                // Sin hunk es la cabecera del fichero: debe pasar.
                None => true,
            });

            self.inner
                .apply(diff, ApplyLocation::Index, Some(&mut options))?;
        }

        anyhow::ensure!(applied, "el hunk ya no existe; recarga el diff");
        Ok(())
    }

    pub fn stage_all(&self, paths: &[String]) -> Result<()> {
        for path in paths {
            self.stage(path)?;
        }
        Ok(())
    }

    pub fn unstage_all(&self, paths: &[String]) -> Result<()> {
        for path in paths {
            self.unstage(path)?;
        }
        Ok(())
    }

    /// Crea un commit con lo que haya en el índice.
    pub fn commit(&self, message: &str) -> Result<Oid> {
        let message = message.trim();
        anyhow::ensure!(!message.is_empty(), "el mensaje del commit está vacío");

        let signature = self
            .inner
            .signature()
            .context("falta configurar user.name y user.email en Git")?;

        let mut index = self.inner.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.inner.find_tree(tree_id)?;

        let parent = match self.inner.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(_) => None,
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();

        let oid = self
            .inner
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)?;
        Ok(oid)
    }
}

/// Recorre un diff ya calculado y lo trocea en hunks y líneas.
fn collect_file_diff(diff: &Diff<'_>, path: &str) -> Result<FileDiff> {
    let mut out = FileDiff { path: path.to_string(), ..Default::default() };

    diff.print(DiffFormat::Patch, |_delta, hunk, line| {
        match line.origin() {
            // Cabecera del fichero: ya sabemos de qué fichero se trata.
            'F' => {}
            'B' => out.binary = true,
            'H' => {
                let header = String::from_utf8_lossy(line.content()).trim_end().to_string();
                let id = hunk.as_ref().map(hunk_id).unwrap_or_default();
                out.hunks.push(Hunk { header, id, lines: Vec::new() });
            }
            origin => {
                // Un fichero sin seguimiento no trae cabecera de hunk.
                if out.hunks.is_empty() {
                    let header = hunk
                        .as_ref()
                        .map(|h| String::from_utf8_lossy(h.header()).trim_end().to_string())
                        .unwrap_or_default();
                    let id = hunk.as_ref().map(hunk_id).unwrap_or_default();
                    out.hunks.push(Hunk { header, id, lines: Vec::new() });
                }
                if let Some(current) = out.hunks.last_mut() {
                    current.lines.push(DiffLine {
                        origin,
                        content: String::from_utf8_lossy(line.content())
                            .trim_end_matches(['\n', '\r'])
                            .to_string(),
                        old_lineno: line.old_lineno(),
                        new_lineno: line.new_lineno(),
                    });
                }
            }
        }
        true
    })?;

    Ok(out)
}

/// Traduce el estado de un delta de libgit2 al nuestro.
fn delta_kind(status: Delta) -> ChangeKind {
    match status {
        Delta::Added | Delta::Copied | Delta::Untracked => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Typechange => ChangeKind::TypeChange,
        Delta::Conflicted => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    }
}

/// Los cuatro rangos de la cabecera de un hunk, que lo identifican dentro de
/// su diff.
fn hunk_id(hunk: &DiffHunk<'_>) -> HunkId {
    HunkId {
        old_start: hunk.old_start(),
        old_lines: hunk.old_lines(),
        new_start: hunk.new_start(),
        new_lines: hunk.new_lines(),
    }
}
