//! The scaffolding the tests share: a real repository in a temporary
//! directory, which deletes itself when each test ends.
//!
//! Every test binary compiles this whole module, so what one uses is surplus
//! to the other. It is not dead code: it is the other test's code.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gitrust_core::Repo;

/// Makes an empty repository in a temporary directory of this test's own.
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

/// A plain temporary directory, for what is not a working tree.
pub fn temp_dir(tag: &str) -> PathBuf {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("gitrust-{tag}-{stamp}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}
