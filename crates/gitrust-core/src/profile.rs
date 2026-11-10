//! Who you are working as: the name on a commit, the key that opens the
//! remote, and the account the forge answers to.
//!
//! The third thing kept outside a repository, and for the same reason as the
//! other two: a key on this disk and a token for this person are about this
//! machine, and they mean nothing in a clone somebody else pulls.
//!
//! # What it is for
//!
//! One person is often several people. The same laptop writes commits as an
//! employee in the morning and as a contractor in the afternoon, against two
//! different hosts, with two different keys and two different accounts, and
//! the usual way of living with that is a pile of `includeIf` blocks in
//! `.gitconfig` and an `~/.ssh/config` nobody dares touch. A profile is that
//! pile named and switched in one click.
//!
//! Nothing here is required. With no profiles, gitrust behaves exactly as it
//! did before there were any: the identity comes from Git's own settings, the
//! key from the SSH agent, and the forge from whatever `gh` is logged in as.
//! A profile replaces those three answers one at a time -- a profile that
//! names a key and nothing else changes only the key.
//!
//! # Where
//!
//! `$XDG_CONFIG_HOME/gitrust/profiles`, or `$HOME/.config/gitrust/profiles`,
//! beside the list of known repositories and the settings, and found the same
//! way.
//!
//! # What
//!
//! A line saying which one is in use, and then a block per profile:
//!
//! ```text
//! using Acme
//!
//! profile Acme
//!   author Someone
//!   email someone@acme.example
//!   key ~/.ssh/acme_ed25519
//!   github github.com someone-at-acme ghp_xxxxxxxxxxxx
//!
//! profile Personal
//!   author Someone
//!   email someone@example.invalid
//!   gitlab gitlab.com someone
//! ```
//!
//! A line it does not understand is skipped and a field that is not there
//! keeps its default, like the settings file beside it: a file written by a
//! newer gitrust does not stop an older one, and neither does one somebody
//! has edited by hand. A name can have spaces in it; the fields under it
//! cannot, which is true of every key path, host and token anybody has.
//!
//! # The tokens, said out loud
//!
//! **A token in here is on the disk in plain text.** The file is written
//! `rw-------` so that nobody else on the machine can read it, and that is
//! the whole of the protection: there is no keyring, no passphrase, and no
//! encryption that would only move the problem to where the key to it is
//! kept. It is the same bargain `~/.netrc` and `gh`'s own `hosts.yml` make,
//! and it is written here so that nobody finds it out by accident.
//!
//! Whoever is not content with that leaves the token empty. Then `gh` and
//! `glab` are run as they are logged in, and the password for an HTTPS remote
//! comes from the credential helper, exactly as before.
//!
//! An SSH key is a path and never the key itself. A key with a passphrase on
//! it cannot be opened from here -- there is nowhere to ask for the
//! passphrase and nowhere safe to keep it -- so that one goes in the agent
//! and the profile leaves the key empty.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Which forge an account is on.
///
/// Two, and they are the two that come with a command line tool gitrust can
/// hand the work to. A third would be a third tool, not a third name here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
}

impl Forge {
    /// What it is called in the file, and in the interface.
    pub fn name(self) -> &'static str {
        match self {
            Forge::GitHub => "github",
            Forge::GitLab => "gitlab",
        }
    }

    /// The label a person reads.
    pub fn label(self) -> &'static str {
        match self {
            Forge::GitHub => "GitHub",
            Forge::GitLab => "GitLab",
        }
    }

    /// The command that talks to it, which has to be on the PATH.
    pub fn tool(self) -> &'static str {
        match self {
            Forge::GitHub => "gh",
            Forge::GitLab => "glab",
        }
    }

    /// What the forge calls a branch offered for review. The two words mean
    /// the same thing and a sentence that uses the other one reads as though
    /// it were about somebody else's repository.
    pub fn request(self) -> &'static str {
        match self {
            Forge::GitHub => "pull request",
            Forge::GitLab => "merge request",
        }
    }

    /// Where it lives when nobody says otherwise. A self-hosted one says
    /// otherwise.
    pub fn home(self) -> &'static str {
        match self {
            Forge::GitHub => "github.com",
            Forge::GitLab => "gitlab.com",
        }
    }
}

