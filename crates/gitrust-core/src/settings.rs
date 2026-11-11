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
//! auto-fetch 1
//! default-branch trunk
//! tab /home/someone/work/picstar
//! ```
//!
//! The ones on the preferences screen are [`General`]. The `tab` lines are
//! not a preference at all -- they are what was open when the window was
//! last closed -- but they are kept here for the reason the remote
//! checkouts are: one file that is about this computer is easier to reason
//! about, and to delete, than three.
//!
//! A line it does not understand is skipped and a setting that is not in the
//! file keeps its default, so a file written by a newer gitrust does not stop
//! an older one, and neither does a file somebody has edited by hand. Like
//! the list of known repositories, this is a convenience: one that stops the
//! window opening is not one.

use std::path::PathBuf;

use crate::ssh::{Login, RemoteDescriptor};
use anyhow::{Context, Result};

/// The appearance chosen for the application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    /// Follow macOS, Windows, or Linux's current appearance.
    #[default]
    Auto,
    /// The original dark graphite palette.
    Dark,
    /// A white and light-grey palette.
    Light,
}

impl Appearance {
    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "auto" => Some(Self::Auto),
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// The fewest commits the graph will be asked to start with. Below this the
/// history stops before most people have scrolled once.
pub const FEWEST_COMMITS: usize = 500;

/// The longest the window will wait between two fetches of its own, in
/// minutes. An hour: past that it is not keeping anything up to date.
pub const LONGEST_FETCH_WAIT: u32 = 60;

/// What the preferences screen is a screen of.
///
/// Apart from the rest because every tab needs a copy of it and none of them
/// needs the list of remote checkouts that travels with the others.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct General {
    /// How many minutes between fetches nobody asked for. Nought is never.
    pub auto_fetch: u32,
    /// Whether a fetch also takes away the remote branches that have gone
    /// from the remote. Off is not "never": it is "as Git is configured".
    pub auto_prune: bool,
    /// Whether the submodules are brought to the commit the repository
    /// records for them, once something has moved it.
    pub submodules: bool,
    /// The branch a repository made here starts on. Empty is whatever Git
    /// itself would have called it.
    pub default_branch: String,
    /// Whether the `.orig` copy a merge tool leaves beside a file goes when
    /// the conflict in that file is settled.
    pub delete_orig: bool,
    /// Whether the graph is the whole history, however long.
    pub show_all: bool,
    /// How many commits the graph starts with when it is not the whole.
    pub initial_commits: usize,
    /// Whether reaching the bottom of the graph brings more of it.
    pub lazy_load: bool,
    /// Whether the tabs that were open come back the next time.
    pub remember_tabs: bool,
    /// Whether the activity log also lists what the window did by itself.
    pub extended_log: bool,
    /// Whether the branch is checked, ahead of time, for conflicts with the
    /// branch it will be merged into.
    pub foresee_conflicts: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            auto_fetch: 1,
            auto_prune: true,
            submodules: true,
            default_branch: String::new(),
            delete_orig: false,
            show_all: false,
            // What the graph has always read, from before there was anywhere
            // to say otherwise.
            initial_commits: 5_000,
            lazy_load: true,
            remember_tabs: true,
            extended_log: false,
            foresee_conflicts: true,
        }
    }
}

impl General {
    /// How many commits to ask the repository for to begin with.
    pub fn commits(&self) -> usize {
        match self.show_all {
            true => usize::MAX,
            false => self.initial_commits.max(FEWEST_COMMITS),
        }
    }

    /// Takes one line of the file, if it is one of these.
    fn take(&mut self, name: &str, value: &str) -> bool {
        let value = value.trim();
        let on = value == "on";
        match name {
            "auto-fetch" => {
                if let Ok(minutes) = value.parse::<u32>() {
                    self.auto_fetch = minutes.min(LONGEST_FETCH_WAIT);
                }
            }
            "auto-prune" => self.auto_prune = on,
            "submodules" => self.submodules = on,
            // A name with a space in it is not a branch, and a line that
            // says so was not written by this program.
            "default-branch" => {
                if !value.contains(char::is_whitespace) {
                    self.default_branch = value.to_string();
                }
            }
            "delete-orig" => self.delete_orig = on,
            "show-all-commits" => self.show_all = on,
            "initial-commits" => {
                if let Ok(count) = value.parse::<usize>() {
                    self.initial_commits = count.max(FEWEST_COMMITS);
                }
            }
            "lazy-load" => self.lazy_load = on,
            "remember-tabs" => self.remember_tabs = on,
            "extended-log" => self.extended_log = on,
            "foresee-conflicts" => self.foresee_conflicts = on,
            _ => return false,
        }
        true
    }

