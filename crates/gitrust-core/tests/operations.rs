//! Pruebas de lo que hace la barra de acciones: rama, stash y remoto.
//!
//! El remoto es un repositorio bare en otro directorio temporal. Hablar con él
//! por una ruta de disco recorre el mismo camino que con uno de verdad —los
//! refspecs, el avance rápido, la rama que pasa a seguir a otra— sin depender
//! de que haya red ni credenciales.

mod common;

use common::{temp_dir, TempRepo};
use gitrust_core::Repo;

/// Deja el repositorio con un commit y devuelve el nombre de su rama.
fn primer_commit(temp: &TempRepo) -> String {
    temp.write("a.txt", "uno\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: uno").unwrap();
    repo.head_name()
}

// ---------------------------------------------------------------- ramas

#[test]
fn la_rama_nueva_se_lleva_consigo_lo_que_habia_a_medias() {
    let temp = TempRepo::new("rama");
    primer_commit(&temp);

    temp.write("a.txt", "uno\ndos\n");
    temp.write("b.txt", "nuevo\n");

    let repo = temp.open();
    let antes = repo.head_id().unwrap();
    repo.branch_here("arreglo").unwrap();

    let repo = temp.open();
    assert_eq!(repo.head_name(), "arreglo");
    // La rama nueva apunta al mismo sitio: no se ha escrito historia.
    assert_eq!(repo.head_id().unwrap(), antes);

    // Y lo que estaba a medias sigue a medias, que es el único motivo de
    // crear una rama a mitad de faena.
    let state = repo.status().unwrap();
    let pendientes: Vec<&str> = state.unstaged.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(pendientes, ["a.txt", "b.txt"]);
}

#[test]
fn una_rama_sin_nombre_no_se_crea() {
    let temp = TempRepo::new("rama-sin-nombre");
    primer_commit(&temp);

    let repo = temp.open();
    let err = repo.branch_here("   ").unwrap_err().to_string();
    assert!(err.contains("nombre"), "{err}");
}

#[test]
fn dos_ramas_con_el_mismo_nombre_no_caben() {
    let temp = TempRepo::new("rama-repetida");
    primer_commit(&temp);

    let repo = temp.open();
    repo.branch_here("dos").unwrap();
    let err = temp.open().branch_here("dos").unwrap_err().to_string();
    assert!(err.contains("dos"), "{err}");
}

// ---------------------------------------------------------------- stash

#[test]
fn apartar_deja_el_arbol_limpio_y_sacar_lo_devuelve() {
    let temp = TempRepo::new("stash");
    primer_commit(&temp);
    temp.write("a.txt", "uno\ndos\n");

    let mut repo = temp.open();
    assert!(!repo.status().unwrap().is_clean());

    repo.stash_save("a medias").unwrap();
    assert!(repo.status().unwrap().is_clean());

    let pila = repo.stashes().unwrap();
    assert_eq!(pila.len(), 1);
    assert!(pila[0].message.contains("a medias"));
    assert!(pila[0].time > 0);

    repo.stash_pop(0).unwrap();
    assert!(!repo.status().unwrap().is_clean());
    assert!(repo.stashes().unwrap().is_empty());
}

#[test]
fn aplicar_devuelve_los_cambios_sin_vaciar_la_pila() {
    let temp = TempRepo::new("stash-aplicar");
    primer_commit(&temp);
    temp.write("a.txt", "uno\ndos\n");

    let mut repo = temp.open();
    repo.stash_save("").unwrap();
    repo.stash_apply(0).unwrap();

    assert!(!repo.status().unwrap().is_clean());
    assert_eq!(repo.stashes().unwrap().len(), 1);
}

