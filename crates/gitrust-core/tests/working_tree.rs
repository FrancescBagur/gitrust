//! Pruebas del ciclo preparar → commitear sobre un repositorio real y temporal.

mod common;

use std::fs;

use common::TempRepo;
use gitrust_core::{ChangeKind, Repo};

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

/// Veinte líneas numeradas, con las indicadas reescritas.
fn numbered(changed: &[u32]) -> String {
    (1..=20)
        .map(|n| {
            if changed.contains(&n) {
                format!("line {n} changed\n")
            } else {
                format!("line {n}\n")
            }
        })
        .collect()
}

/// Prepara un fichero de veinte líneas y lo commitea como base.
fn repo_with_base(temp: &TempRepo) -> Repo {
    temp.write("a.txt", &numbered(&[]));
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();
    repo
}

fn has_line(diff: &gitrust_core::FileDiff, origin: char, needle: &str) -> bool {
    diff.hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .any(|line| line.origin == origin && line.content.contains(needle))
}

#[test]
fn preparar_un_hunk_deja_el_otro_sin_preparar() {
    let temp = TempRepo::new("stage-hunk");
    let repo = repo_with_base(&temp);

    // Dos cambios lo bastante separados para caer en hunks distintos.
    temp.write("a.txt", &numbered(&[2, 18]));

    let pending = repo.file_diff("a.txt", false).unwrap();
    assert_eq!(pending.hunks.len(), 2, "se esperaban dos hunks separados");

    repo.stage_hunk("a.txt", pending.hunks[0].id).unwrap();

    // El índice recoge solo el primer cambio.
    let staged = repo.file_diff("a.txt", true).unwrap();
    assert_eq!(staged.hunks.len(), 1);
    assert!(has_line(&staged, '+', "line 2 changed"));
    assert!(!has_line(&staged, '+', "line 18 changed"));

    // El segundo sigue pendiente en el árbol de trabajo.
    let left = repo.file_diff("a.txt", false).unwrap();
    assert_eq!(left.hunks.len(), 1);
    assert!(has_line(&left, '+', "line 18 changed"));

    // Y el fichero aparece en las dos listas a la vez.
    let state = repo.status().unwrap();
    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.unstaged.len(), 1);
}

#[test]
fn quitar_un_hunk_lo_devuelve_al_arbol_de_trabajo() {
    let temp = TempRepo::new("unstage-hunk");
    let repo = repo_with_base(&temp);

    temp.write("a.txt", &numbered(&[2, 18]));
    repo.stage("a.txt").unwrap();

    let staged = repo.file_diff("a.txt", true).unwrap();
    assert_eq!(staged.hunks.len(), 2);
    assert!(repo.status().unwrap().unstaged.is_empty());

    repo.unstage_hunk("a.txt", staged.hunks[1].id).unwrap();

    // En el índice queda el primero; el segundo vuelve a estar pendiente.
    let left_staged = repo.file_diff("a.txt", true).unwrap();
    assert_eq!(left_staged.hunks.len(), 1);
    assert!(has_line(&left_staged, '+', "line 2 changed"));

    let pending = repo.file_diff("a.txt", false).unwrap();
    assert_eq!(pending.hunks.len(), 1);
    assert!(has_line(&pending, '+', "line 18 changed"));

    // El fichero del disco no se toca: sigue con los dos cambios.
    let contents = fs::read_to_string(temp.path().join("a.txt")).unwrap();
    assert!(contents.contains("line 2 changed"));
    assert!(contents.contains("line 18 changed"));
}

#[test]
fn un_hunk_que_ya_no_existe_da_error_en_vez_de_pasar_en_silencio() {
    let temp = TempRepo::new("stale-hunk");
    let repo = repo_with_base(&temp);

    temp.write("a.txt", &numbered(&[2]));
    let diff = repo.file_diff("a.txt", false).unwrap();
    let hunk = diff.hunks[0].id;

    repo.stage_hunk("a.txt", hunk).unwrap();
    // Ya está en el índice: el segundo intento no encuentra ese hunk.
    assert!(repo.stage_hunk("a.txt", hunk).is_err());
}

#[test]
fn el_detalle_separa_asunto_y_cuerpo_y_lista_los_ficheros() {
    let temp = TempRepo::new("detail");
    temp.write("a.txt", "uno\n");
    temp.write("b.txt", "dos\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: dos ficheros\n\nY una explicación\nen dos líneas.")
        .unwrap();

    let head = repo.commits(1).unwrap()[0].id;
    let detail = repo.commit_detail(head).unwrap();

    assert_eq!(detail.summary, "feat: dos ficheros");
    assert_eq!(detail.body, "Y una explicación\nen dos líneas.");
    assert!(!detail.is_merge());

    let paths: Vec<&str> = detail.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["a.txt", "b.txt"]);
    assert!(detail.files.iter().all(|f| f.kind == ChangeKind::Added));
    assert_eq!(detail.counts(), (2, 0, 0));

    // Y el diff de uno de ellos sale del propio commit, no del árbol.
    let diff = repo.commit_file_diff(head, "a.txt").unwrap();
    assert!(has_line(&diff, '+', "uno"));
}

