//! El andamio que comparten las pruebas: un repositorio de verdad en un
//! directorio temporal, que se borra solo al terminar cada prueba.
//!
//! Cada binario de pruebas compila este módulo entero, así que lo que use
//! uno le sobra al otro. No es código muerto: es código de la otra prueba.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gitrust_core::Repo;

/// Crea un repositorio vacío en un directorio temporal propio de esta prueba.
pub struct TempRepo {
    dir: PathBuf,
}

impl TempRepo {
    pub fn new(tag: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("gitrust-{tag}-{stamp}"));
        fs::create_dir_all(&dir).unwrap();

        let repo = git2::Repository::init(&dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "gitrust test").unwrap();
        config.set_str("user.email", "test@example.invalid").unwrap();

        Self { dir }
    }

    pub fn write(&self, name: &str, contents: &str) {
        fs::write(self.dir.join(name), contents).unwrap();
    }

    pub fn open(&self) -> Repo {
        Repo::open(&self.dir).unwrap()
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Un directorio temporal a secas, para lo que no es un árbol de trabajo.
pub fn temp_dir(tag: &str) -> PathBuf {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("gitrust-{tag}-{stamp}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}