    fn render(&self) -> String {
        let word = |on: bool| if on { "on" } else { "off" };
        let mut text = format!(
            "auto-fetch {}\nauto-prune {}\nsubmodules {}\n",
            self.auto_fetch,
            word(self.auto_prune),
            word(self.submodules),
        );
        // Left out when empty rather than written as a name with nothing
        // after it: a line this file's own reader would skip is not one to
        // write.
        if !self.default_branch.is_empty() {
            text.push_str(&format!("default-branch {}\n", self.default_branch));
        }
        text.push_str(&format!(
            "delete-orig {}\nshow-all-commits {}\ninitial-commits {}\nlazy-load {}\n\
             remember-tabs {}\nextended-log {}\nforesee-conflicts {}\n",
            word(self.delete_orig),
            word(self.show_all),
            self.initial_commits,
            word(self.lazy_load),
            word(self.remember_tabs),
            word(self.extended_log),
            word(self.foresee_conflicts),
        ));
        text
    }
}

/// Everything that is remembered between runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Whether the graph answers the pointer with a wave.
    ///
    /// On by default: it is the one thing in the window that moves, and
    /// somebody who has never seen it cannot ask for it. Whoever does not
    /// want it turns it off once and it stays off.
    pub waves: bool,
    /// The application's light, dark, or system appearance.
    pub appearance: Appearance,
    /// Remote checkouts previously selected by the user.  This is strictly
    /// connection metadata; authentication remains with SSH agent/password.
    pub remotes: Vec<RemoteDescriptor>,
    /// What the preferences screen changes.
    pub general: General,
    /// What was open when the window was last looked at, in the order of
    /// the strip: a folder, or the `ssh://` address of a remote checkout.
    pub tabs: Vec<String>,
    /// Which of them was in front.
    pub front: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            waves: true,
            appearance: Appearance::Auto,
            remotes: Vec::new(),
            general: General::default(),
            tabs: Vec::new(),
            front: 0,
        }
    }
}