#[test]
fn tirar_un_apartado_lo_saca_de_la_pila_sin_tocar_el_arbol() {
    let temp = TempRepo::new("stash-tirar");
    primer_commit(&temp);
    temp.write("a.txt", "uno\ndos\n");

    let mut repo = temp.open();
    repo.stash_save("sobra").unwrap();
    repo.stash_drop(0).unwrap();

    assert!(repo.stashes().unwrap().is_empty());
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn apartar_con_el_arbol_limpio_lo_dice_en_vez_de_fallar_raro() {
    let temp = TempRepo::new("stash-limpio");
    primer_commit(&temp);

    let mut repo = temp.open();
    let err = repo.stash_save("nada").unwrap_err().to_string();
    assert_eq!(err, "no hay nada que apartar");
}

// --------------------------------------------------------------- remoto

/// Un repositorio bare al que llevar cosas, montado como «origin».
fn con_remoto(temp: &TempRepo, tag: &str) -> std::path::PathBuf {
    let bare = temp_dir(tag);
    git2::Repository::init_bare(&bare).unwrap();

    let repo = git2::Repository::open(temp.path()).unwrap();
    repo.remote("origin", &bare.display().to_string()).unwrap();
    bare
}

#[test]
fn sin_rama_remota_no_hay_cuentas_que_dar() {
    let temp = TempRepo::new("sin-remoto");
    primer_commit(&temp);
    assert!(temp.open().tracking().is_none());
}

#[test]
fn el_primer_envio_deja_la_rama_siguiendo_al_remoto() {
    let temp = TempRepo::new("push");
    let rama = primer_commit(&temp);
    let bare = con_remoto(&temp, "push-bare");

    let repo = temp.open();
    assert!(repo.tracking().is_none());
    repo.push().unwrap();

    let seguimiento = temp.open().tracking().expect("ya debería seguir a origin");
    assert_eq!(seguimiento.remote, format!("origin/{rama}"));
    assert_eq!((seguimiento.ahead, seguimiento.behind), (0, 0));

    // Y el commit está de verdad al otro lado.
    let remoto = git2::Repository::open(&bare).unwrap();
    assert_eq!(
        remoto.find_reference(&format!("refs/heads/{rama}")).unwrap().target(),
        Some(repo.head_id().unwrap()),
    );

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn traer_adelanta_la_rama_y_las_cuentas_lo_cantan_antes() {
    let origen = TempRepo::new("pull-origen");
    primer_commit(&origen);
    let bare = con_remoto(&origen, "pull-bare");
    origen.open().push().unwrap();

    // El otro lado: una copia que todavía no sabe nada de lo que viene.
    let copia = temp_dir("pull-copia");
    git2::Repository::clone(&bare.display().to_string(), &copia).unwrap();

    origen.write("a.txt", "uno\ndos\n");
    let repo = origen.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: dos").unwrap();
    repo.push().unwrap();
    let esperado = repo.head_id().unwrap();

    let aqui = Repo::open(&copia).unwrap();
    // Antes de traer, las cuentas son las de la última vez que se habló.
    assert_eq!(aqui.tracking().map(|t| t.behind), Some(0));

    let nota = aqui.fetch().unwrap();
    assert!(nota.contains("1 commit"), "{nota}");
    assert_eq!(Repo::open(&copia).unwrap().tracking().map(|t| t.behind), Some(1));

    let aqui = Repo::open(&copia).unwrap();
    aqui.pull().unwrap();
    assert_eq!(Repo::open(&copia).unwrap().head_id(), Some(esperado));

    // Y el fichero ha llegado al árbol de trabajo, no solo a la referencia.
    assert_eq!(std::fs::read_to_string(copia.join("a.txt")).unwrap(), "uno\ndos\n");

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copia);
}

#[test]
fn traer_cuando_no_hay_nada_nuevo_no_miente() {
    let temp = TempRepo::new("pull-aldia");
    let rama = primer_commit(&temp);
    let bare = con_remoto(&temp, "pull-aldia-bare");
    temp.open().push().unwrap();

    let nota = temp.open().pull().unwrap();
    assert_eq!(nota, format!("{rama} ya estaba al día"));

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn sin_remoto_configurado_traer_lo_dice_claro() {
    let temp = TempRepo::new("pull-sin-remoto");
    primer_commit(&temp);

    let err = temp.open().pull().unwrap_err().to_string();
    assert!(err.contains("no sigue a ninguna rama remota"), "{err}");
}
