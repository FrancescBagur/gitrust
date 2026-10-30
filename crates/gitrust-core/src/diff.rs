//! Representación del diff de un fichero, ya troceado en hunks.

/// Una línea del diff. `origin` es la marca de libgit2: `+`, `-` o espacio.
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub origin: char,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

/// Un bloque contiguo de cambios, con su cabecera `@@ ... @@`.
#[derive(Debug, Clone)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Default)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
}

impl FileDiff {
    /// Cuántas líneas se añaden y cuántas se quitan.
    pub fn counts(&self) -> (usize, usize) {
        let mut added = 0;
        let mut removed = 0;
        for hunk in &self.hunks {
            for line in &hunk.lines {
                match line.origin {
                    '+' => added += 1,
                    '-' => removed += 1,
                    _ => {}
                }
            }
        }
        (added, removed)
    }
}
