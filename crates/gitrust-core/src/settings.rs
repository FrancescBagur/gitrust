//! What somebody has told gitrust they prefer.
//!
//! The second thing kept outside a repository, and for the same reason the
//! first one is: a choice about how this window behaves is about this
//! computer, and it means nothing in a clone somebody else pulls.
//!
//! # Where
//!
//! `$XDG_CONFIG_HOME/gitrust/settings`, or `$HOME/.config/gitrust/settings`,
//! beside the list of known repositories and found the same way.
//!
//! # What
//!
//! One setting per line, its name and then its value:
//!
//! ```text
//! waves on
//! ```
//!
//! A line it does not understand is skipped and a setting that is not in the
//! file keeps its default, so a file written by a newer gitrust does not stop
//! an older one, and neither does a file somebody has edited by hand. Like
//! the list of known repositories, this is a convenience: one that stops the
//! window opening is not one.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Everything that is remembered between runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Whether the graph answers the pointer with a wave.
    ///
    /// On by default: it is the one thing in the window that moves, and
    /// somebody who has never seen it cannot ask for it. Whoever does not
    /// want it turns it off once and it stays off.
    pub waves: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { waves: true }
    }
}

/// Where the file lives, whether or not it is there yet.
pub fn file() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("gitrust").join("settings"))
}

impl Settings {
    /// What a settings file says, with the defaults for whatever it leaves
    /// out or garbles.
    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            match line.split_once(' ') {
                Some(("waves", value)) => settings.waves = value.trim() == "on",
                _ => continue,
            }
        }
        settings
    }

    fn render(&self) -> String {
        format!("waves {}\n", match self.waves {
            true => "on",
            false => "off",
        })
    }

    /// What is in the file, with the defaults for whatever is not.
    ///
    /// An unreadable file is the defaults, not an error.
    pub fn load() -> Self {
        let text = file()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .unwrap_or_default();
        Self::parse(&text)
    }

    /// Writes them back, whole.
    ///
    /// Through a file beside it and a rename, for the reason the list of
    /// known repositories does it: a machine that goes down mid-write is
    /// left with the old settings rather than half of the new ones.
    pub fn save(&self) -> Result<()> {
        let file = file().context("there is no home folder to keep the settings in")?;
        let folder = file.parent().context("the settings have nowhere to live")?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not make {}", folder.display()))?;

        let beside = file.with_extension("writing");
        std::fs::write(&beside, self.render())
            .with_context(|| format!("could not write {}", beside.display()))?;
        std::fs::rename(&beside, &file)
            .with_context(|| format!("could not put {} in place", file.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_survives_the_trip_to_the_file_and_back() {
        for waves in [true, false] {
            let settings = Settings { waves };
            assert_eq!(Settings::parse(&settings.render()), settings);
        }
    }

    #[test]
    fn a_setting_the_file_does_not_mention_keeps_its_default() {
        assert_eq!(Settings::parse("colour green\n"), Settings::default());
        assert_eq!(Settings::parse(""), Settings::default());
    }

    #[test]
    fn a_line_that_makes_no_sense_is_skipped_rather_than_refused() {
        // The one line it does understand still counts, which is the whole
        // point of skipping rather than giving up on the file.
        let settings = Settings::parse("waves off\nthis line is nonsense\n");
        assert!(!settings.waves);
    }
}
