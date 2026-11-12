//! SSH/SFTP access to a checkout that stays on another machine.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ssh2::{CheckResult, HashType, KnownHostFileKind, OpenFlags, OpenType, Session as RawSession};

/// An ordinary `Host` entry in the user's SSH configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
}

/// Connection details. The password is intentionally not represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    pub host: String,
    pub user: String,
    pub port: u16,
}

/// The non-secret part of a remote checkout.  Passwords and private-key
/// passphrases deliberately have no place in this type or in settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDescriptor {
    pub login: Login,
    pub path: String,
    /// The private key this checkout was opened with, when one was named
    /// for it: a path, as it was typed, and never the key itself.
    ///
    /// It is what a server hands out as a `.pem` and expects back on every
    /// connection. Kept beside the address so that reconnecting, and the
    /// terminal, knock with the same key the first connection did -- a
    /// server reached with a named key is very often one the agent has
    /// never heard of. `None` is the profile's key and then the agent.
    pub key: Option<String>,
}

impl RemoteDescriptor {
    pub fn new(login: Login, path: String) -> Result<Self> {
        anyhow::ensure!(
            path.starts_with('/'),
            "the repository path must be absolute"
        );
        Ok(Self { login, path, key: None })
    }

    /// The same checkout, reached with that key. A blank is no key.
    pub fn keyed(mut self, key: Option<&str>) -> Self {
        self.key = key
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_string);
        self
    }

    /// Where that key is on this disk, with a leading `~/` undone.
    pub fn key_file(&self) -> Option<PathBuf> {
        key_file(self.key.as_deref()?)
    }

    /// Where this checkout is, as one line: `ssh://user@host:port/path`.
    ///
    /// What a profile files a remote checkout under, the way it files a
    /// local one under its folder. The port is always written, 22 included,
    /// so that the same checkout is the same line however it was typed.
    pub fn place(&self) -> String {
        format!(
            "ssh://{}@{}:{}{}",
            self.login.user, self.login.host, self.login.port, self.path
        )
    }
}

impl Login {
    pub fn new(host: String, user: String, port: u16) -> Result<Self> {
        anyhow::ensure!(!host.trim().is_empty(), "the SSH host is missing");
        anyhow::ensure!(!user.trim().is_empty(), "the SSH user is missing");
        Ok(Self { host, user, port })
    }
}

/// A key path as somebody writes one, made into a path on this disk: a
/// leading `~/` is the home folder, as it is for a profile's key.
pub fn key_file(written: &str) -> Option<PathBuf> {
    let written = written.trim();
    if written.is_empty() {
        return None;
    }
    match written.strip_prefix("~/") {
        Some(rest) => Some(crate::home::folder()?.join(rest)),
        None => Some(PathBuf::from(written)),
    }
}

/// What a connection proves itself with.
enum How<'a> {
    Password(&'a str),
    /// A private key file. `then_agent` is whether the agent is asked when
    /// the key is refused.
    Key {
        file: &'a Path,
        passphrase: Option<&'a str>,
        then_agent: bool,
    },
    Agent,
}

/// An open, authenticated connection. It holds its TCP socket until dropped.
pub struct Session {
    login: Login,
    raw: RawSession,
}

impl Session {
    /// Connect using the supplied password, or the SSH agent when it is blank.
    /// The caller owns the password and can clear it as soon as this returns.
    pub fn connect(login: Login, password: Option<&str>, trust_new_host: bool) -> Result<Self> {
        let how = match password.filter(|password| !password.is_empty()) {
            Some(password) => How::Password(password),
            None => How::Agent,
        };
        Self::connect_inner(login, how, trust_new_host, None)
    }

