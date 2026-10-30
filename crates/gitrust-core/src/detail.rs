//! Detalle de un commit y de las referencias del repositorio.

use git2::Oid;

use crate::model::RefKind;
use crate::status::ChangeKind;

/// Un fichero tocado por un commit.
#[derive(Debug, Clone)]
pub struct CommitFile {
    pub path: String,
    pub kind: ChangeKind,
}

/// Todo lo que el panel de detalle necesita de un commit.
#[derive(Debug, Clone)]
pub struct CommitDetail {
    pub id: Oid,
    pub short_id: String,
    pub summary: String,
    /// El cuerpo del mensaje, sin el asunto ni la línea en blanco.
    pub body: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub parents: Vec<Oid>,
    pub files: Vec<CommitFile>,
}

impl CommitDetail {
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// Cuántos ficheros se añaden, se modifican y se borran.
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut added = 0;
        let mut modified = 0;
        let mut deleted = 0;
        for file in &self.files {
            match file.kind {
                ChangeKind::Added => added += 1,
                ChangeKind::Deleted => deleted += 1,
                _ => modified += 1,
            }
        }
        (added, modified, deleted)
    }
}

/// Una referencia del repositorio, para la barra lateral.
#[derive(Debug, Clone)]
pub struct RefEntry {
    /// Nombre corto: `main`, `origin/main`, `v1.0`.
    pub name: String,
    pub kind: RefKind,
    pub target: Oid,
    /// Si es la rama en la que está HEAD.
    pub is_head: bool,
}

impl RefEntry {
    /// La carpeta del nombre, para agrupar como hace el árbol de ramas:
    /// `feat/login` vive bajo `feat`, y `main` en la raíz.
    pub fn folder(&self) -> Option<&str> {
        let name = match self.kind {
            // En una remota el primer segmento es el remoto, no una carpeta.
            RefKind::RemoteBranch => self.name.split_once('/').map(|(_, rest)| rest)?,
            _ => self.name.as_str(),
        };
        name.rsplit_once('/').map(|(folder, _)| folder)
    }

    /// El nombre sin la carpeta.
    pub fn leaf(&self) -> &str {
        self.name.rsplit_once('/').map_or(self.name.as_str(), |(_, leaf)| leaf)
    }
}
