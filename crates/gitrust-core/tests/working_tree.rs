//! Pruebas del ciclo preparar → commitear sobre un repositorio real y temporal.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gitrust_core::{ChangeKind, Repo};

/// Crea un repositorio vacío en un directorio temporal propio de esta prueba.
struct TempRepo {
    dir: PathBuf,
}

impl TempRepo {
    fn new(tag: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("gitrust-{tag}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();

        let repo = git2::Repository::init(&dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "gitrust test").unwrap();
        config.set_str("user.email", "test@example.invalid").unwrap();

        Self { dir }
    }

    fn write(&self, name: &str, contents: &str) {
        fs::write(self.dir.join(name), contents).unwrap();
    }

    fn open(&self) -> Repo {
        Repo::open(&self.dir).unwrap()
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn un_fichero_nuevo_pasa_de_sin_seguimiento_a_preparado() {
    let temp = TempRepo::new("stage");
    temp.write("a.txt", "hola\n");
    let repo = temp.open();

    let state = repo.status().unwrap();
    assert!(state.staged.is_empty());
    assert_eq!(state.unstaged.len(), 1);
    assert_eq!(state.unstaged[0].path, "a.txt");
    assert_eq!(state.unstaged[0].kind, ChangeKind::Untracked);

    repo.stage("a.txt").unwrap();

    let state = repo.status().unwrap();
    assert!(state.unstaged.is_empty());
    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.staged[0].kind, ChangeKind::Added);
}

#[test]
fn el_diff_del_indice_muestra_las_lineas_anadidas() {
    let temp = TempRepo::new("diff");
    temp.write("a.txt", "uno\ndos\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();

    let diff = repo.file_diff("a.txt", true).unwrap();
    assert!(!diff.binary);

    let (added, removed) = diff.counts();
    assert_eq!((added, removed), (2, 0));

    let lines: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter(|line| line.origin == '+')
        .map(|line| line.content.as_str())
        .collect();
    assert_eq!(lines, vec!["uno", "dos"]);
}

#[test]
fn commitear_deja_el_arbol_limpio_y_crea_historia() {
    let temp = TempRepo::new("commit");
    temp.write("a.txt", "hola\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("primer commit").unwrap();

    assert!(repo.status().unwrap().is_clean());

    let commits = repo.commits(10).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].summary, "primer commit");
    assert!(commits[0].parents.is_empty());
    // HEAD y la rama apuntan al commit recién creado.
    assert!(commits[0].refs.len() >= 2);
}

#[test]
fn quitar_del_indice_devuelve_el_cambio_al_arbol_de_trabajo() {
    let temp = TempRepo::new("unstage");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    temp.write("a.txt", "uno\ndos\n");
    let state = repo.status().unwrap();
    assert_eq!(state.unstaged[0].kind, ChangeKind::Modified);

    repo.stage("a.txt").unwrap();
    let state = repo.status().unwrap();
    assert_eq!(state.staged[0].kind, ChangeKind::Modified);
    assert!(state.unstaged.is_empty());

    repo.unstage("a.txt").unwrap();
    let state = repo.status().unwrap();
    assert!(state.staged.is_empty(), "el índice debe quedar vacío");
    assert_eq!(state.unstaged[0].kind, ChangeKind::Modified);
}

#[test]
fn borrar_un_fichero_se_prepara_como_borrado() {
    let temp = TempRepo::new("delete");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    fs::remove_file(temp.path().join("a.txt")).unwrap();
    repo.stage("a.txt").unwrap();

    let state = repo.status().unwrap();
    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.staged[0].kind, ChangeKind::Deleted);
}

#[test]
fn un_commit_sin_mensaje_se_rechaza() {
    let temp = TempRepo::new("empty-msg");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();

    assert!(repo.commit("   ").is_err());
}
