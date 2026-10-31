//! Hablar con el remoto: traer, llevar, y saber cuánto se debe de cada lado.
//!
//! Las tres operaciones devuelven la frase que hay que enseñar al terminar, en
//! vez de un montón de datos que la interfaz tendría que redactar. Lo que
//! cuenta de un `pull` no es cuántos objetos se han bajado, sino si la rama se
//! movió y adónde.

use anyhow::{Context, Result};
use std::collections::HashMap;

use git2::{
    build::CheckoutBuilder, BranchType, Config, Cred, CredentialType, Direction, FetchOptions,
    ObjectType, Oid, PushOptions, Remote, RemoteCallbacks,
};

use crate::repo::Repo;

/// Qué hacer con lo que viene del remoto cuando no basta con avanzar.
///
/// Son las mismas tres que ofrece `git pull`, y la de siempre es la primera:
/// avanzar si se puede y, si no, juntar las dos historias con un commit que
/// lo diga. Las otras dos existen porque hay quien no quiere ninguna de esas
/// dos cosas, y es una preferencia legítima, no un detalle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullMode {
    /// Avanza si puede; si no, escribe un commit de fusión.
    Merge,
    /// Solo avanza. Con las historias divergidas, se para y lo dice.
    FastForward,
    /// Rehace tus commits encima de lo que viene, dejando la historia recta.
    Rebase,
}

/// Cuánto se aparta la rama actual de la que sigue en el remoto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracking {
    /// El nombre corto de la rama remota: «origin/main».
    pub remote: String,
    /// Commits que hay aquí y no allí.
    pub ahead: usize,
    /// Commits que hay allí y no aquí.
    pub behind: usize,
}

impl Repo {
    /// La rama remota que sigue la actual, y la distancia a ella.
    ///
    /// Es `None` cuando la rama no sigue a ninguna, que es lo normal mientras
    /// no se ha hecho el primer `push`. No distingue ese caso de un fallo al
    /// leer: para lo único que sirve esto es para pintar dos cuentas, y si no
    /// se pueden saber no se pintan.
    pub fn tracking(&self) -> Option<Tracking> {
        let head = self.inner.head().ok()?;
        let upstream = self.inner.branch_upstream_name(head.name()?).ok()?;
        let upstream = upstream.as_str()?;

        let ours = head.peel(ObjectType::Commit).ok()?.id();
        let theirs = self
            .inner
            .find_reference(upstream)
            .ok()?
            .peel(ObjectType::Commit)
            .ok()?
            .id();

        let (ahead, behind) = self.inner.graph_ahead_behind(ours, theirs).ok()?;
        Some(Tracking {
            remote: upstream.strip_prefix("refs/remotes/").unwrap_or(upstream).to_string(),
            ahead,
            behind,
        })
    }

    /// Trae lo que haya en el remoto sin tocar el árbol de trabajo.
    pub fn fetch(&self) -> Result<String> {
        let (remote, _) = self.upstream()?;
        self.fetch_from(&remote)?;

        match self.tracking() {
            Some(t) if t.behind > 0 => Ok(format!("{} to pull from {}", commits(t.behind), t.remote)),
            _ => Ok(format!("{remote} has nothing new")),
        }
    }

    /// Trae de todos los remotos, siga la rama actual a alguno o a ninguno.
    pub fn fetch_all(&self) -> Result<String> {
        let remotes: Vec<String> =
            self.inner.remotes()?.iter().flatten().map(str::to_string).collect();
        anyhow::ensure!(!remotes.is_empty(), "this repository has no remotes");

        for remote in &remotes {
            self.fetch_from(remote)?;
        }

        match self.tracking() {
            Some(t) if t.behind > 0 => Ok(format!("{} to pull from {}", commits(t.behind), t.remote)),
            _ if remotes.len() == 1 => Ok(format!("{} has nothing new", remotes[0])),
            _ => Ok(format!("fetched from {} remotes", remotes.len())),
        }
    }