/// An account on a forge: which one, where it is, and what opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub forge: Forge,
    /// The host the remotes of this account are on: `github.com`, or
    /// `git.acme.example` for a server somebody runs themselves. It is what
    /// a remote's address is matched against, so it is the whole of what
    /// decides which account a repository belongs to.
    pub host: String,
    /// The name of the account on that host. It is what an HTTPS remote is
    /// asked for as the user, with the token as the password.
    pub user: String,
    /// A personal access token, or nothing at all. Nothing means the forge's
    /// own tool is left to use whatever it is logged in as.
    pub token: String,
}

impl Account {
    /// A new one on a forge's own host, with nothing filled in yet.
    pub fn new(forge: Forge) -> Self {
        Self {
            forge,
            host: forge.home().to_string(),
            user: String::new(),
            token: String::new(),
        }
    }

    fn render(&self) -> String {
        let mut line = format!("  {} {} {}", self.forge.name(), self.host, self.user);
        if !self.token.is_empty() {
            line.push(' ');
            line.push_str(&self.token);
        }
        line.push('\n');
        line
    }

    /// `github host user token`, with the token optional.
    fn parse(forge: Forge, rest: &str) -> Option<Self> {
        let mut fields = rest.split_whitespace();
        let host = fields.next()?.to_string();
        let user = fields.next().unwrap_or("").to_string();
        let token = fields.next().unwrap_or("").to_string();
        if host.is_empty() {
            return None;
        }
        Some(Account { forge, host, user, token })
    }
}

/// One identity: a name to commit under, a key to reach the remote with, and
/// the forge accounts that go with it.
///
/// Every part of it is optional except the name, and an empty part means
/// "leave it as it was": the identity falls back to Git's settings, the key
/// to the agent, the account to whatever the forge's tool is logged in as.
/// That is what lets a profile be made for one thing only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// What it is called. It is also how the file says which one is in use,
    /// so two profiles with the same name are one profile as far as that
    /// line can tell; [`Profiles::add`] keeps them apart.
    pub name: String,
    /// The name on the commits written while it is in use.
    pub author: String,
    pub email: String,
    /// The private SSH key, written as it is typed -- a leading `~` and all.
    /// Empty means the agent, which is where a key with a passphrase lives.
    pub key: String,
    pub accounts: Vec<Account>,
}

impl Profile {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Self::default()
        }
    }

    /// Who commits are written as, when the profile says.
    ///
    /// Both halves or neither: a commit with a name and no address, or an
    /// address and no name, is one Git itself refuses to write, and finding
    /// that out at the moment of committing is finding it out too late.
    pub fn who(&self) -> Option<(&str, &str)> {
        let author = self.author.trim();
        let email = self.email.trim();
        match !author.is_empty() && !email.is_empty() {
            true => Some((author, email)),
            false => None,
        }
    }

    /// The key to open remotes with, as a path on this disk.
    ///
    /// A leading `~` is expanded here rather than left to libgit2, which does
    /// not expand it and would go looking for a folder called `~`.
    pub fn key_file(&self) -> Option<PathBuf> {
        let key = self.key.trim();
        if key.is_empty() {
            return None;
        }
        let Some(rest) = key.strip_prefix("~/") else {
            return Some(PathBuf::from(key));
        };
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join(rest))
    }

    /// The account that owns a host, if this profile has one for it.
    ///
    /// Hosts are compared with no regard for case, because `GitHub.com` and
    /// `github.com` are the same machine and somebody will type both.
    pub fn account_on(&self, host: &str) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|account| account.host.eq_ignore_ascii_case(host))
    }

    /// The account that owns the host in a remote's address.
    pub fn account_for(&self, url: &str) -> Option<&Account> {
        self.account_on(&host_of(url)?)
    }

    fn render(&self) -> String {
        let mut text = format!("profile {}\n", self.name.trim());
        for (field, value) in [
            ("author", &self.author),
            ("email", &self.email),
            ("key", &self.key),
        ] {
            let value = value.trim();
            if !value.is_empty() {
                text.push_str(&format!("  {field} {value}\n"));
            }
        }
        for account in &self.accounts {
            text.push_str(&account.render());
        }
        text
    }
}

