//! Tipos de datos compartidos entre el lector del repositorio y la UI.

use git2::Oid;

/// Qué clase de referencia apunta a un commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// Una etiqueta visible sobre un commit: una rama, un tag o HEAD.
#[derive(Debug, Clone)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

/// Un commit, ya aplanado a los campos que la UI necesita pintar.
#[derive(Debug, Clone)]
pub struct Commit {
    pub id: Oid,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub time: i64,
    pub parents: Vec<Oid>,
    pub refs: Vec<RefLabel>,
}

impl Commit {
    /// El commit que todavía no existe: los cambios del árbol de trabajo.
    ///
    /// Git no le da identificador al árbol de trabajo, así que esta fila lleva
    /// el Oid nulo. Ningún commit real puede tenerlo, de modo que es lo que la
    /// distingue de las demás sin necesidad de un campo aparte, y el trazado
    /// del grafo la trata como a cualquier otra: cuelga de HEAD por su padre.
    pub fn working(head: Oid) -> Self {
        Self {
            id: Oid::zero(),
            short_id: String::new(),
            summary: String::new(),
            author: String::new(),
            time: 0,
            parents: vec![head],
            refs: Vec::new(),
        }
    }

    /// Si esta fila son los cambios sin confirmar en vez de un commit.
    pub fn is_working(&self) -> bool {
        self.id.is_zero()
    }

    /// Las referencias que señalan aquí, de la que más importa a la que menos,
    /// y cada una diciendo si es donde uno está.
    ///
    /// La rama en la que estás va primera: es la respuesta a la pregunta que
    /// uno se hace al mirar la historia. Detrás, las demás locales, luego las
    /// etiquetas y al final las remotas, que casi siempre repiten a una local
    /// y son las primeras que sobran cuando no cabe todo.
    ///
    /// La marca de HEAD no sale por su cuenta si hay una rama local con su
    /// mismo nombre: es esa rama la que está puesta, y decirlo dos veces no
    /// dice más. Solo con HEAD desacoplado se queda sola.
    pub fn ordered_refs(&self) -> Vec<(&RefLabel, bool)> {
        let head = self
            .refs
            .iter()
            .find(|label| label.kind == RefKind::Head)
            .map(|label| label.name.as_str());

        let doubled = head.is_some_and(|name| {
            self.refs
                .iter()
                .any(|label| label.kind == RefKind::LocalBranch && label.name == name)
        });

        let mut out: Vec<(&RefLabel, bool)> = self
            .refs
            .iter()
            .filter(|label| !(doubled && label.kind == RefKind::Head))
            .map(|label| {
                let current = match label.kind {
                    RefKind::Head => true,
                    RefKind::LocalBranch => Some(label.name.as_str()) == head,
                    _ => false,
                };
                (label, current)
            })
            .collect();

        // Estable: dentro de un mismo rango se respeta el orden en que vinieron.
        out.sort_by_key(|(label, current)| rank(label.kind, *current));
        out
    }
}

/// Cuánto importa una referencia. Cero es la que no puede faltar.
fn rank(kind: RefKind, current: bool) -> u8 {
    if current {
        return 0;
    }
    match kind {
        RefKind::Head => 0,
        RefKind::LocalBranch => 1,
        RefKind::Tag => 2,
        RefKind::RemoteBranch => 3,
    }
}

/// Una línea dibujada en la banda que va de una fila a la siguiente.
///
/// `from` es el carril en la fila actual y `to` el carril en la fila de abajo.
/// Cuando ambos coinciden la línea es vertical; si no, describe una curva.
#[derive(Debug, Clone, Copy)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    pub color: usize,
}

/// Una fila del grafo: el commit, el carril donde va su punto y las líneas
/// que salen de ella hacia la fila siguiente.
#[derive(Debug, Clone)]
pub struct Row {
    pub commit: Commit,
    pub lane: usize,
    pub color: usize,
    pub links: Vec<Link>,
}

/// El grafo completo, listo para pintar.
#[derive(Debug, Default)]
pub struct Graph {
    pub rows: Vec<Row>,
    pub lane_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(refs: &[(&str, RefKind)]) -> Commit {
        Commit {
            id: Oid::zero(),
            short_id: String::new(),
            summary: String::new(),
            author: String::new(),
            time: 0,
            parents: Vec::new(),
            refs: refs
                .iter()
                .map(|(name, kind)| RefLabel { name: name.to_string(), kind: *kind })
                .collect(),
        }
    }

    fn names(commit: &Commit) -> Vec<(&str, bool)> {
        commit
            .ordered_refs()
            .into_iter()
            .map(|(label, current)| (label.name.as_str(), current))
            .collect()
    }

    #[test]
    fn la_rama_puesta_absorbe_la_marca_de_head() {
        let commit = commit(&[
            ("main", RefKind::Head),
            ("main", RefKind::LocalBranch),
            ("origin/main", RefKind::RemoteBranch),
        ]);
        assert_eq!(names(&commit), [("main", true), ("origin/main", false)]);
    }

    #[test]
    fn entre_varias_locales_solo_una_es_la_puesta() {
        let commit = commit(&[
            ("arreglo", RefKind::Head),
            ("main", RefKind::LocalBranch),
            ("arreglo", RefKind::LocalBranch),
        ]);
        assert_eq!(names(&commit), [("arreglo", true), ("main", false)]);
    }

    #[test]
    fn con_head_desacoplado_la_marca_se_queda_sola() {
        let commit = commit(&[("HEAD", RefKind::Head), ("v1.0", RefKind::Tag)]);
        assert_eq!(names(&commit), [("HEAD", true), ("v1.0", false)]);
    }

    #[test]
    fn las_remotas_son_las_ultimas_y_las_etiquetas_van_antes() {
        let commit = commit(&[
            ("origin/main", RefKind::RemoteBranch),
            ("v2", RefKind::Tag),
            ("main", RefKind::LocalBranch),
        ]);
        assert_eq!(
            names(&commit),
            [("main", false), ("v2", false), ("origin/main", false)]
        );
    }

    #[test]
    fn un_commit_sin_referencias_no_da_ninguna() {
        assert!(commit(&[]).ordered_refs().is_empty());
    }
}