    /// Trae del remoto y junta lo que venga con lo que hay, como se le pida.
    ///
    /// Lo de traer es igual en los tres modos; lo que cambia es qué se hace
    /// cuando las dos historias se han separado. Avanzar, cuando se puede, se
    /// hace siempre: no hay nada que juntar si una va por delante de la otra.
    pub fn pull(&self, mode: PullMode) -> Result<String> {
        let (remote, branch) = self.upstream()?;
        self.fetch_from(&remote)?;
        let upstream = format!("{remote}/{branch}");

        match mode {
            PullMode::Merge => self.merge(&upstream),
            PullMode::FastForward => self.fast_forward(&remote, &branch),
            PullMode::Rebase => self.rebase_onto(&upstream),
        }
    }

    /// Adelanta la rama solo si se puede hacer sin fusionar nada.
    fn fast_forward(&self, remote: &str, branch: &str) -> Result<String> {
        let head = self.inner.head()?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let upstream = self.inner.branch_upstream_name(&refname)?;
        let upstream = upstream.as_str().context("the remote has no name")?;

        let theirs = self.inner.find_reference(upstream)?;
        let theirs = self.inner.reference_to_annotated_commit(&theirs)?;
        let (analysis, _) = self.inner.merge_analysis(&[&theirs])?;

        if analysis.is_up_to_date() {
            return Ok(format!("{branch} was already up to date"));
        }
        anyhow::ensure!(
            analysis.is_fast_forward(),
            "{branch} and {remote}/{branch} have diverged: pull with merge or rebase instead"
        );

        let behind = self.tracking().map(|t| t.behind).unwrap_or(0);

        // El orden importa: primero el árbol, después la referencia. Si el
        // árbol falla —porque hay cambios sin guardar que estorban—, la rama
        // se queda donde estaba y no hay nada que deshacer.
        let commit = self.inner.find_commit(theirs.id())?;
        self.inner
            .checkout_tree(commit.as_object(), Some(CheckoutBuilder::new().safe()))
            .context("unsaved changes are in the way; commit them or stash them")?;
        self.inner
            .find_reference(&refname)?
            .set_target(theirs.id(), "pull: fast-forward")?;

        Ok(format!("{} from {remote}/{branch}", commits(behind)))
    }

    /// Lleva la rama actual al remoto, y la deja siguiéndolo si no lo hacía.
    pub fn push(&self) -> Result<String> {
        let head = self.inner.head()?;
        let branch = head
            .shorthand()
            .filter(|_| head.is_branch())
            .context("HEAD is detached: there is no branch to push")?
            .to_string();

        self.push_branch(&branch)
    }