/// The host part of a remote's address, however the address is written.
///
/// Git has three shapes for the same thing -- `https://host/owner/name`,
/// `ssh://git@host/owner/name` and `git@host:owner/name` -- and the last of
/// them is a URL in no scheme anybody wrote down, which is why this is a few
/// lines of splitting rather than a crate. A path on this disk has no host
/// and gives nothing back, which is the right answer: a local clone belongs
/// to no account.
pub fn host_of(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.starts_with('/') || url.starts_with('.') {
        return None;
    }

    // Everything after the scheme, if there is one. `file://` is the one
    // scheme that names no host worth having.
    let rest = match url.split_once("://") {
        Some(("file", _)) => return None,
        Some((_, rest)) => rest,
        None => url,
    };

    // Whatever is after the last `@`: the user in `git@host` is not the host,
    // and a password in `https://user:pass@host` is not either.
    let rest = rest.rsplit('@').next()?;

    // And the host is what comes before the path, the port, or the colon of
    // the `git@host:owner/name` form.
    let host = rest.split(['/', ':']).next()?.trim();
    match host.is_empty() {
        true => None,
        false => Some(host.to_lowercase()),
    }
}

/// Every profile on this machine, and which one is in use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profiles {
    /// In the order they were made. The interface shows them in it, because
    /// any other order means the list moves under the pointer when a profile
    /// is renamed.
    pub list: Vec<Profile>,
    /// Which one is in use, by position. `None` is a real answer and the one
    /// everybody starts at: Git's own settings, the agent, and whatever the
    /// forge's tool is logged in as.
    at: Option<usize>,
}

/// Where the file lives, whether or not it is there yet.
pub fn file() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("gitrust").join("profiles"))
}

impl Profiles {
    /// The one in use, if any.
    pub fn active(&self) -> Option<&Profile> {
        self.at.and_then(|at| self.list.get(at))
    }

    /// Where it is in the list.
    pub fn at(&self) -> Option<usize> {
        self.at
    }

    /// Puts one in use, or none.
    ///
    /// A position that is not in the list is nobody, not a panic: the only
    /// thing that can hand one in is a file that was edited by hand.
    pub fn use_at(&mut self, which: Option<usize>) {
        self.at = which.filter(|at| *at < self.list.len());
    }

    /// Adds one, under a name no other profile has.
    ///
    /// Names are what the file says "using" with, so two of them called
    /// "Acme" would be one profile to that line and two everywhere else.
    /// Rather than refuse the name, the second one is "Acme 2".
    pub fn add(&mut self, profile: Profile) -> usize {
        let mut profile = profile;
        if profile.name.trim().is_empty() {
            profile.name = "Profile".to_string();
        }
        let wanted = profile.name.clone();
        let mut tries = 2;
        while self.list.iter().any(|kept| kept.name == profile.name) {
            profile.name = format!("{wanted} {tries}");
            tries += 1;
        }
        self.list.push(profile);
        self.list.len() - 1
    }

    /// Takes one away, and leaves the right one in use.
    ///
    /// Deleting the profile that was in use leaves none in use rather than
    /// quietly moving to its neighbour: which identity you are working under
    /// is not something to be decided by a list shuffling up.
    pub fn remove(&mut self, which: usize) {
        if which >= self.list.len() {
            return;
        }
        self.list.remove(which);
        self.at = match self.at {
            Some(at) if at == which => None,
            Some(at) if at > which => Some(at - 1),
            at => at,
        };
    }