/// Where the file lives, whether or not it is there yet.
pub fn file() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(
            std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?,
        )
        .join(".config"),
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
                Some(("appearance", value)) => {
                    if let Some(appearance) = Appearance::parse(value) {
                        settings.appearance = appearance;
                    }
                }
                Some(("remote", value)) => {
                    let fields: Vec<_> = value.split('\t').collect();
                    if let [host, user, port, path] = fields.as_slice() {
                        if let Ok(port) = port.parse() {
                            if let Ok(login) =
                                Login::new((*host).to_string(), (*user).to_string(), port)
                            {
                                if let Ok(remote) =
                                    RemoteDescriptor::new(login, (*path).to_string())
                                {
                                    settings.remotes.push(remote);
                                }
                            }
                        }
                    }
                }
                // The rest of the line and not a word of it: a folder can
                // have a space in its name.
                Some(("tab", value)) if !value.trim().is_empty() => {
                    settings.tabs.push(value.trim().to_string());
                }
                Some(("front", value)) => {
                    settings.front = value.trim().parse().unwrap_or(0);
                }
                Some((name, value)) => {
                    settings.general.take(name, value);
                }
                None => continue,
            }
        }
        // A finger on a tab that is not there points at the first one.
        if settings.front >= settings.tabs.len() {
            settings.front = 0;
        }
        settings
    }

    fn render(&self) -> String {
        let mut text = format!("waves {}\nappearance {}\n", match self.waves {
            true => "on",
            false => "off",
        }, self.appearance.as_str());
        for remote in &self.remotes {
            text.push_str(&format!(
                "remote {}\t{}\t{}\t{}\n",
                remote.login.host, remote.login.user, remote.login.port, remote.path
            ));
        }
        text.push_str(&self.general.render());
        for tab in &self.tabs {
            text.push_str(&format!("tab {tab}\n"));
        }
        if !self.tabs.is_empty() {
            text.push_str(&format!("front {}\n", self.front));
        }
        text
    }

    /// The remote checkout a remembered tab was, when it was one and is
    /// still on the list.
    pub fn remote_at(&self, place: &str) -> Option<&RemoteDescriptor> {
        self.remotes.iter().find(|remote| remote.place() == place)
    }

    /// Notes what is open. Says whether that changed anything, so the caller
    /// writes the file when it did and not sixty times a second.
    pub fn note_tabs(&mut self, tabs: Vec<String>, front: usize) -> bool {
        if self.tabs == tabs && self.front == front {
            return false;
        }
        self.tabs = tabs;
        self.front = front;
        true
    }

    /// Keeps a checkout descriptor once, replacing an older identical entry.
    pub fn remember_remote(&mut self, remote: RemoteDescriptor) {
        self.remotes.retain(|known| known != &remote);
        self.remotes.insert(0, remote);
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
        for (waves, appearance) in [(true, Appearance::Auto), (false, Appearance::Dark), (true, Appearance::Light)] {
            let settings = Settings { waves, appearance, ..Settings::default() };
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

    #[test]
    fn every_preference_survives_the_trip_to_the_file_and_back() {
        let general = General {
            auto_fetch: 15,
            auto_prune: false,
            submodules: false,
            default_branch: "trunk".to_string(),
            delete_orig: true,
            show_all: true,
            initial_commits: 800,
            lazy_load: false,
            remember_tabs: false,
            extended_log: true,
            foresee_conflicts: false,
        };
        let settings = Settings { general, ..Settings::default() };
        assert_eq!(Settings::parse(&settings.render()), settings);
        // And the defaults, which is the case with no branch name to write.
        assert_eq!(Settings::parse(&Settings::default().render()), Settings::default());
    }

    #[test]
    fn a_preference_out_of_range_is_brought_back_inside_it() {
        let settings = Settings::parse("auto-fetch 600\ninitial-commits 3\n");
        assert_eq!(settings.general.auto_fetch, LONGEST_FETCH_WAIT);
        assert_eq!(settings.general.initial_commits, FEWEST_COMMITS);
        // One that is not a number at all leaves what was there.
        let settings = Settings::parse("auto-fetch often\ninitial-commits lots\n");
        assert_eq!(settings.general, General::default());
    }

    #[test]
    fn a_branch_name_with_a_space_in_it_is_not_taken() {
        let settings = Settings::parse("default-branch my branch\n");
        assert_eq!(settings.general.default_branch, "");
    }

    #[test]
    fn the_whole_history_is_asked_for_only_when_it_is_wanted() {
        let mut general = General::default();
        assert_eq!(general.commits(), 5_000);
        general.show_all = true;
        assert_eq!(general.commits(), usize::MAX);
    }

    #[test]
    fn the_open_tabs_come_back_in_order_with_the_one_in_front() {
        let mut settings = Settings::default();
        let tabs = vec![
            "/home/someone/my work/picstar".to_string(),
            "ssh://dev@host.example:22/srv/project".to_string(),
        ];
        assert!(settings.note_tabs(tabs.clone(), 1));
        // Saying it again changes nothing, which is what keeps the file
        // from being written on every frame.
        assert!(!settings.note_tabs(tabs.clone(), 1));

        let back = Settings::parse(&settings.render());
        assert_eq!(back.tabs, tabs);
        assert_eq!(back.front, 1);
    }

    #[test]
    fn a_finger_on_a_tab_that_is_not_there_goes_to_the_first() {
        assert_eq!(Settings::parse("tab /a\nfront 7\n").front, 0);
        assert_eq!(Settings::parse("front 2\n").front, 0);
    }

    #[test]
    fn a_remembered_tab_finds_the_remote_checkout_it_was() {
        let remote = RemoteDescriptor::new(
            Login::new("host.example".to_string(), "dev".to_string(), 2222).unwrap(),
            "/srv/project".to_string(),
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.remember_remote(remote.clone());
        assert_eq!(settings.remote_at(&remote.place()), Some(&remote));
        assert_eq!(settings.remote_at("/home/someone/picstar"), None);
    }

    #[test]
    fn a_remote_checkout_is_remembered_without_a_password() {
        let remote = RemoteDescriptor::new(
            Login::new("host.example".to_string(), "dev".to_string(), 2222).unwrap(),
            "/srv/project".to_string(),
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.remember_remote(remote.clone());
        let text = settings.render();
        assert!(text.contains("remote host.example\tdev\t2222\t/srv/project"));
        assert!(!text.contains("password"));
        assert_eq!(Settings::parse(&text).remotes, vec![remote]);
    }
}
