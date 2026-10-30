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
