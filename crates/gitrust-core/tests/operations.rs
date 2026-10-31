//! Pruebas de lo que hace la barra de acciones: rama, stash y remoto.
//!
//! El remoto es un repositorio bare en otro directorio temporal. Hablar con él
//! por una ruta de disco recorre el mismo camino que con uno de verdad —los
//! refspecs, el avance rápido, la rama que pasa a seguir a otra— sin depender
//! de que haya red ni credenciales.

mod common;

use common::{temp_dir, TempRepo};
use gitrust_core::{layout, Lane, RefKind, Repo};

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

#[test]
fn cambiar_de_rama_trae_los_ficheros_de_la_otra() {
    let temp = TempRepo::new("cambiar");
    let vieja = primer_commit(&temp);

    temp.open().branch_here("otra").unwrap();
    temp.write("b.txt", "solo en otra\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();
    assert!(temp.path().join("b.txt").exists());

    let nota = temp.open().checkout_branch(&vieja).unwrap();
    assert_eq!(nota, format!("ahora estás en {vieja}"));

    let repo = temp.open();
    assert_eq!(repo.head_name(), vieja);
    // El fichero que solo existía en la otra rama se ha ido del disco, y el
    // árbol queda limpio: el índice también se enteró del cambio.
    assert!(!temp.path().join("b.txt").exists());
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn lo_cambiado_que_no_estorba_viaja_contigo() {
    let temp = TempRepo::new("cambiar-con-faena");
    let vieja = primer_commit(&temp);

    temp.open().branch_here("otra").unwrap();
    temp.write("b.txt", "solo en otra\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    // a.txt es igual en las dos ramas, así que lo que uno le haya hecho no
    // choca con nada y tiene que sobrevivir al cambio.
    temp.write("a.txt", "uno\na medias\n");
    temp.open().checkout_branch(&vieja).unwrap();

    let repo = temp.open();
    assert_eq!(repo.head_name(), vieja);
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt")).unwrap(),
        "uno\na medias\n"
    );
    let state = repo.status().unwrap();
    assert_eq!(state.unstaged.len(), 1);
    assert_eq!(state.unstaged[0].path, "a.txt");
}

#[test]
fn con_cambios_que_chocan_no_se_cambia_ni_a_medias() {
    let temp = TempRepo::new("cambiar-choque");
    let vieja = primer_commit(&temp);

    temp.open().branch_here("otra").unwrap();
    temp.write("a.txt", "lo que dice otra\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    repo.commit("feat: a distinto").unwrap();

    // Ahora a.txt difiere entre las dos ramas y además está a medias aquí.
    temp.write("a.txt", "lo que estoy escribiendo\n");

    let err = temp.open().checkout_branch(&vieja).unwrap_err().to_string();
    assert!(err.contains("chocan") && err.contains("Stash"), "{err}");

    // Y nada se ha movido: ni HEAD ni el fichero.
    let repo = temp.open();
    assert_eq!(repo.head_name(), "otra");
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt")).unwrap(),
        "lo que estoy escribiendo\n"
    );
}

#[test]
fn cambiar_a_una_rama_que_no_existe_lo_dice() {
    let temp = TempRepo::new("cambiar-fantasma");
    primer_commit(&temp);

    let err = temp.open().checkout_branch("fantasma").unwrap_err().to_string();
    assert!(err.contains("fantasma"), "{err}");
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

#[test]
fn cambiar_a_una_remota_crea_la_local_y_la_deja_siguiendo() {
    let temp = TempRepo::new("remota");
    let vieja = primer_commit(&temp);
    let bare = con_remoto(&temp, "remota-bare");
    temp.open().push().unwrap();

    // Una rama que solo vive en el remoto para quien venga después.
    temp.open().branch_here("entrega").unwrap();
    temp.write("c.txt", "de la entrega\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();
    repo.push().unwrap();

    let copia = temp_dir("remota-copia");
    git2::Repository::clone(&bare.display().to_string(), &copia).unwrap();
    assert_eq!(Repo::open(&copia).unwrap().head_name(), vieja);

    let nota = Repo::open(&copia).unwrap().checkout_remote("origin/entrega").unwrap();
    assert_eq!(nota, "ahora estás en entrega");

    let repo = Repo::open(&copia).unwrap();
    assert_eq!(repo.head_name(), "entrega");
    assert_eq!(repo.tracking().map(|t| t.remote), Some("origin/entrega".to_string()));
    assert!(copia.join("c.txt").exists());

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&copia);
}

#[test]
fn si_la_local_ya_existe_se_va_a_ella_sin_tocarla() {
    let temp = TempRepo::new("remota-existente");
    let vieja = primer_commit(&temp);
    let bare = con_remoto(&temp, "remota-existente-bare");
    temp.open().push().unwrap();

    temp.open().branch_here("entrega").unwrap();
    temp.write("c.txt", "de la entrega\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    let suyo = repo.commit("feat: c").unwrap();
    repo.push().unwrap();
    temp.open().checkout_branch(&vieja).unwrap();

    // La local existe y apunta a donde apunta: ir a la remota es ir a ella.
    temp.open().checkout_remote("origin/entrega").unwrap();
    let repo = temp.open();
    assert_eq!(repo.head_name(), "entrega");
    assert_eq!(repo.head_id(), Some(suyo));

    let _ = std::fs::remove_dir_all(&bare);
}

// ------------------------------------------------------------- etiquetas

#[test]
fn una_etiqueta_sin_mensaje_sale_ligera_y_con_mensaje_anotada() {
    let temp = TempRepo::new("etiquetas");
    primer_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();

    repo.tag("v1.0", commit, "").unwrap();
    repo.tag("v2.0", commit, "La primera entrega de verdad").unwrap();

    let raw = git2::Repository::open(temp.path()).unwrap();

    // La ligera es una referencia a secas: apunta al commit.
    let ligera = raw.find_reference("refs/tags/v1.0").unwrap();
    assert_eq!(ligera.target(), Some(commit));
    assert!(raw.find_tag(ligera.target().unwrap()).is_err());

    // La anotada es un objeto propio, con su mensaje y quien la puso.
    let anotada = raw.find_reference("refs/tags/v2.0").unwrap();
    let objeto = raw.find_tag(anotada.target().unwrap()).unwrap();
    assert_eq!(objeto.message(), Some("La primera entrega de verdad"));
    assert_eq!(objeto.tagger().unwrap().name(), Some("gitrust test"));
    assert_eq!(objeto.target_id(), commit);
}

#[test]
fn las_dos_clases_salen_en_las_referencias_y_sobre_su_commit() {
    let temp = TempRepo::new("etiquetas-refs");
    primer_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();
    repo.tag("v1.0", commit, "").unwrap();
    repo.tag("v2.0", commit, "anotada").unwrap();

    let repo = temp.open();
    let refs = repo.references().unwrap();
    let etiquetas: Vec<&str> = refs
        .iter()
        .filter(|entry| entry.kind == RefKind::Tag)
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(etiquetas, ["v1.0", "v2.0"]);

    // Y en el grafo cuelgan del commit, anotada incluida: pelarla es lo que
    // hace que no se quede flotando en su propio objeto.
    let graph = layout(repo.commits(10).unwrap());
    let nombres: Vec<&str> = graph.rows[0]
        .commit
        .refs
        .iter()
        .filter(|label| label.kind == RefKind::Tag)
        .map(|label| label.name.as_str())
        .collect();
    assert_eq!(nombres, ["v1.0", "v2.0"]);
}

#[test]
fn una_etiqueta_no_pisa_a_otra_con_su_nombre() {
    let temp = TempRepo::new("etiquetas-repetida");
    primer_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();

    repo.tag("v1.0", commit, "").unwrap();
    let err = repo.tag("v1.0", commit, "").unwrap_err().to_string();
    assert!(err.contains("v1.0"), "{err}");
}

#[test]
fn borrar_una_etiqueta_no_toca_al_commit() {
    let temp = TempRepo::new("etiquetas-borrar");
    primer_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();
    repo.tag("v1.0", commit, "con mensaje").unwrap();

    repo.delete_tag("v1.0").unwrap();

    let repo = temp.open();
    assert!(!repo.references().unwrap().iter().any(|e| e.kind == RefKind::Tag));
    assert_eq!(repo.head_id(), Some(commit));
}

#[test]
fn una_etiqueta_sin_nombre_no_se_crea() {
    let temp = TempRepo::new("etiquetas-sin-nombre");
    primer_commit(&temp);
    let repo = temp.open();
    let commit = repo.head_id().unwrap();

    let err = repo.tag("  ", commit, "").unwrap_err().to_string();
    assert!(err.contains("nombre"), "{err}");
}

// -------------------------------------------------------- borrar ramas

#[test]
fn borrar_una_rama_no_toca_a_los_commits_que_comparte() {
    let temp = TempRepo::new("borrar-rama");
    let vieja = primer_commit(&temp);
    let antes = temp.open().head_id().unwrap();

    temp.open().branch_here("sobra").unwrap();
    temp.open().checkout_branch(&vieja).unwrap();

    // No tiene nada suyo: nació donde estaba todo lo demás.
    assert_eq!(temp.open().unmerged("sobra").unwrap(), 0);
    temp.open().delete_branch("sobra").unwrap();

    let repo = temp.open();
    assert!(!repo.references().unwrap().iter().any(|e| e.name == "sobra"));
    assert_eq!(repo.head_id(), Some(antes));
}

#[test]
fn la_cuenta_dice_lo_que_se_perderia() {
    let temp = TempRepo::new("borrar-cuenta");
    let vieja = primer_commit(&temp);

    temp.open().branch_here("suya").unwrap();
    for n in 1..=2 {
        temp.write("b.txt", &format!("version {n}\n"));
        let repo = temp.open();
        repo.stage("b.txt").unwrap();
        repo.commit(&format!("feat: b{n}")).unwrap();
    }
    temp.open().checkout_branch(&vieja).unwrap();

    // Dos commits que no están en ningún otro sitio.
    assert_eq!(temp.open().unmerged("suya").unwrap(), 2);

    // Y en cuanto una etiqueta los alcanza, dejan de estar solos.
    let repo = temp.open();
    let punta = repo
        .references()
        .unwrap()
        .iter()
        .find(|e| e.name == "suya")
        .unwrap()
        .target;
    repo.tag("guardada", punta, "").unwrap();
    assert_eq!(temp.open().unmerged("suya").unwrap(), 0);
}

#[test]
fn la_rama_en_la_que_estas_no_se_borra() {
    let temp = TempRepo::new("borrar-actual");
    let actual = primer_commit(&temp);

    let err = temp.open().delete_branch(&actual).unwrap_err().to_string();
    assert!(err.contains("en la que estás"), "{err}");
}

#[test]
fn borrar_una_rama_que_no_existe_lo_dice() {
    let temp = TempRepo::new("borrar-fantasma");
    primer_commit(&temp);
    let err = temp.open().delete_branch("fantasma").unwrap_err().to_string();
    assert!(err.contains("fantasma"), "{err}");
}

#[test]
fn borrar_en_el_remoto_la_quita_de_alli_y_de_aqui() {
    let temp = TempRepo::new("borrar-remota");
    let vieja = primer_commit(&temp);
    let bare = con_remoto(&temp, "borrar-remota-bare");
    temp.open().push().unwrap();

    temp.open().branch_here("entrega").unwrap();
    temp.write("c.txt", "de la entrega\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();
    repo.push().unwrap();
    temp.open().checkout_branch(&vieja).unwrap();

    let remoto = git2::Repository::open(&bare).unwrap();
    assert!(remoto.find_reference("refs/heads/entrega").is_ok());

    temp.open().delete_remote_branch("origin/entrega").unwrap();

    // Ya no está allí, ni queda el rastro de aquí.
    let remoto = git2::Repository::open(&bare).unwrap();
    assert!(remoto.find_reference("refs/heads/entrega").is_err());

    let repo = temp.open();
    let refs = repo.references().unwrap();
    assert!(!refs.iter().any(|e| e.name == "origin/entrega"));
    // Y la local sigue donde estaba: borrar allí no borra aquí.
    assert!(refs.iter().any(|e| e.name == "entrega"));

    let _ = std::fs::remove_dir_all(&bare);
}

// ---------------------------------------------------------------- fusión

/// Deja `rama` con un commit propio que toca `file`, y vuelve a donde estaba.
fn rama_con_trabajo(temp: &TempRepo, rama: &str, file: &str, texto: &str) -> String {
    let vuelta = temp.open().head_name();
    temp.open().branch_here(rama).unwrap();
    temp.write(file, texto);
    let repo = temp.open();
    repo.stage(file).unwrap();
    repo.commit(&format!("feat: {file} en {rama}")).unwrap();
    temp.open().checkout_branch(&vuelta).unwrap();
    vuelta
}

#[test]
fn sin_haberse_movido_la_rama_solo_avanza() {
    let temp = TempRepo::new("fusion-rapida");
    primer_commit(&temp);
    rama_con_trabajo(&temp, "suya", "b.txt", "de la otra\n");

    let antes = temp.open().commits(50).unwrap().len();
    let nota = temp.open().merge("suya").unwrap();
    assert!(nota.contains("sin fusión"), "{nota}");

    let repo = temp.open();
    // No hay commit de fusión: la historia es la misma, solo más larga.
    assert_eq!(repo.commits(50).unwrap().len(), antes);
    assert!(temp.path().join("b.txt").exists());
    assert_eq!(repo.commit_detail(repo.head_id().unwrap()).unwrap().parents.len(), 1);
}

#[test]
fn dos_historias_distintas_se_juntan_en_un_commit_de_dos_padres() {
    let temp = TempRepo::new("fusion-real");
    primer_commit(&temp);
    rama_con_trabajo(&temp, "suya", "b.txt", "de la otra\n");

    // Y aquí, mientras tanto, otra cosa en otro fichero.
    temp.write("c.txt", "de la mia\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    let nota = temp.open().merge("suya").unwrap();
    assert!(nota.contains("fusionada"), "{nota}");

    let repo = temp.open();
    let head = repo.commit_detail(repo.head_id().unwrap()).unwrap();
    assert_eq!(head.parents.len(), 2);
    assert!(head.summary.starts_with("Merge suya"));

    // Los dos ficheros están en el disco, y el árbol queda limpio.
    assert!(temp.path().join("b.txt").exists());
    assert!(temp.path().join("c.txt").exists());
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn un_choque_deja_el_repositorio_exactamente_como_estaba() {
    let temp = TempRepo::new("fusion-choque");
    primer_commit(&temp);
    rama_con_trabajo(&temp, "suya", "a.txt", "lo que dice la otra\n");

    temp.write("a.txt", "lo que digo yo\n");
    let repo = temp.open();
    repo.stage("a.txt").unwrap();
    let mio = repo.commit("feat: a a mi manera").unwrap();

    let err = temp.open().merge("suya").unwrap_err().to_string();
    assert!(err.contains("a.txt"), "dice en qué choca: {err}");
    assert!(err.contains("no se ha tocado nada"), "{err}");

    // Ni commit de fusión, ni árbol revuelto, ni índice con conflictos.
    let repo = temp.open();
    assert_eq!(repo.head_id(), Some(mio));
    assert!(repo.status().unwrap().is_clean());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("a.txt")).unwrap(),
        "lo que digo yo\n"
    );
}

#[test]
fn fusionar_con_el_arbol_a_medias_se_para_antes_de_empezar() {
    let temp = TempRepo::new("fusion-sucia");
    primer_commit(&temp);
    rama_con_trabajo(&temp, "suya", "b.txt", "de la otra\n");

    temp.write("c.txt", "de la mia\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    // Faena sin guardar encima de una fusión que sí hace falta escribir.
    temp.write("a.txt", "uno\na medias\n");

    let err = temp.open().merge("suya").unwrap_err().to_string();
    assert!(err.contains("aparta"), "{err}");
    assert!(!temp.open().status().unwrap().is_clean(), "sigue a medias");
}

#[test]
fn fusionar_lo_que_ya_esta_dentro_no_escribe_nada() {
    let temp = TempRepo::new("fusion-aldia");
    let vieja = primer_commit(&temp);
    temp.open().branch_here("suya").unwrap();
    temp.open().checkout_branch(&vieja).unwrap();

    let nota = temp.open().merge("suya").unwrap();
    assert!(nota.contains("ya está dentro"), "{nota}");
    assert_eq!(temp.open().commits(50).unwrap().len(), 1);
}

#[test]
fn una_rama_no_se_fusiona_consigo_misma() {
    let temp = TempRepo::new("fusion-misma");
    let actual = primer_commit(&temp);
    let err = temp.open().merge(&actual).unwrap_err().to_string();
    assert!(err.contains("en la que estás"), "{err}");
}

// -------------------------------------------------- forzar y etiquetas

#[test]
fn forzar_lleva_la_historia_rehecha_al_remoto() {
    let temp = TempRepo::new("forzar");
    let rama = primer_commit(&temp);
    let bare = con_remoto(&temp, "forzar-bare");
    temp.open().push().unwrap();

    // Se rehace lo que ya había salido: el remoto y esto dejan de cuadrar.
    let rehecho = temp.open().amend("feat: uno, mejor contado").unwrap();
    assert!(temp.open().push().is_err(), "sin forzar no debería poder");

    let nota = temp.open().push_force().unwrap();
    assert!(nota.contains("forzada"), "{nota}");

    let remoto = git2::Repository::open(&bare).unwrap();
    assert_eq!(
        remoto.find_reference(&format!("refs/heads/{rama}")).unwrap().target(),
        Some(rehecho),
    );

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn forzar_se_niega_si_el_remoto_ha_cambiado_desde_la_ultima_visita() {
    let temp = TempRepo::new("forzar-ajeno");
    primer_commit(&temp);
    let bare = con_remoto(&temp, "forzar-ajeno-bare");
    temp.open().push().unwrap();

    // Otra persona escribe en el remoto sin que aquí nos enteremos.
    let otro = temp_dir("forzar-ajeno-otro");
    git2::Repository::clone(&bare.display().to_string(), &otro).unwrap();
    {
        let raw = git2::Repository::open(&otro).unwrap();
        let mut config = raw.config().unwrap();
        config.set_str("user.name", "Otra").unwrap();
        config.set_str("user.email", "otra@example.invalid").unwrap();
    }
    std::fs::write(otro.join("suyo.txt"), "lo mio\n").unwrap();
    let repo = Repo::open(&otro).unwrap();
    repo.stage("suyo.txt").unwrap();
    repo.commit("feat: lo suyo").unwrap();
    repo.push().unwrap();

    temp.open().amend("feat: uno, a mi manera").unwrap();
    let err = temp.open().push_force().unwrap_err().to_string();
    assert!(err.contains("ha cambiado"), "{err}");

    let _ = std::fs::remove_dir_all(&bare);
    let _ = std::fs::remove_dir_all(&otro);
}

#[test]
fn las_etiquetas_salen_una_vez_y_no_se_repiten() {
    let temp = TempRepo::new("etiquetas-push");
    primer_commit(&temp);
    let bare = con_remoto(&temp, "etiquetas-push-bare");
    temp.open().push().unwrap();

    let repo = temp.open();
    let commit = repo.head_id().unwrap();
    repo.tag("v1.0", commit, "").unwrap();
    repo.tag("v1.1", commit, "anotada").unwrap();

    let nota = temp.open().push_tags().unwrap();
    assert!(nota.contains("2 etiquetas enviadas"), "{nota}");

    let remoto = git2::Repository::open(&bare).unwrap();
    assert!(remoto.find_reference("refs/tags/v1.0").is_ok());
    assert!(remoto.find_reference("refs/tags/v1.1").is_ok());

    // La segunda vez no hay nada que mandar, y lo dice.
    let nota = temp.open().push_tags().unwrap();
    assert!(nota.contains("ya tiene todas"), "{nota}");

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn sin_etiquetas_no_se_inventa_un_envio() {
    let temp = TempRepo::new("etiquetas-push-vacio");
    primer_commit(&temp);
    let bare = con_remoto(&temp, "etiquetas-push-vacio-bare");
    temp.open().push().unwrap();

    let nota = temp.open().push_tags().unwrap();
    assert!(nota.contains("ya tiene todas"), "{nota}");

    let _ = std::fs::remove_dir_all(&bare);
}

// --------------------------------------------------------------- tablero

/// La tarjeta de esa rama, o el pánico de la prueba.
fn carta<'a>(board: &'a gitrust_core::Board, branch: &str) -> &'a gitrust_core::Card {
    board
        .cards
        .iter()
        .find(|card| card.branch == branch)
        .unwrap_or_else(|| panic!("no hay tarjeta de {branch} en {:?}", board.cards))
}

#[test]
fn una_rama_recien_hecha_esta_por_hacer_y_con_commits_en_marcha() {
    let temp = TempRepo::new("tablero");
    let principal = primer_commit(&temp);

    temp.open().branch_here("sin-empezar").unwrap();
    temp.open().checkout_branch(&principal).unwrap();

    let board = temp.open().board().unwrap();
    assert_eq!(board.trunk, principal);
    assert_eq!(carta(&board, "sin-empezar").lane, Lane::Todo);
    assert_eq!(carta(&board, "sin-empezar").ahead, 0);

    // En cuanto tiene algo suyo, cambia de columna sola.
    temp.open().checkout_branch("sin-empezar").unwrap();
    temp.write("b.txt", "ya hay algo\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    let board = temp.open().board().unwrap();
    let card = carta(&board, "sin-empezar");
    assert_eq!(card.lane, Lane::InProgress);
    assert_eq!(card.ahead, 1);
    assert!(card.current, "es la que está puesta");
    assert!(card.time > 0);
}

#[test]
fn empujada_y_sin_fusionar_pasa_a_en_revision() {
    let temp = TempRepo::new("tablero-revision");
    let principal = primer_commit(&temp);
    let bare = con_remoto(&temp, "tablero-revision-bare");
    temp.open().push().unwrap();

    temp.open().branch_here("propuesta").unwrap();
    temp.write("b.txt", "para que lo miren\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    // Antes de salir de aquí, está en marcha.
    assert_eq!(carta(&temp.open().board().unwrap(), "propuesta").lane, Lane::InProgress);

    temp.open().push().unwrap();
    temp.open().checkout_branch(&principal).unwrap();

    assert_eq!(carta(&temp.open().board().unwrap(), "propuesta").lane, Lane::InReview);

    let _ = std::fs::remove_dir_all(&bare);
}

#[test]
fn fusionada_en_la_principal_queda_hecha() {
    let temp = TempRepo::new("tablero-hecho");
    let principal = primer_commit(&temp);

    temp.open().branch_here("terminada").unwrap();
    temp.write("b.txt", "lo que hacía falta\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();
    temp.open().checkout_branch(&principal).unwrap();

    // La principal avanza por su cuenta, para que la fusión sea de verdad.
    temp.write("c.txt", "mientras tanto\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    temp.open().merge("terminada").unwrap();

    let board = temp.open().board().unwrap();
    let card = carta(&board, "terminada");
    assert_eq!(card.lane, Lane::Done);
    assert_eq!(card.ahead, 0);
    assert!(card.behind > 0);
}

#[test]
fn la_principal_no_es_una_tarjeta() {
    let temp = TempRepo::new("tablero-principal");
    let principal = primer_commit(&temp);

    let board = temp.open().board().unwrap();
    assert_eq!(board.trunk, principal);
    assert!(!board.cards.iter().any(|card| card.branch == principal));
}

#[test]
fn el_proyecto_sale_de_la_carpeta_del_nombre() {
    let temp = TempRepo::new("tablero-proyecto");
    let principal = primer_commit(&temp);

    for rama in ["tema/colores", "tema/tipografia", "suelta"] {
        temp.open().branch_here(rama).unwrap();
        temp.open().checkout_branch(&principal).unwrap();
    }

    let board = temp.open().board().unwrap();
    assert_eq!(carta(&board, "tema/colores").project.as_deref(), Some("tema"));
    assert_eq!(carta(&board, "tema/colores").leaf(), "colores");
    assert_eq!(carta(&board, "suelta").project, None);
    assert_eq!(board.projects(), ["tema"]);
}

#[test]
fn las_columnas_reparten_todas_las_tarjetas_y_ninguna_dos_veces() {
    let temp = TempRepo::new("tablero-columnas");
    let principal = primer_commit(&temp);

    temp.open().branch_here("a").unwrap();
    temp.open().checkout_branch(&principal).unwrap();
    temp.open().branch_here("b").unwrap();
    temp.write("b.txt", "algo\n");
    let repo = temp.open();
    repo.stage("b.txt").unwrap();
    repo.commit("feat: b").unwrap();

    let board = temp.open().board().unwrap();
    let repartidas: usize = Lane::ALL.iter().map(|lane| board.lane(*lane).count()).sum();
    assert_eq!(repartidas, board.cards.len());
    assert_eq!(board.lane(Lane::Todo).count(), 1);
    assert_eq!(board.lane(Lane::InProgress).count(), 1);
}

#[test]
fn una_rama_vacia_que_se_queda_atras_sigue_por_hacer() {
    let temp = TempRepo::new("tablero-olvidada");
    let principal = primer_commit(&temp);

    temp.open().branch_here("algun-dia").unwrap();
    temp.open().checkout_branch(&principal).unwrap();

    // La principal avanza sin ella. Nunca hizo nada, así que no ha terminado
    // nada: quedarse atrás no es haber acabado.
    temp.write("c.txt", "mientras tanto\n");
    let repo = temp.open();
    repo.stage("c.txt").unwrap();
    repo.commit("feat: c").unwrap();

    let board = temp.open().board().unwrap();
    let card = carta(&board, "algun-dia");
    assert_eq!(card.lane, Lane::Todo);
    assert_eq!(card.ahead, 0);
    assert!(card.behind > 0, "se ha quedado atrás, pero no hecha");
}
