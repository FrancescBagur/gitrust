//! Crear ramas y cambiar de una a otra.
//!
//! Cambiar de rama es la operación que más escribe en tu disco de todas las
//! que hay aquí, así que todo en este módulo está puesto para que, cuando algo
//! salga mal, salga mal antes de haber tocado nada.

use anyhow::{Context, Result};
use git2::{build::CheckoutBuilder, BranchType, ErrorCode, ObjectType};

use crate::repo::Repo;

impl Repo {
    /// Crea una rama donde está HEAD y se cambia a ella.
    ///
    /// El árbol de trabajo no se toca: la rama nueva apunta al mismo commit
    /// que la vieja, así que lo que hubiera a medias sigue ahí y se puede
    /// commitear ya en la rama nueva. Es la manera de darse cuenta tarde de
    /// que uno estaba escribiendo en la rama equivocada.
    pub fn branch_here(&self, name: &str) -> Result<()> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "the branch needs a name");

        let commit = self
            .inner
            .head()
            .and_then(|head| head.peel_to_commit())
            .context("the repository has no commits to branch from")?;

        self.inner
            .branch(name, &commit, false)
            .with_context(|| format!("could not create '{name}'"))?;
        self.inner.set_head(&format!("refs/heads/{name}"))?;
        Ok(())
    }

    /// Se cambia a una rama local que ya existe.
    ///
    /// Primero el árbol y después HEAD. Si el árbol falla —porque hay cambios
    /// sin guardar que chocan con lo que habría que escribir—, HEAD se queda
    /// donde estaba y no hay nada a medias que arreglar.
    ///
    /// El cambio va en modo seguro, que es el de `git checkout`: lo que tienes
    /// cambiado y no estorba viaja contigo a la rama nueva, y lo que estorba
    /// para el cambio entero. Los cambios sin guardar son tuyos; no es cosa
    /// nuestra pisarlos para salir del paso.
    pub fn checkout_branch(&self, name: &str) -> Result<String> {
        let reference = self
            .inner
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("no branch called '{name}'"))?
            .into_reference();

        let refname = reference.name().context("the branch has no name")?.to_string();
        let commit = reference.peel_to_commit()?;

        self.inner
            .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
            .map_err(|err| blocked(err, name))?;
        self.inner.set_head(&refname)?;

        Ok(format!("you are now on {name}"))
    }

    /// Se cambia a una rama remota creando su pareja local.
    ///
    /// Es lo que hace `git checkout rama` cuando la rama solo existe en el
    /// remoto: crea la local donde apunta la remota y la deja siguiéndola. Si
    /// la local ya existe no se toca y se va a ella, porque puede tener
    /// trabajo que la remota todavía no conoce.
    pub fn checkout_remote(&self, name: &str) -> Result<String> {
        let (_, local) = name
            .split_once('/')
            .context("a remote branch is named remote/branch")?;

        if self.inner.find_branch(local, BranchType::Local).is_ok() {
            return self.checkout_branch(local);
        }

        let commit = self
            .inner
            .find_branch(name, BranchType::Remote)
            .with_context(|| format!("no remote branch called '{name}'"))?
            .into_reference()
            .peel_to_commit()?;

        self.inner
            .branch(local, &commit, false)
            .with_context(|| format!("could not create '{local}'"))?
            .set_upstream(Some(name))?;

        self.checkout_branch(local)
    }

    /// Borra una rama local.
    ///
    /// La actual no: borrarla dejaría HEAD señalando a algo que ya no existe,
    /// y salir de ahí no se puede desde la aplicación.
    pub fn delete_branch(&self, name: &str) -> Result<()> {
        let head = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string));
        anyhow::ensure!(
            head.as_deref() != Some(name),
            "{name} is the branch you are on: switch to another one first"
        );

        self.inner
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("no branch called '{name}'"))?
            .delete()
            .with_context(|| format!("could not delete '{name}'"))
    }

    /// Cuántos commits de esa rama no están en ninguna otra parte.
    ///
    /// Es exactamente lo que se pierde al borrarla. Cero quiere decir que
    /// todo lo suyo ya está en otro sitio —otra rama, un remoto, una
    /// etiqueta— y que borrarla no se lleva nada por delante.
    ///
    /// Se calcula recorriendo su historia y escondiendo la de todas las demás
    /// referencias, que es lo mismo que hace `git branch -d` para decidir si
    /// se niega.
    pub fn unmerged(&self, name: &str) -> Result<usize> {
        let refname = format!("refs/heads/{name}");
        let target = self
            .inner
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("no branch called '{name}'"))?
            .get()
            .peel(ObjectType::Commit)?
            .id();

        let mut walk = self.inner.revwalk()?;
        walk.push(target)?;

        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(other) = reference.name() else {
                continue;
            };
            if other == refname {
                continue;
            }
            if let Ok(object) = reference.peel(ObjectType::Commit) {
                walk.hide(object.id())?;
            }
        }

        Ok(walk.count())
    }
}

/// Traduce el «N conflicts prevent checkout» de libgit2 a algo que se pueda
/// hacer. El número de conflictos no le sirve a nadie; saber que hay un stash
/// a un clic, sí.
fn blocked(err: git2::Error, name: &str) -> anyhow::Error {
    if err.code() == ErrorCode::Conflict {
        anyhow::anyhow!(
            "unsaved changes clash with {name}: commit them, or set them aside with Stash"
        )
    } else {
        err.into()
    }
}