    /// Connect the way a profile would: with its key first, and the SSH
    /// agent after it.
    ///
    /// A password that was typed still wins, because somebody typed it. With
    /// no key this is `connect` and nothing else. With one, the agent is
    /// still asked when the key is refused: a profile's key is the first
    /// answer and not the only one, exactly as it is for a fetch or a push,
    /// and a server that does not know this client's key is very often one
    /// that knows the key already in the agent.
    pub fn connect_as(login: Login, password: Option<&str>, key: Option<&Path>, trust_new_host: bool) -> Result<Self> {
        let how = match (password.filter(|password| !password.is_empty()), key) {
            (Some(password), _) => How::Password(password),
            (None, Some(file)) => How::Key { file, passphrase: None, then_agent: true },
            (None, None) => How::Agent,
        };
        Self::connect_inner(login, how, trust_new_host, None)
    }

    /// Connect with the key somebody named for this connection: the `.pem`
    /// a server handed out, or any other private key file.
    ///
    /// That key and nothing after it. A profile's key falls back to the
    /// agent because nobody chose it for this server; this one was chosen,
    /// and a connection that quietly got in some other way would leave a
    /// key written down beside the checkout that does not open it.
    ///
    /// The passphrase is for a key that has one, and like a password it is
    /// the caller's to clear: it is used once, here, and kept nowhere.
    pub fn connect_keyed(login: Login, key: &Path, passphrase: Option<&str>, trust_new_host: bool) -> Result<Self> {
        let passphrase = passphrase.filter(|passphrase| !passphrase.is_empty());
        Self::connect_inner(login, How::Key { file: key, passphrase, then_agent: false }, trust_new_host, None)
    }

    /// Connect with an explicit private key and known-hosts file. This is
    /// useful for isolated transport tests without borrowing the user's SSH
    /// agent or their `known_hosts` file.
    pub fn connect_with_key(login: Login, private_key: &Path, passphrase: Option<&str>, known_hosts: &Path, trust_new_host: bool) -> Result<Self> {
        Self::connect_inner(login, How::Key { file: private_key, passphrase, then_agent: false }, trust_new_host, Some(known_hosts))
    }

    fn connect_inner(login: Login, how: How, trust_new_host: bool, known_hosts: Option<&Path>) -> Result<Self> {
        // Said before anything is asked of the network: libssh2 answers a
        // file that is not there with a line about the format of keys, and
        // only once the server has been reached. Only for a key that was
        // named -- a profile's has the agent after it, and that is the
        // answer to its being gone.
        if let How::Key { file, then_agent: false, .. } = &how {
            anyhow::ensure!(file.is_file(), "there is no key at {}", file.display());
        }
        let tcp = TcpStream::connect((login.host.as_str(), login.port))
            .with_context(|| format!("could not reach {}:{}", login.host, login.port))?;
        tcp.set_read_timeout(Some(Duration::from_secs(15)))
            .context("could not set the SSH read timeout")?;
        tcp.set_write_timeout(Some(Duration::from_secs(15)))
            .context("could not set the SSH write timeout")?;
        let mut raw = RawSession::new().context("could not prepare SSH")?;
        raw.set_tcp_stream(tcp);
        raw.handshake().context("the SSH handshake failed")?;
        verify_host(&raw, &login, trust_new_host, known_hosts)?;

        match how {
            How::Password(password) => raw
                .userauth_password(&login.user, password)
                .context("the SSH password was refused")?,
            How::Key { file, passphrase, then_agent } => {
                let keyed = raw.userauth_pubkey_file(&login.user, None, file, passphrase);
                match (keyed, then_agent) {
                    (Ok(()), _) => {}
                    (Err(err), false) => {
                        return Err(err).with_context(|| match passphrase {
                            Some(_) => format!("{} was refused, or that is not its passphrase", file.display()),
                            None => format!("{} was refused, or it has a passphrase and none was given", file.display()),
                        })
                    }
                    // Both are named when both fail: "the agent refused"
                    // alone would send somebody to look at the agent, when
                    // the thing they set up and expected to work was the key.
                    (Err(key), true) => raw.userauth_agent(&login.user).with_context(|| {
                        format!(
                            "{} was refused ({key}), and so was the SSH agent",
                            file.display()
                        )
                    })?,
                }
            }
            How::Agent => raw
                .userauth_agent(&login.user)
                .context("could not authenticate with the SSH agent")?,
        }
        anyhow::ensure!(
            raw.authenticated(),
            "the SSH server did not authenticate you"
        );
        Ok(Self { login, raw })
    }