    /// What a profiles file says.
    fn parse(text: &str) -> Self {
        let mut profiles = Profiles::default();
        let mut using: Option<String> = None;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (field, value) = match line.split_once(' ') {
                Some((field, value)) => (field, value.trim()),
                None => (line, ""),
            };

            match field {
                "using" => using = Some(value.to_string()),
                "profile" => profiles.list.push(Profile::new(value)),
                _ => {
                    // Everything else belongs to the profile above it. A
                    // field before any profile is a file somebody has cut
                    // the top off; it is skipped rather than refused.
                    let Some(profile) = profiles.list.last_mut() else {
                        continue;
                    };
                    match field {
                        "author" => profile.author = value.to_string(),
                        "email" => profile.email = value.to_string(),
                        "key" => profile.key = value.to_string(),
                        "github" | "gitlab" => {
                            let forge = match field {
                                "github" => Forge::GitHub,
                                _ => Forge::GitLab,
                            };
                            if let Some(account) = Account::parse(forge, value) {
                                profile.accounts.push(account);
                            }
                        }
                        _ => continue,
                    }
                }
            }
        }

        // The name is resolved to a position once, here, so that everything
        // afterwards can rename a profile without the line that picked it
        // going stale.
        if let Some(using) = using {
            profiles.at = profiles.list.iter().position(|profile| profile.name == using);
        }
        profiles
    }

    fn render(&self) -> String {
        let mut text = String::new();
        if let Some(active) = self.active() {
            text.push_str(&format!("using {}\n\n", active.name.trim()));
        }
        for profile in &self.list {
            text.push_str(&profile.render());
            text.push('\n');
        }
        text
    }

    /// What is in the file, or no profiles at all.
    ///
    /// An unreadable file is no profiles, not an error. Like the settings
    /// beside it, this is a convenience, and a convenience that stops the
    /// window opening is not one.
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
    /// left with the old profiles rather than half of the new ones.
    ///
    /// The permissions are set on that file before the rename and not after,
    /// so there is no instant in which a token is on the disk readable by
    /// everybody.
    pub fn save(&self) -> Result<()> {
        let file = file().context("there is no home folder to keep the profiles in")?;
        let folder = file.parent().context("the profiles have nowhere to live")?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("could not make {}", folder.display()))?;

        let beside = file.with_extension("writing");
        std::fs::write(&beside, self.render())
            .with_context(|| format!("could not write {}", beside.display()))?;
        shut(&beside)?;
        std::fs::rename(&beside, &file)
            .with_context(|| format!("could not put {} in place", file.display()))?;
        Ok(())
    }
}

