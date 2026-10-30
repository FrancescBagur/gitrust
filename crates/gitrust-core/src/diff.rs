//! Representación del diff de un fichero, ya troceado en hunks.

/// Una línea del diff. `origin` es la marca de libgit2: `+`, `-` o espacio.
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub origin: char,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

/// Identifica un hunk por los rangos de su cabecera `@@ -a,b +c,d @@`.
///
/// Hace falta porque al preparar un hunk suelto hay que reconocerlo dentro de
/// un diff que libgit2 vuelve a calcular: contar posiciones no vale, ya que el
/// conjunto de hunks cambia en cuanto se aplica uno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HunkId {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

impl HunkId {
    /// Los mismos rangos vistos desde un diff invertido.
    ///
    /// Quitar un hunk del índice aplica el diff al revés, y en ese diff los
    /// lados viejo y nuevo están intercambiados.
    pub fn reversed(self) -> Self {
        Self {
            old_start: self.new_start,
            old_lines: self.new_lines,
            new_start: self.old_start,
            new_lines: self.old_lines,
        }
    }
}

/// Un bloque contiguo de cambios, con su cabecera `@@ ... @@`.
#[derive(Debug, Clone)]
pub struct Hunk {
    pub header: String,
    pub id: HunkId,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// Cuántas líneas añade y cuántas quita este hunk.
    pub fn counts(&self) -> (usize, usize) {
        let added = self.lines.iter().filter(|line| line.origin == '+').count();
        let removed = self.lines.iter().filter(|line| line.origin == '-').count();
        (added, removed)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
}

impl FileDiff {
    /// Cuántas líneas se añaden y cuántas se quitan en todo el fichero.
    pub fn counts(&self) -> (usize, usize) {
        self.hunks.iter().fold((0, 0), |(added, removed), hunk| {
            let (a, r) = hunk.counts();
            (added + a, removed + r)
        })
    }
}