#[test]
fn las_referencias_marcan_la_rama_de_head() {
    let temp = TempRepo::new("refs");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    let refs = repo.references().unwrap();
    assert_eq!(refs.len(), 1, "solo la rama inicial");

    let branch = &refs[0];
    assert!(branch.is_head);
    assert_eq!(branch.kind, gitrust_core::RefKind::LocalBranch);
    // Sin barras en el nombre, no hay carpeta y la hoja es el nombre entero.
    assert_eq!(branch.folder(), None);
    assert_eq!(branch.leaf(), branch.name);
}

#[test]
fn una_rama_con_barras_se_agrupa_por_su_carpeta() {
    let temp = TempRepo::new("folders");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("base").unwrap();

    // Dos ramas con prefijo, como las que usa cualquier repositorio real.
    let git = git2::Repository::open(temp.path()).unwrap();
    let head = git.head().unwrap().peel_to_commit().unwrap();
    git.branch("feat/login", &head, false).unwrap();
    git.branch("feat/signup", &head, false).unwrap();

    let refs = repo.references().unwrap();
    let feat: Vec<_> = refs.iter().filter(|r| r.folder() == Some("feat")).collect();

    assert_eq!(feat.len(), 2);
    let leaves: Vec<&str> = feat.iter().map(|r| r.leaf()).collect();
    assert_eq!(leaves, vec!["login", "signup"]);
}

#[test]
fn un_commit_sin_mensaje_se_rechaza() {
    let temp = TempRepo::new("empty-msg");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();

    assert!(repo.commit("   ").is_err());
}

// --------------------------------------------------------------- enmendar

#[test]
fn enmendar_cambia_el_mensaje_sin_escribir_historia() {
    let temp = TempRepo::new("enmendar-mensaje");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    let viejo = repo.commit("feat: asnuto mal escrito").unwrap();

    let nuevo = temp.open().amend("feat: asunto bien escrito").unwrap();
    assert_ne!(nuevo, viejo, "enmendar escribe otro commit, no edita el de antes");

    let repo = temp.open();
    let detail = repo.commit_detail(nuevo).unwrap();
    assert_eq!(detail.summary, "feat: asunto bien escrito");
    assert!(detail.parents.is_empty(), "el padre sigue siendo el mismo: ninguno");
    assert_eq!(repo.commits(50).unwrap().len(), 1, "no hay un commit de más");
}

#[test]
fn enmendar_mete_lo_preparado_en_el_ultimo_commit() {
    let temp = TempRepo::new("enmendar-ficheros");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    // El fichero que se olvidó, que es para lo que se enmienda.
    temp.write("b.txt", "dos\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    let nuevo = repo.amend("feat: a y b").unwrap();

    let repo = temp.open();
    let files: Vec<String> = repo
        .commit_detail(nuevo)
        .unwrap()
        .files
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert_eq!(files, ["a.txt", "b.txt"]);
    assert_eq!(repo.commits(50).unwrap().len(), 1);
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn enmendar_conserva_al_autor_y_cambia_a_quien_lo_cierra() {
    let temp = TempRepo::new("enmendar-autoria");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    let viejo = repo.commit("feat: a").unwrap();

    let cuando = {
        let raw = git2::Repository::open(temp.path()).unwrap();
        let seconds = raw.find_commit(viejo).unwrap().author().when().seconds();
        seconds
    };

    // Otra persona enmienda el commit de la primera.
    {
        let raw = git2::Repository::open(temp.path()).unwrap();
        let mut config = raw.config().unwrap();
        config.set_str("user.name", "Otra").unwrap();
        config.set_str("user.email", "otra@example.invalid").unwrap();
    }

    let nuevo = temp.open().amend("feat: a, mejor contado").unwrap();

    let raw = git2::Repository::open(temp.path()).unwrap();
    let commit = raw.find_commit(nuevo).unwrap();
    assert_eq!(commit.author().name(), Some("gitrust test"));
    assert_eq!(commit.author().when().seconds(), cuando);
    assert_eq!(commit.committer().name(), Some("Otra"));
}

#[test]
fn sin_commits_no_hay_nada_que_enmendar() {
    let temp = TempRepo::new("enmendar-vacio");
    let err = temp.open().amend("feat: nada").unwrap_err().to_string();
    assert!(err.contains("enmendar"), "{err}");
}

#[test]
fn enmendar_sin_mensaje_se_rechaza() {
    let temp = TempRepo::new("enmendar-sin-mensaje");
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a").unwrap();

    assert!(temp.open().amend("   ").is_err());
}

#[test]
fn el_mensaje_de_head_vuelve_partido_en_asunto_y_cuerpo() {
    let temp = TempRepo::new("mensaje-head");
    assert!(temp.open().head_message().is_none(), "sin commits no hay mensaje");

    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: el asunto\n\nY el cuerpo,\nen dos líneas.\n").unwrap();

    let (subject, body) = temp.open().head_message().unwrap();
    assert_eq!(subject, "feat: el asunto");
    assert_eq!(body, "Y el cuerpo,\nen dos líneas.");
}