/// Makes a file readable by its owner and nobody else.
///
/// On Windows there is no mode to set and this does nothing, which is worth
/// saying rather than hiding: a token in a profile on Windows is protected by
/// the permissions of the folder it is in and by nothing this program does.
fn shut(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("could not keep {} to yourself", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Profiles {
        let mut profiles = Profiles::default();
        profiles.add(Profile {
            name: "Acme Ltd".to_string(),
            author: "Someone".to_string(),
            email: "someone@acme.example".to_string(),
            key: "~/.ssh/acme_ed25519".to_string(),
            accounts: vec![
                Account {
                    forge: Forge::GitHub,
                    host: "github.com".to_string(),
                    user: "someone-at-acme".to_string(),
                    token: "ghp_token".to_string(),
                },
                Account {
                    forge: Forge::GitLab,
                    host: "git.acme.example".to_string(),
                    user: "someone".to_string(),
                    token: String::new(),
                },
            ],
        });
        profiles.add(Profile::new("Personal"));
        profiles.use_at(Some(0));
        profiles
    }

    #[test]
    fn a_profile_survives_the_trip_to_the_file_and_back() {
        let profiles = sample();
        assert_eq!(Profiles::parse(&profiles.render()), profiles);
    }

    #[test]
    fn the_one_in_use_survives_a_rename_of_the_others() {
        let mut profiles = sample();
        profiles.list[1].name = "Weekends".to_string();
        assert_eq!(profiles.active().map(|p| p.name.as_str()), Some("Acme Ltd"));
    }

    #[test]
    fn no_profiles_is_a_file_that_is_not_there() {
        let profiles = Profiles::parse("");
        assert!(profiles.list.is_empty());
        assert!(profiles.active().is_none());
    }

    #[test]
    fn a_line_that_makes_no_sense_is_skipped_rather_than_refused() {
        let profiles = Profiles::parse(
            "using Acme\nprofile Acme\n  email someone@acme.example\n  colour green\nnonsense\n",
        );
        assert_eq!(profiles.list.len(), 1);
        assert_eq!(profiles.active().unwrap().email, "someone@acme.example");
    }

    #[test]
    fn a_field_before_any_profile_belongs_to_nobody() {
        let profiles = Profiles::parse("  email orphan@example.invalid\nprofile Acme\n");
        assert_eq!(profiles.list.len(), 1);
        assert!(profiles.list[0].email.is_empty());
    }

    #[test]
    fn a_profile_that_is_no_longer_there_is_nobody_in_use() {
        let profiles = Profiles::parse("using Gone\nprofile Acme\n");
        assert!(profiles.active().is_none());
    }

    #[test]
    fn half_an_identity_is_no_identity() {
        let mut profile = Profile::new("Acme");
        assert_eq!(profile.who(), None);
        profile.author = "Someone".to_string();
        assert_eq!(profile.who(), None);
        profile.email = "someone@acme.example".to_string();
        assert_eq!(profile.who(), Some(("Someone", "someone@acme.example")));
    }

    #[test]
    fn a_second_profile_by_the_same_name_is_told_apart() {
        let mut profiles = Profiles::default();
        profiles.add(Profile::new("Acme"));
        let second = profiles.add(Profile::new("Acme"));
        assert_eq!(profiles.list[second].name, "Acme 2");
    }

    #[test]
    fn deleting_the_one_in_use_leaves_nobody_in_use() {
        let mut profiles = sample();
        profiles.remove(0);
        assert!(profiles.active().is_none());
        assert_eq!(profiles.list.len(), 1);
    }

    #[test]
    fn deleting_one_above_the_one_in_use_keeps_it_in_use() {
        let mut profiles = sample();
        profiles.use_at(Some(1));
        profiles.remove(0);
        assert_eq!(profiles.active().map(|p| p.name.as_str()), Some("Personal"));
    }

    #[test]
    fn a_remote_says_which_host_it_is_on() {
        for url in [
            "https://github.com/someone/picstar.git",
            "git@github.com:someone/picstar.git",
            "ssh://git@github.com/someone/picstar.git",
            "https://someone:token@GitHub.com/someone/picstar",
            "github.com:someone/picstar",
        ] {
            assert_eq!(host_of(url).as_deref(), Some("github.com"), "{url}");
        }
    }

    #[test]
    fn a_folder_on_this_disk_belongs_to_no_account() {
        for url in ["", "   ", "/srv/mirrors/picstar.git", "./picstar", "file:///srv/x"] {
            assert_eq!(host_of(url), None, "{url:?}");
        }
    }

    #[test]
    fn an_account_is_found_by_the_host_of_the_remote() {
        let profiles = sample();
        let profile = profiles.active().unwrap();

        let github = profile.account_for("git@github.com:acme/site.git").unwrap();
        assert_eq!(github.forge, Forge::GitHub);
        assert_eq!(github.user, "someone-at-acme");

        let gitlab = profile.account_for("https://git.acme.example/acme/site").unwrap();
        assert_eq!(gitlab.forge, Forge::GitLab);

        assert!(profile.account_for("https://bitbucket.org/acme/site").is_none());
    }

    #[test]
    fn a_key_path_written_with_a_tilde_points_at_the_home_folder() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        let profile = Profile {
            key: "~/.ssh/acme_ed25519".to_string(),
            ..Profile::new("Acme")
        };
        assert_eq!(
            profile.key_file(),
            Some(PathBuf::from(&home).join(".ssh/acme_ed25519"))
        );
    }

    #[test]
    fn no_key_is_the_agent_and_not_a_path() {
        assert_eq!(Profile::new("Acme").key_file(), None);
    }
}
