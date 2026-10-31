//! Juntar una rama con la actual.
//!
//! Lo que no hay aquí es tan importante como lo que hay: si la fusión sale con
//! conflictos, no se toca nada. Un repositorio a medio fusionar es un sitio
//! del que hay que saber salir, y mientras la aplicación no tenga editor de
//! conflictos no puede dejar a nadie ahí dentro.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, ErrorCode, ObjectType};

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
        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let current = head.shorthand().unwrap_or("HEAD").to_string();
        anyhow::ensure!(current != name, "{name} is the branch you are on");

        let reference = self
            .inner
            .resolve_reference_from_short_name(name)
            .with_context(|| format!("no branch called '{name}'"))?;
        let theirs = self.inner.reference_to_annotated_commit(&reference)?;

        let (analysis, _) = self.inner.merge_analysis(&[&theirs])?;
        if analysis.is_up_to_date() {
            return Ok(format!("{name} is already inside {current}"));
        }

        let commit = self.inner.find_commit(theirs.id())?;

        if analysis.is_fast_forward() {
            // Aquí el modo seguro vale: lo que tengas a medias y no estorbe
            // viaja contigo, igual que al cambiar de rama.
            self.inner
                .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
                .context("unsaved changes are in the way; commit them or stash them")?;
            self.inner
                .find_reference(&refname)?
                .set_target(theirs.id(), "merge: fast-forward")?;

            return Ok(format!("{current} moves up to {name}, with no merge to write"));
        }

        // De aquí en adelante hay que escribir un commit y reescribir el árbol
        // entero, y eso no se puede hacer con trabajo a medias por encima.
        anyhow::ensure!(
            self.status()?.is_clean(),
            "commit or stash what you have half-done before merging"
        );

        let ours = head.peel(ObjectType::Commit)?.id();
        let ours = self.inner.find_commit(ours)?;
        let mut index = self.inner.merge_commits(&ours, &commit, None)?;

        if index.has_conflicts() {
            anyhow::bail!(
                "{name} clashes with {current} in {}. There is no conflict editor yet, \
                 so nothing has been touched",
                conflicting(&index)
            );
        }

        let tree = index.write_tree_to(&self.inner)?;
        let tree = self.inner.find_tree(tree)?;
        let signature = self
            .inner
            .signature()
            .context("set user.name and user.email in Git first")?;

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
            .context("the merge was written but the working tree could not be updated")?;

        Ok(format!("{name} merged into {current}"))
    }
}

impl Repo {
    /// Trae una rama a la principal, cambiándose a ella si hace falta.
    ///
    /// Es lo que uno haría a mano: ponerse en la principal y fusionar.
    /// Arrastrar una tarjeta a «hecho» es una sola intención, así que se
    /// dice en una sola operación y no en dos que se pueden quedar a medias.
    ///
    /// Si el cambio de rama falla no se ha tocado nada. Si falla la fusión,
    /// te quedas en la principal, que es donde ibas: el aviso lo dice antes.
    pub fn land(&self, branch: &str) -> Result<String> {
        let trunk = self.trunk()?;
        anyhow::ensure!(branch != trunk, "{branch} is the main branch");

        let moved = self.head_name() != trunk;
        if moved {
            self.checkout_branch(&trunk)?;
        }

        let note = self.merge(branch)?;
        Ok(match moved {
            true => format!("{note}, and you are now on {trunk}"),
            false => note,
        })
    }

    /// Rehace los commits de la rama actual encima de otra.
    ///
    /// Deja la historia recta, que es para lo que se usa. Si al reaplicar un
    /// commit choca algo, se aborta el rebase entero: libgit2 sabe deshacerlo
    /// y devolver la rama a donde estaba, así que no hay manera de quedarse a
    /// medio camino. Terminarlo haría falta el editor de conflictos.
    pub fn rebase_onto(&self, upstream: &str) -> Result<String> {
        anyhow::ensure!(
            self.status()?.is_clean(),
            "commit or stash what you have half-done before rebasing"
        );

        let head = self.inner.head().context("the repository has no commits")?;
        let current = head.shorthand().unwrap_or("HEAD").to_string();
        let branch = self.inner.reference_to_annotated_commit(&head)?;

        let onto = self
            .inner
            .resolve_reference_from_short_name(upstream)
            .with_context(|| format!("no branch called '{upstream}'"))?;
        let onto = self.inner.reference_to_annotated_commit(&onto)?;

        let signature = self
            .inner
            .signature()
            .context("set user.name and user.email in Git first")?;

        let mut rebase = self.inner.rebase(Some(&branch), Some(&onto), None, None)?;
        let mut replayed = 0;

        while let Some(step) = rebase.next() {
            if let Err(err) = step {
                let _ = rebase.abort();
                return Err(err.into());
            }

            let index = self.inner.index()?;
            if index.has_conflicts() {
                let files = conflicting(&index);
                let _ = rebase.abort();
                anyhow::bail!(
                    "{current} clashes with {upstream} in {files}. There is no conflict \
                     editor yet, so the rebase was undone and nothing changed"
                );
            }

            match rebase.commit(None, &signature, None) {
                Ok(_) => replayed += 1,
                // El cambio ya estaba arriba: no es un fallo, es un commit
                // menos que reaplicar.
                Err(err) if err.code() == ErrorCode::Applied => {}
                Err(err) => {
                    let _ = rebase.abort();
                    return Err(err.into());
                }
            }
        }

        rebase.finish(Some(&signature))?;

        Ok(match replayed {
            0 => format!("{current} moved onto {upstream}, with nothing to replay"),
            1 => format!("1 commit replayed on top of {upstream}"),
            n => format!("{n} commits replayed on top of {upstream}"),
        })
    }
}

/// Los ficheros que chocan, como se los nombraría en voz alta: tres y lo que
/// quede. La lista entera no cabe en una barra de estado ni hace falta.
fn conflicting(index: &git2::Index) -> String {
    let Ok(conflicts) = index.conflicts() else {
        return "some file".to_string();
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
        1 => format!("{} and one more", shown.join(", ")),
        n => format!("{} and {n} more", shown.join(", ")),
    }
}