    /// Lists immediate folders over SFTP. Returned paths are absolute server
    /// paths, ready to be passed back into this method to descend.
    pub fn folders(&self, path: &str) -> Result<Vec<String>> {
        anyhow::ensure!(path.starts_with('/'), "remote folders must be absolute");
        let sftp = self.raw.sftp().context("could not start SFTP")?;
        let mut folders: Vec<String> = sftp
            .readdir(Path::new(path))
            .with_context(|| format!("could not read {path}"))?
            .into_iter()
            .filter(|(_, stat)| stat.is_dir())
            .map(|(path, _)| path.display().to_string())
            .collect();
        folders.sort();
        Ok(folders)
    }

    /// Checks that a selected folder is a non-bare Git checkout. It does not
    /// read or copy any file from that checkout.
    pub fn verify_checkout(&self, path: &str) -> Result<()> {
        anyhow::ensure!(
            path.starts_with('/'),
            "the repository path must be absolute"
        );
        let output = self.command(&["git", "-C", path, "rev-parse", "--is-inside-work-tree"])?;
        anyhow::ensure!(output.trim() == "true", "{path} is not a Git working tree");
        Ok(())
    }

    pub fn login(&self) -> &Login {
        &self.login
    }

    /// Reads a working-tree file over SFTP. The caller decides whether its
    /// bytes are text; SSH never creates a local copy.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let sftp = self.raw.sftp().context("could not start SFTP")?;
        let mut file = sftp.open(Path::new(path)).with_context(|| format!("could not read {path}"))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).with_context(|| format!("could not read {path}"))?;
        Ok(bytes)
    }

    /// Replaces a working-tree file over SFTP. Git index changes remain Git
    /// commands and are deliberately not hidden in this transport method.
    pub fn write_file(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let sftp = self.raw.sftp().context("could not start SFTP")?;
        let mut file = sftp.open_mode(Path::new(path), OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE, 0o644, OpenType::File)
            .with_context(|| format!("could not write {path}"))?;
        file.write_all(bytes).with_context(|| format!("could not write {path}"))?;
        Ok(())
    }

    pub fn exists(&self, path: &str) -> bool {
        self.raw.sftp().and_then(|sftp| sftp.stat(Path::new(path))).is_ok()
    }

    pub(crate) fn command(&self, words: &[&str]) -> Result<String> {
        let (status, stdout, stderr) = self.run(words)?;
        if status != 0 {
            bail!(
                "{}",
                if stderr.trim().is_empty() {
                    "the remote command failed"
                } else {
                    stderr.trim()
                }
            );
        }
        Ok(stdout)
    }

    /// Runs a command and hands back how it ended along with what it said.
    ///
    /// For the few whose exit status is an answer and not a verdict: a merge
    /// tried in memory ends with one when it found conflicts, and that is
    /// the result that was asked for.
    pub(crate) fn run(&self, words: &[&str]) -> Result<(i32, String, String)> {
        let command = words
            .iter()
            .map(|word| quote(word))
            .collect::<Vec<_>>()
            .join(" ");
        let mut channel = self
            .raw
            .channel_session()
            .context("could not open an SSH channel")?;
        channel
            .exec(&command)
            .context("could not run the command on the SSH server")?;
        let mut stdout = String::new();
        let mut stderr = String::new();
        channel.read_to_string(&mut stdout)?;
        channel.stderr().read_to_string(&mut stderr)?;
        channel.wait_close()?;
        Ok((channel.exit_status()?, stdout, stderr))
    }
}

