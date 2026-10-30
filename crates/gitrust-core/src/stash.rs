//! La pila de cambios guardados a un lado.

use anyhow::{Context, Result};
use git2::{Oid, StashFlags};

use crate::repo::Repo;

/// Un apartado de la pila. El índice es su sitio en ella: el 0 es el último
/// que se guardó, y cambia en cuanto se saca o se suelta cualquiera de los
/// que tiene encima.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub id: Oid,
    pub time: i64,
}

impl Repo {
    /// La pila entera, del más reciente al más antiguo.
    pub fn stashes(&mut self) -> Result<Vec<StashEntry>> {
        let mut out = Vec::new();
        self.inner.stash_foreach(|index, message, id| {
            out.push(StashEntry {
                index,
                message: message.to_string(),
                id: *id,
                time: 0,
            });
            true
        })?;

        // La fecha vive en el commit del apartado, no en la pila, así que hay
        // que ir a buscarla. Si no se puede leer se queda en cero y la
        // interfaz calla la fecha en vez de inventarse una.
        for entry in &mut out {
            if let Ok(commit) = self.inner.find_commit(entry.id) {
                entry.time = commit.time().seconds();
            }
        }

        Ok(out)
    }

    /// Aparta todo lo que hay a medias y deja el árbol como el último commit.
    ///
    /// Los ficheros sin seguimiento se quedan donde están, igual que hace
    /// `git stash`: no estorban para traer del remoto ni para cambiar de rama,
    /// que es para lo que se aparta lo demás.
    pub fn stash_save(&mut self, message: &str) -> Result<String> {
        let signature = self
            .inner
            .signature()
            .context("falta configurar user.name y user.email en Git")?;

        let message = message.trim();
        let message = (!message.is_empty()).then_some(message);

        self.inner
            .stash_save2(&signature, message, Some(StashFlags::DEFAULT))
            .map_err(|err| match err.code() {
                // libgit2 llama «no encontrado» a no tener nada que guardar.
                git2::ErrorCode::NotFound => anyhow::anyhow!("no hay nada que apartar"),
                _ => anyhow::Error::from(err),
            })?;

        Ok("cambios apartados".to_string())
    }

    /// Devuelve un apartado al árbol de trabajo y lo saca de la pila.
    pub fn stash_pop(&mut self, index: usize) -> Result<String> {
        self.inner
            .stash_pop(index, None)
            .context("no se pudo recuperar: hay cambios que chocan con los apartados")?;
        Ok("cambios recuperados".to_string())
    }

    /// Lo mismo, pero dejándolo en la pila.
    pub fn stash_apply(&mut self, index: usize) -> Result<String> {
        self.inner
            .stash_apply(index, None)
            .context("no se pudo aplicar: hay cambios que chocan con los apartados")?;
        Ok("cambios aplicados, y siguen apartados".to_string())
    }

    /// Tira un apartado. Lo que hubiera en él no se puede recuperar.
    pub fn stash_drop(&mut self, index: usize) -> Result<String> {
        self.inner.stash_drop(index)?;
        Ok("apartado tirado".to_string())
    }
}
