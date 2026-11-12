//! Where this person's files are.
//!
//! One answer, asked in one place. It used to be asked by each file that
//! wanted it, and they did not all ask the same way: the settings looked for
//! `HOME` and then for `USERPROFILE`, the list of known repositories and the
//! profiles looked for `HOME` and stopped. A Windows program started from
//! the desktop has no `HOME`, so on Windows the first was written and the
//! other two never were -- and nothing said so, because a file with nowhere
//! to go is not an error anywhere in this program, on purpose.

use std::ffi::OsString;
use std::path::PathBuf;

/// The home folder: `HOME`, or `USERPROFILE` where there is none.
///
/// `HOME` goes first on every system. On Windows it is only there when
/// something put it there -- Git Bash does -- and whoever set it meant it.
pub fn folder() -> Option<PathBuf> {
    first(std::env::var_os("HOME"), std::env::var_os("USERPROFILE"))
}

/// The folder gitrust keeps its own files in.
///
/// `$XDG_CONFIG_HOME/gitrust`, or `.config/gitrust` under the home folder --
/// on Windows too, where `%APPDATA%` would be the custom. The settings were
/// already being written under `.config` there, and three files in one
/// folder beat two folders with one of them in the proper place.
pub fn config() -> Option<PathBuf> {
    config_under(std::env::var_os("XDG_CONFIG_HOME"), folder())
}

/// A variable that is set to nothing says nothing: a shell that exports
/// `HOME=` has not named the root of the disk.
fn first(home: Option<OsString>, profile: Option<OsString>) -> Option<PathBuf> {
    [home, profile]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn config_under(xdg: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    let base = match xdg {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home?.join(".config"),
    };
    Some(base.join("gitrust"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    // The variables are handed in rather than set: the environment belongs
    // to the whole test process, and a test that changed HOME would change
    // it under every other test running beside it.

    #[test]
    fn a_machine_with_no_home_variable_still_has_a_home_folder() {
        assert_eq!(first(None, said("C:\\Users\\ana")), Some(PathBuf::from("C:\\Users\\ana")));
        assert_eq!(first(said(""), said("C:\\Users\\ana")), Some(PathBuf::from("C:\\Users\\ana")));
        assert_eq!(first(said("/home/ana"), said("C:\\Users\\ana")), Some(PathBuf::from("/home/ana")));
        assert_eq!(first(None, None), None);
        assert_eq!(first(said(""), said("")), None);
    }

    #[test]
    fn the_files_go_under_the_home_folder_unless_somewhere_else_was_named() {
        let home = Some(PathBuf::from("/home/ana"));
        assert_eq!(
            config_under(None, home.clone()),
            Some(PathBuf::from("/home/ana").join(".config").join("gitrust"))
        );
        assert_eq!(config_under(said(""), home.clone()), config_under(None, home.clone()));
        assert_eq!(config_under(said("/etc/xdg"), home), Some(PathBuf::from("/etc/xdg").join("gitrust")));
        // Named outright, it needs no home folder to hang from.
        assert_eq!(config_under(said("/etc/xdg"), None), Some(PathBuf::from("/etc/xdg").join("gitrust")));
        assert_eq!(config_under(None, None), None);
    }
}