/// Refuse unknown or changed servers. The UI will later offer an explicit
/// trust decision; silently adding a key here would make a first connection
/// vulnerable to a machine-in-the-middle attack.
fn verify_host(raw: &RawSession, login: &Login, trust_new_host: bool, known_hosts: Option<&Path>) -> Result<()> {
    let file = match known_hosts {
        Some(file) => file.to_path_buf(),
        None => {
            let Some(home) = crate::home::folder() else {
                bail!("could not find known_hosts to verify {}", login.host)
            };
            home.join(".ssh").join("known_hosts")
        }
    };
    let mut known = raw.known_hosts().context("could not read known_hosts")?;
    // An absent file is the normal first-connection case. `check` below will
    // report it as an unknown server and only the explicit UI choice may then
    // create the file.
    if file.exists() {
        known
            .read_file(&file, KnownHostFileKind::OpenSSH)
            .context("could not read known_hosts")?;
    }
    let (key, kind) = raw
        .host_key()
        .context("the SSH server did not provide a host key")?;
    let name = if login.port == 22 {
        login.host.clone()
    } else {
        format!("[{}]:{}", login.host, login.port)
    };
    match known.check(&name, key) {
        CheckResult::Match => Ok(()),
        CheckResult::Mismatch => bail!("the host key for {} has changed", name),
        CheckResult::NotFound if trust_new_host => {
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)
                    .context("could not create the SSH settings folder")?;
            }
            known
                .add(&name, key, &name, kind.into())
                .context("could not remember the new host key")?;
            known
                .write_file(&file, KnownHostFileKind::OpenSSH)
                .context("could not save known_hosts")?;
            Ok(())
        }
        CheckResult::NotFound => bail!(
            "{} is not in known_hosts (SHA-256 fingerprint: {})",
            name,
            fingerprint(raw)
        ),
        CheckResult::Failure => bail!("could not verify the host key for {}", name),
    }
}

fn fingerprint(raw: &RawSession) -> String {
    raw.host_key_hash(HashType::Sha256)
        .map(|hash| hash.iter().map(|byte| format!("{byte:02x}")).collect())
        .unwrap_or_else(|| "unavailable".to_string())
}

/// Reads aliases from `~/.ssh/config`, or `%USERPROFILE%\\.ssh\\config` when
/// HOME is unavailable (as is common on native Windows).
pub fn configured_hosts() -> Result<Vec<Host>> {
    let home = crate::home::folder().context("could not find your home folder")?;
    let path = home.join(".ssh").join("config");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(parse_hosts(&text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err).with_context(|| format!("could not read {}", path.display())),
    }
}

fn parse_hosts(text: &str) -> Vec<Host> {
    let mut hosts = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let value = words.collect::<Vec<_>>().join(" ");
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                hosts.append(&mut current);
                current = value
                    .split_whitespace()
                    .filter(|alias| !alias.contains(['*', '?', '!']))
                    .map(|alias| Host {
                        alias: alias.to_string(),
                        hostname: None,
                        user: None,
                        port: None,
                    })
                    .collect();
            }
            "hostname" => current
                .iter_mut()
                .for_each(|host| host.hostname = Some(value.clone())),
            "user" => current
                .iter_mut()
                .for_each(|host| host.user = Some(value.clone())),
            "port" => {
                if let Ok(port) = value.parse() {
                    current.iter_mut().for_each(|host| host.port = Some(port))
                }
            }
            _ => {}
        }
    }
    hosts.append(&mut current);
    hosts
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_keep_their_connection_details() {
        let hosts = parse_hosts(
            "Host forge staging *.ignored\n HostName git.example\n User jane\n Port 2222\n",
        );
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].hostname.as_deref(), Some("git.example"));
        assert_eq!(hosts[1].user.as_deref(), Some("jane"));
        assert_eq!(hosts[0].port, Some(2222));
    }

    #[test]
    fn login_needs_a_host_and_user() {
        assert!(Login::new("".into(), "jane".into(), 22).is_err());
        assert!(Login::new("host".into(), "".into(), 22).is_err());
    }

    #[test]
    fn a_remote_checkout_is_one_line_with_its_port_in_it() {
        let login = Login::new("git.acme.example".to_string(), "jane".to_string(), 22).unwrap();
        let remote = RemoteDescriptor::new(login, "/srv/site".to_string()).unwrap();
        assert_eq!(remote.place(), "ssh://jane@git.acme.example:22/srv/site");
    }

    #[test]
    fn command_words_are_kept_separate() {
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\"'\"'s'");
    }
}
