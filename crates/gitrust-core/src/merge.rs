//! Juntar una rama con la actual.
//!
//! Lo que no hay aquí es tan importante como lo que hay: si la fusión sale con
//! conflictos, no se toca nada. Un repositorio a medio fusionar es un sitio
//! del que hay que saber salir, y mientras la aplicación no tenga editor de
//! conflictos no puede dejar a nadie ahí dentro.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, ObjectType};

use crate::repo::Repo;

impl Repo {
    /// Fusiona una rama en la actual. Vale tanto una local como una remota.
    ///
    /// Si la actual no se ha movido desde que salió la otra, no hay dos
    /// historias que juntar sino una que seguir, y la rama avanza sin escribir
    /// ningún commit.
    ///
    /// Si hay que juntarlas de verdad, la fusión se calcula primero en memoria.
    /// Solo cuando sale limpia se escribe el commit y se actualiza el árbol, de
    /// modo que un choque deja el repositorio exactamente como estaba.
    pub fn merge(&self, name: &str) -> Result<String> {
        let head = self.inner.head().context("el repositorio no tiene commits")?;
        let refname = head.name().context("HEAD no tiene nombre")?.to_string();
        let current = head.shorthand().unwrap_or("HEAD").to_string();
        anyhow::ensure!(current != name, "{name} es la rama en la que estás");

        let reference = self
            .inner
            .resolve_reference_from_short_name(name)
            .with_context(|| format!("no hay ninguna rama llamada «{name}»"))?;
        let theirs = self.inner.reference_to_annotated_commit(&reference)?;

        let (analysis, _) = self.inner.merge_analysis(&[&theirs])?;
        if analysis.is_up_to_date() {
            return Ok(format!("{name} ya está dentro de {current}"));
        }

        let commit = self.inner.find_commit(theirs.id())?;

        if analysis.is_fast_forward() {
            // Aquí el modo seguro vale: lo que tengas a medias y no estorbe
            // viaja contigo, igual que al cambiar de rama.
            self.inner
                .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
                .context("hay cambios sin guardar que lo impiden; guárdalos o haz un stash")?;
            self.inner
                .find_reference(&refname)?
                .set_target(theirs.id(), "merge: avance rápido")?;

            return Ok(format!("{current} avanza hasta {name}, sin fusión que escribir"));
        }

        // De aquí en adelante hay que escribir un commit y reescribir el árbol
        // entero, y eso no se puede hacer con trabajo a medias por encima.
        anyhow::ensure!(
            self.status()?.is_clean(),
            "guarda o aparta lo que tienes a medias antes de fusionar"
        );

        let ours = head.peel(ObjectType::Commit)?.id();
        let ours = self.inner.find_commit(ours)?;
        let mut index = self.inner.merge_commits(&ours, &commit, None)?;

        if index.has_conflicts() {
            anyhow::bail!(
                "{name} choca con {current} en {}. Todavía no hay editor de conflictos, \
                 así que no se ha tocado nada",
                conflicting(&index)
            );
        }

        let tree = index.write_tree_to(&self.inner)?;
        let tree = self.inner.find_tree(tree)?;
        let signature = self
            .inner
            .signature()
            .context("falta configurar user.name y user.email en Git")?;

        self.inner.commit(
            Some(&refname),
            &signature,
            &signature,
            &format!("Merge {name} into {current}"),
            &tree,
            &[&ours, &commit],
        )?;

        // La rama ya está donde toca; ahora el disco. Forzar aquí no pisa
        // nada: el árbol estaba limpio antes de empezar.
        self.inner
            .checkout_head(Some(CheckoutBuilder::new().force()))
            .context("la fusión se escribió pero el árbol no se pudo actualizar")?;

        Ok(format!("{name} fusionada en {current}"))
    }
}

/// Los ficheros que chocan, como se los nombraría en voz alta: tres y lo que
/// quede. La lista entera no cabe en una barra de estado ni hace falta.
fn conflicting(index: &git2::Index) -> String {
    let Ok(conflicts) = index.conflicts() else {
        return "algún fichero".to_string();
    };

    let paths: Vec<String> = conflicts
        .filter_map(|conflict| conflict.ok())
        .filter_map(|conflict| conflict.our.or(conflict.their).or(conflict.ancestor))
        .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
        .collect();

    let shown: Vec<&str> = paths.iter().take(3).map(String::as_str).collect();
    let rest = paths.len() - shown.len();

    match rest {
        0 => shown.join(", "),
        1 => format!("{} y uno más", shown.join(", ")),
        n => format!("{} y {n} más", shown.join(", ")),
    }
}
