//! El trabajo en marcha, leído del grafo.
//!
//! Una rama es una unidad de trabajo, y el repositorio ya sabe en qué punto
//! está cada una. Este módulo no guarda nada ni pregunta nada: mira dónde cae
//! cada rama respecto a la principal y deduce la columna. Un tablero que se
//! calcula no se puede quedar desfasado, que es lo que le pasa a todos los
//! tableros que hay que mantener a mano.
//!
//! Lo que no puede saber, y conviene no fingir: Git no guarda cuándo se creó
//! una rama. Una rama terminada y una que nunca empezó acaban las dos sin nada
//! propio, y lo único que las separa es si la principal se la tragó en algún
//! merge. Por eso «hecho» se lee de los commits de fusión, no de la distancia:
//! una rama que entró por avance rápido, o aplastada en un solo commit, se
//! queda en «por hacer». Mejor eso que decirte que terminaste algo que no.

use std::collections::HashSet;

use anyhow::{Context, Result};
use git2::{BranchType, ObjectType, Oid};

use crate::repo::Repo;

/// En qué punto está una rama.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Creada y todavía en el mismo sitio que la principal.
    Todo,
    /// Tiene commits propios y no ha salido de esta máquina.
    InProgress,
    /// Tiene commits propios y ya está en el remoto, esperando a alguien.
    InReview,
    /// Todo lo suyo está dentro de la principal.
    Done,
}

impl Lane {
    /// Las cuatro, en el orden en que se leen de izquierda a derecha.
    pub const ALL: [Lane; 4] = [Lane::Todo, Lane::InProgress, Lane::InReview, Lane::Done];
}

/// Una rama vista como trabajo.
#[derive(Debug, Clone)]
pub struct Card {
    pub branch: String,
    /// La carpeta de su nombre: `tema` en `tema/colores`. Es lo que hace de
    /// proyecto, porque es lo que la gente ya usa para decir que dos ramas
    /// son la misma faena.
    pub project: Option<String>,
    pub lane: Lane,
    /// Commits suyos que la principal no tiene.
    pub ahead: usize,
    /// Commits de la principal que a ella le faltan.
    pub behind: usize,
    /// Cuándo se escribió lo último que tiene.
    pub time: i64,
    /// Si es la rama que está puesta ahora mismo.
    pub current: bool,
    pub target: Oid,
}

impl Card {
    /// El nombre sin la carpeta, que es como se llama a la rama en voz alta.
    pub fn leaf(&self) -> &str {
        self.branch.rsplit_once('/').map_or(self.branch.as_str(), |(_, leaf)| leaf)
    }
}

/// El tablero entero.
#[derive(Debug, Clone)]
pub struct Board {
    pub cards: Vec<Card>,
    /// La rama contra la que se mide todo lo demás.
    pub trunk: String,
}

impl Board {
    /// Las tarjetas de una columna, en el orden en que ya vienen.
    pub fn lane(&self, lane: Lane) -> impl Iterator<Item = &Card> {
        self.cards.iter().filter(move |card| card.lane == lane)
    }

    /// Los proyectos que hay, sin repetir y por orden.
    pub fn projects(&self) -> Vec<&str> {
        let mut names: Vec<&str> =
            self.cards.iter().filter_map(|card| card.project.as_deref()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

impl Repo {
    /// Clasifica cada rama local según dónde cae respecto a la principal.
    pub fn board(&self) -> Result<Board> {
        let trunk = self.trunk()?;
        let tip = self
            .inner
            .find_branch(&trunk, BranchType::Local)
            .with_context(|| format!("no branch called '{trunk}'"))?
            .get()
            .peel(ObjectType::Commit)?
            .id();

        let head = self
            .inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string));

        // Las puntas que la principal absorbió en algún merge. Es lo único
        // que distingue una rama terminada de una que nunca llegó a empezar.
        let mut swallowed = HashSet::new();
        let mut walk = self.inner.revwalk()?;
        walk.push(tip)?;
        for oid in walk.flatten() {
            let Ok(commit) = self.inner.find_commit(oid) else {
                continue;
            };
            if commit.parent_count() > 1 {
                swallowed.extend(commit.parent_ids().skip(1));
            }
        }

        // Qué ramas conoce algún remoto, de una pasada: preguntarlo rama por
        // rama sería recorrer la lista entera otras tantas veces.
        let mut published = HashSet::new();
        for reference in self.inner.references()? {
            let Some(name) = reference?.name().map(str::to_string) else {
                continue;
            };
            if let Some(rest) = name.strip_prefix("refs/remotes/") {
                if let Some((_, branch)) = rest.split_once('/') {
                    published.insert(branch.to_string());
                }
            }
        }

        let mut cards = Vec::new();
        for branch in self.inner.branches(Some(BranchType::Local))? {
            let (branch, _) = branch?;
            let Some(name) = branch.name()?.map(str::to_string) else {
                continue;
            };
            if name == trunk {
                continue;
            }
            let Ok(object) = branch.get().peel(ObjectType::Commit) else {
                continue;
            };

            let target = object.id();
            let (ahead, behind) = self.inner.graph_ahead_behind(target, tip)?;

            // Con algo propio, la columna depende de si ha salido de aquí.
            // Sin nada propio, de si la principal se la tragó o es que la
            // rama todavía no ha hecho nada.
            let lane = if ahead > 0 {
                match published.contains(&name) {
                    true => Lane::InReview,
                    false => Lane::InProgress,
                }
            } else if swallowed.contains(&target) {
                Lane::Done
            } else {
                Lane::Todo
            };

            let time = self
                .inner
                .find_commit(target)
                .map(|commit| commit.time().seconds())
                .unwrap_or(0);

            cards.push(Card {
                project: name.rsplit_once('/').map(|(folder, _)| folder.to_string()),
                current: head.as_deref() == Some(name.as_str()),
                branch: name,
                lane,
                ahead,
                behind,
                time,
                target,
            });
        }

        // Lo último que se tocó, primero: un tablero se lee por arriba.
        cards.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.branch.cmp(&b.branch)));
        Ok(Board { cards, trunk })
    }

    /// La rama contra la que se mide el resto del trabajo.
    ///
    /// Primero se le pregunta al remoto, que es quien lo sabe de verdad: su
    /// HEAD apunta a la rama por defecto del repositorio. Si no hay remoto, se
    /// prueban los nombres de siempre. Y si tampoco, la rama que esté puesta,
    /// que al menos no es una invención.
    pub fn trunk(&self) -> Result<String> {
        for remote in self.inner.remotes()?.iter().flatten() {
            let Ok(reference) = self.inner.find_reference(&format!("refs/remotes/{remote}/HEAD"))
            else {
                continue;
            };
            let Some(target) = reference.symbolic_target() else {
                continue;
            };
            let Some(name) = target.strip_prefix(&format!("refs/remotes/{remote}/")) else {
                continue;
            };
            if self.inner.find_branch(name, BranchType::Local).is_ok() {
                return Ok(name.to_string());
            }
        }

        for name in ["main", "master", "trunk", "develop"] {
            if self.inner.find_branch(name, BranchType::Local).is_ok() {
                return Ok(name.to_string());
            }
        }

        self.inner
            .head()
            .ok()
            .and_then(|head| head.shorthand().map(str::to_string))
            .context("the repository has no branches")
    }
}