    /// Lo mismo, pero con la rama dicha por su nombre.
    ///
    /// Hace falta para llevar al remoto una rama en la que no estás, que es
    /// lo que pide arrastrar su tarjeta en el tablero.
    pub fn push_branch(&self, branch: &str) -> Result<String> {
        let refname = format!("refs/heads/{branch}");
        anyhow::ensure!(
            self.inner.find_reference(&refname).is_ok(),
            "no branch called '{branch}'"
        );

        // Sin remoto configurado se prueba «origin», que es como se llama en
        // la inmensa mayoría de los repositorios.
        let remote_name = self.remote_of(&refname);

        let ahead = self.tracking().map(|t| t.ahead).unwrap_or(0);
        let mut remote = self
            .inner
            .find_remote(&remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        // El rechazo del servidor no llega como error de `push`: llega por
        // esta llamada, y sin recogerlo un push rechazado parecería ir bien.
        let refused = std::cell::RefCell::new(None);
        {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(credentials(self.inner.config()?));
            callbacks.push_update_reference(|_, status| {
                if let Some(reason) = status {
                    *refused.borrow_mut() = Some(reason.to_string());
                }
                Ok(())
            });

            let mut options = PushOptions::new();
            options.remote_callbacks(callbacks);
            remote
                .push(&[format!("{refname}:{refname}")], Some(&mut options))
                .with_context(|| format!("could not push {branch} to {remote_name}"))?;
        }

        if let Some(reason) = refused.into_inner() {
            anyhow::bail!("{remote_name} rejected {branch}: {reason}");
        }

        // Una rama recién creada no sigue a nada: después del primer envío ya
        // hay a quién seguir, y dejarlo hecho ahorra el `--set-upstream`.
        if self.inner.branch_upstream_name(&refname).is_err() {
            let mut local = self.inner.find_branch(branch, git2::BranchType::Local)?;
            let _ = local.set_upstream(Some(&format!("{remote_name}/{branch}")));
        }

        Ok(format!("{} to {remote_name}/{branch}", commits(ahead)))
    }

    /// Lleva la rama al remoto pisando lo que haya, pero solo si lo que hay
    /// sigue siendo lo que viste la última vez.
    ///
    /// Eso es el «with lease» de `git push --force-with-lease`, y es lo que
    /// separa rehacer tu propia historia de borrarle el trabajo a otro sin
    /// que se entere nadie. Se pregunta al remoto antes de escribir: si ha
    /// cambiado desde tu última visita, no se fuerza y se dice por qué.
    pub fn push_force(&self) -> Result<String> {
        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?.to_string();
        let branch = refname
            .strip_prefix("refs/heads/")
            .context("HEAD is detached: there is no branch to push")?
            .to_string();

        let remote_name = self.remote_of(&refname);
        let mut remote = self
            .inner
            .find_remote(&remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        // Lo que vimos la última vez que trajimos.
        let seen = self
            .inner
            .find_branch(&format!("{remote_name}/{branch}"), BranchType::Remote)
            .ok()
            .and_then(|branch| branch.get().target());

        let theirs = self.heads_of(&mut remote)?.get(&refname).copied();
        anyhow::ensure!(
            theirs == seen,
            "{remote_name}/{branch} has changed since you last fetched: \
             fetch and look at what is there before overwriting it"
        );

        // El «+» delante del refspec es lo que autoriza a no avanzar.
        self.send(&mut remote, &[format!("+{refname}:{refname}")])?;
        Ok(format!("{branch} forced onto {remote_name}"))
    }

    /// Manda al remoto las etiquetas que no tenga.
    ///
    /// Las que ya están no se vuelven a enviar, y una que haya cambiado de
    /// sitio se queda aquí: el remoto la rechaza y eso está bien, porque una
    /// etiqueta que se mueve deja de valer para lo único que sirve.
    pub fn push_tags(&self) -> Result<String> {
        let remote_name = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.name().map(|name| self.remote_of(name)))
            .unwrap_or_else(|| "origin".to_string());

        let mut remote = self
            .inner
            .find_remote(&remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        let theirs = self.heads_of(&mut remote)?;
        let mut missing = Vec::new();

        for reference in self.inner.references()? {
            let reference = reference?;
            let Some(name) = reference.name() else {
                continue;
            };
            if !name.starts_with("refs/tags/") {
                continue;
            }
            if theirs.contains_key(name) {
                continue;
            }
            missing.push(format!("{name}:{name}"));
        }

        if missing.is_empty() {
            return Ok(format!("{remote_name} already has every tag"));
        }

        let count = missing.len();
        self.send(&mut remote, &missing)?;
        Ok(format!(
            "{count} {} to {remote_name}",
            if count == 1 { "tag sent" } else { "tags sent" }
        ))
    }

    /// Qué referencias tiene el remoto ahora mismo.
    fn heads_of(&self, remote: &mut Remote<'_>) -> Result<HashMap<String, Oid>> {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(credentials(self.inner.config()?));

        let connection = remote
            .connect_auth(Direction::Push, Some(callbacks), None)
            .context("could not reach the remote")?;

        let heads = connection
            .list()?
            .iter()
            .map(|head| (head.name().to_string(), head.oid()))
            .collect();

        Ok(heads)
    }

    /// Manda unos refspecs y se entera de si el servidor dice que no.
    fn send(&self, remote: &mut Remote<'_>, refspecs: &[String]) -> Result<()> {
        let refused = std::cell::RefCell::new(None);
        {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(credentials(self.inner.config()?));
            callbacks.push_update_reference(|name, status| {
                if let Some(reason) = status {
                    *refused.borrow_mut() = Some(format!("{name}: {reason}"));
                }
                Ok(())
            });

            let mut options = PushOptions::new();
            options.remote_callbacks(callbacks);
            remote.push(refspecs, Some(&mut options))?;
        }

        match refused.into_inner() {
            Some(reason) => anyhow::bail!("the remote refused — {reason}"),
            None => Ok(()),
        }
    }

    /// El remoto de una rama, o «origin», que es como se llama casi siempre.
    fn remote_of(&self, refname: &str) -> String {
        self.inner
            .branch_upstream_remote(refname)
            .ok()
            .and_then(|buf| buf.as_str().map(str::to_string))
            .unwrap_or_else(|| "origin".to_string())
    }

    /// Borra una rama en el remoto, y de paso su rastro aquí.
    ///
    /// Un refspec sin nada a la izquierda es cómo se le dice a Git que la
    /// referencia del otro lado sobra. Es lo mismo que `git push origin :rama`.
    ///
    /// Esto se ve desde todas las máquinas, no solo desde esta.
    pub fn delete_remote_branch(&self, name: &str) -> Result<String> {
        let (remote_name, branch) = name
            .split_once('/')
            .context("a remote branch is named remote/branch")?;

        let mut remote = self
            .inner
            .find_remote(remote_name)
            .with_context(|| format!("no remote called '{remote_name}'"))?;

        let refused = std::cell::RefCell::new(None);
        {
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(credentials(self.inner.config()?));
            callbacks.push_update_reference(|_, status| {
                if let Some(reason) = status {
                    *refused.borrow_mut() = Some(reason.to_string());
                }
                Ok(())
            });

            let mut options = PushOptions::new();
            options.remote_callbacks(callbacks);
            remote
                .push(&[format!(":refs/heads/{branch}")], Some(&mut options))
                .with_context(|| format!("could not delete {branch} on {remote_name}"))?;
        }

        if let Some(reason) = refused.into_inner() {
            anyhow::bail!("{remote_name} refused: {reason}");
        }

        // Y la copia local de esa rama remota, que si no se queda señalando a
        // algo que ya no está.
        if let Ok(mut local) = self.inner.find_branch(name, git2::BranchType::Remote) {
            let _ = local.delete();
        }

        Ok(format!("{name} deleted on the remote"))
    }

    /// El remoto al que mira la rama actual, y su nombre.
    fn upstream(&self) -> Result<(String, String)> {
        let head = self.inner.head().context("the repository has no commits")?;
        let refname = head.name().context("HEAD has no name")?;
        let branch = refname
            .strip_prefix("refs/heads/")
            .context("HEAD is detached: there is no branch to track")?
            .to_string();

        let remote = self
            .inner
            .branch_upstream_remote(refname)
            .ok()
            .and_then(|buf| buf.as_str().map(str::to_string))
            .with_context(|| format!("{branch} does not track any remote branch"))?;

        Ok((remote, branch))
    }

    fn fetch_from(&self, remote: &str) -> Result<()> {
        let mut remote = self
            .inner
            .find_remote(remote)
            .with_context(|| format!("no remote called '{remote}'"))?;

        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(credentials(self.inner.config()?));

        let mut options = FetchOptions::new();
        options.remote_callbacks(callbacks);

        // Sin refspecs usa las del remoto, que es lo que hace `git fetch`.
        let empty: [&str; 0] = [];
        remote.fetch(&empty, Some(&mut options), None).context("could not fetch from the remote")?;
        Ok(())
    }
}

/// Las credenciales, en el mismo orden que las prueba `git`: el agente SSH, el
/// ayudante que haya configurado, y el usuario a secas para los remotos SSH
/// que lo piden antes que nada.
///
/// Cada vía se prueba una sola vez. libgit2 vuelve a llamar mientras no se le
/// dé algo que sirva, así que sin llevar la cuenta esto daría vueltas para
/// siempre contra un servidor que rechace la primera clave.
pub(crate) fn credentials(
    config: Config,
) -> impl FnMut(&str, Option<&str>, CredentialType) -> Result<Cred, git2::Error> {
    let mut used = CredentialType::empty();

    move |url, username, allowed| {
        let user = username.unwrap_or("git");
        let mut once = |kind: CredentialType| {
            let first = allowed.contains(kind) && !used.contains(kind);
            if first {
                used |= kind;
            }
            first
        };

        if once(CredentialType::SSH_KEY) {
            return Cred::ssh_key_from_agent(user);
        }
        if once(CredentialType::USER_PASS_PLAINTEXT) {
            return Cred::credential_helper(&config, url, username);
        }
        if once(CredentialType::USERNAME) {
            return Cred::username(user);
        }
        if once(CredentialType::DEFAULT) {
            return Cred::default();
        }

        Err(git2::Error::from_str(
            "no credentials for this remote: try loading your key into the SSH agent",
        ))
    }
}

/// «1 commit», «4 commits».
fn commits(count: usize) -> String {
    if count == 1 {
        "1 commit".to_string()
    } else {
        format!("{count} commits")
    }
}
