//! Etiquetas: marcar un commit con un nombre que no se mueve.

use anyhow::{Context, Result};
use git2::Oid;

use crate::repo::Repo;

impl Repo {
    /// Pone una etiqueta en un commit.
    ///
    /// Con mensaje sale anotada: lleva quién la puso, cuándo y por qué, y es
    /// un objeto propio del repositorio. Sin mensaje sale ligera, que es solo
    /// un nombre apuntando al commit. Es la misma diferencia que hay entre
    /// `git tag -a` y `git tag` a secas, y aquí se elige escribiendo o no, sin
    /// un interruptor que explicar.
    ///
    /// No pisa una que ya exista. Una etiqueta vale justamente porque no se
    /// mueve; moverla en silencio sería quitarle lo único que tiene.
    pub fn tag(&self, name: &str, target: Oid, message: &str) -> Result<()> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "the tag needs a name");

        let object = self
            .inner
            .find_object(target, None)
            .context("that commit is no longer in the repository")?;
        let message = message.trim();

        if message.is_empty() {
            self.inner.tag_lightweight(name, &object, false).map(|_| ())
        } else {
            let tagger = self
                .inner
                .signature()
                .context("set user.name and user.email in Git first")?;
            self.inner.tag(name, &object, &tagger, message, false).map(|_| ())
        }
        .with_context(|| format!("could not create '{name}'"))
    }

    /// Quita una etiqueta. El commit que señalaba se queda donde estaba.
    ///
    /// Solo la de aquí: la que esté en un remoto sigue allí hasta que se borre
    /// allí, y eso todavía no se hace desde la aplicación.
    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.inner
            .tag_delete(name)
            .with_context(|| format!("could not delete '{name}'"))
    }
}
