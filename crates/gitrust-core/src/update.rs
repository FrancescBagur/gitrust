//! Knowing there is a newer version, and bringing it down.
//!
//! A program that is installed once and never says another word is one
//! that is still on its first beta a year later. So it looks, and says
//! so, and will fetch the file for this system if asked -- and stops
//! there. It does not install anything. Each system installs a different
//! way, one of them wants a password, another will not let a running
//! program be written over, and none of these files is signed: a program
//! that replaced itself with something it could not vouch for would be
//! worse than one that left the last step to whoever is sitting there.
//!
//! **Looking** costs no dependency. A version is a tag, the tags of a
//! repository are what `git ls-remote` lists, and talking to a Git
//! remote is the one thing this program was always able to do. No API,
//! no account, no limit on how often -- and nothing sent but what any
//! clone of a public repository sends.
//!
//! A tag is not a release, though, and only a release is offered: see
//! `first_published`.
//!
//! **Bringing down** is `curl`, which Linux nearly always has and macOS
//! and Windows ship. The alternative was a library for HTTPS and the
//! forty crates behind it, for one request a month. Where there is no
//! `curl` this says so, and the page of the release is still a click
//! away.
//!
//! **Checking** is against the `SHA256SUMS` published beside the files,
//! with a SHA-256 written out here because it is sixty lines and the
//! same on every system. It proves what arrived is what was built. It
//! says nothing about who built it, which is what signing is for and
//! what there is none of.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{ensure, Context, Result};

/// Where the program lives: its repository, and under it the releases.
pub const HOME: &str = "https://github.com/FrancescBagur/gitrust";

/// One piece of what follows the hyphen in `0.1.0-beta.2`.
///
/// In this order on purpose: a number sorts before a word, which is what
/// the people who write versions down agreed on and what deriving the
/// comparison gives for free.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(u64),
    Word(String),
}

/// A version, as Cargo writes one and as a tag carries one behind a `v`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    numbers: [u64; 3],
    /// What follows a hyphen, in pieces. Empty for a version that is not
    /// a beta of anything.
    before: Vec<Part>,
    /// As it was written, without the `v`.
    text: String,
}

impl Version {
    /// Reads `0.1.0`, `v0.1.0` or `0.1.0-beta.2`. Anything else is not a
    /// version: a tag called `nightly` is not a release to offer anybody.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        // Build metadata says where a version came from, not which it is.
        let plain = text.split('+').next()?;
        let (numbers, before) = match plain.split_once('-') {
            Some((numbers, before)) => (numbers, Some(before)),
            None => (plain, None),
        };
        let mut read = numbers.split('.').map(|number| match number.is_empty() {
            true => None,
            false => number.parse::<u64>().ok(),
        });
        let numbers = [read.next()??, read.next()??, read.next()??];
        if read.next().is_some() {
            return None;
        }
        let before = match before {
            None => Vec::new(),
            Some("") => return None,
            Some(before) => before
                .split('.')
                .map(|part| match part.parse::<u64>() {
                    Ok(number) => Some(Part::Number(number)),
                    Err(_) if part.is_empty() => None,
                    Err(_) => Some(Part::Word(part.to_string())),
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(Self { numbers, before, text: text.to_string() })
    }

    /// The version this program was built as.
    pub fn running() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("Cargo writes versions this can read")
    }

    /// Whether it is a version on the way to one, and not one.
    pub fn is_beta(&self) -> bool {
        !self.before.is_empty()
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    /// `0.1.0-beta.1`, then `0.1.0-beta.2`, then `0.1.0`: a beta comes
    /// before the version it is a beta of, however it is spelled.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.numbers.cmp(&other.numbers).then_with(|| {
            match (self.before.is_empty(), other.before.is_empty()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => self.before.cmp(&other.before),
            }
        })
    }
}

/// A version that is out, and newer than the one running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The tag it was published from, `v` and all.
    pub tag: String,
    /// The `SHA256SUMS` published with it, read when it was found. What
    /// a file brought down later is checked against -- and having it at
    /// all is how a release is known to be one, and not only a tag.
    /// `None` when there was no way to ask.
    sums: Option<String>,
}

impl Release {
    /// The page that says what is in it, and has every file.
    pub fn page(&self) -> String {
        format!("{HOME}/releases/tag/{}", self.tag)
    }

    /// The file for the system this is running on, when there is one.
    pub fn file(&self) -> Option<String> {
        file_for(
            &self.version.text,
            std::env::consts::OS,
            std::env::consts::ARCH,
            installed_as_a_package(),
        )
    }

    fn address(&self, name: &str) -> String {
        format!("{HOME}/releases/download/{}/{name}", self.tag)
    }
}

/// The name a release gives the file for a system.
///
/// These are the names the release workflow writes, and they are written
/// in both places: change one and this stops finding the file, which the
/// test beside it is there to say out loud.
///
/// `packaged` is whether this copy came from the `.deb`. Whoever took the
/// `.deb` is given the next `.deb`; whoever unpacked the folder or built
/// it is given the folder. Only what the release builds is offered --
/// there is no Linux file for a processor that is not x86-64, and saying
/// "none" is better than handing over one that will not start.
pub fn file_for(version: &str, system: &str, processor: &str, packaged: bool) -> Option<String> {
    match (system, processor, packaged) {
        ("linux", "x86_64", true) => Some(format!("gitrust_{version}_amd64.deb")),
        ("linux", "x86_64", false) => Some(format!("gitrust-{version}-linux-x86_64.tar.gz")),
        ("macos", _, _) => Some(format!("gitrust-{version}-macos-universal.zip")),
        ("windows", "x86_64", _) => Some(format!("gitrust-{version}-windows-x86_64.zip")),
        _ => None,
    }
}

/// Whether dpkg put this program here.
fn installed_as_a_package() -> bool {
    cfg!(target_os = "linux") && Path::new("/var/lib/dpkg/info/gitrust.list").exists()
}

/// The newest of those tags that is newer than what is running.
///
/// Betas are offered to whoever asked for them, and to whoever is running
/// one: they asked by installing it, and a beta that is never told about
/// the next beta is stuck on the one with the bug it reported.
pub fn newest<'a>(tags: impl IntoIterator<Item = &'a str>, running: &Version, betas: bool) -> Option<Release> {
    newer(tags, running, betas).into_iter().next()
}

/// Every tag that is newer than what is running, newest first.
fn newer<'a>(tags: impl IntoIterator<Item = &'a str>, running: &Version, betas: bool) -> Vec<Release> {
    let betas = betas || running.is_beta();
    let mut found: Vec<Release> = tags
        .into_iter()
        .filter_map(|tag| {
            let tag = tag.strip_prefix("refs/tags/").unwrap_or(tag);
            // The second line a tag with a message gets in the listing.
            if tag.ends_with("^{}") || !tag.starts_with('v') {
                return None;
            }
            let version = Version::parse(tag)?;
            Some(Release { version, tag: tag.to_string(), sums: None })
        })
        .filter(|release| release.version > *running && (betas || !release.version.is_beta()))
        .collect();
    found.sort_by(|one, other| other.version.cmp(&one.version));
    found
}

/// What asking for a release's sums came back with.
enum Asked {
    /// They are there: it is published.
    Sums(String),
    /// The tag is there and the release is not, or not yet.
    Nothing,
    /// There is no `curl` to ask with.
    NoWay,
}

/// The newest of those that has really been published.
///
/// A tag is not a release. The files are built from the tag and take a
/// while, and a tag pushed by mistake never gets any -- the workflow
/// refuses one that disagrees with the version in the source. Offering
/// a version on the strength of its tag would be offering, to everybody
/// who has the program, a download that is not there. So the sums are
/// asked for, and a tag without them is passed over for the one before.
///
/// Only the few newest are asked about. With no way of asking, the
/// newest tag is offered as it is: its page is still worth a visit.
fn first_published(
    candidates: Vec<Release>,
    mut ask: impl FnMut(&Release) -> Result<Asked>,
) -> Result<Option<Release>> {
    for mut release in candidates.into_iter().take(3) {
        match ask(&release)? {
            Asked::Sums(sums) => {
                release.sums = Some(sums);
                return Ok(Some(release));
            }
            Asked::Nothing => continue,
            Asked::NoWay => return Ok(Some(release)),
        }
    }
    Ok(None)
}

/// Asks the repository which versions there are, and gives back the one
/// worth mentioning, if any.
///
/// Over the network, so never from the thread that paints. With no
/// network this is an error, and whoever asked decides whether it is one
/// worth showing -- looking unasked and finding no network is not.
pub fn look(running: &Version, betas: bool) -> Result<Option<Release>> {
    let mut remote = git2::Remote::create_detached(HOME).context("could not prepare to ask for the versions")?;
    remote
        .connect(git2::Direction::Fetch)
        .with_context(|| format!("could not reach {HOME}"))?;
    let listed = remote.list().context("the list of versions did not arrive")?;
    let tags: Vec<&str> = listed.iter().map(|head| head.name()).collect();
    first_published(newer(tags, running, betas), |release| {
        match ask(&release.address("SHA256SUMS"), None) {
            Ok(sums) => Ok(String::from_utf8(sums).map(Asked::Sums).unwrap_or(Asked::Nothing)),
            Err(Trouble::Refused(_)) => Ok(Asked::Nothing),
            Err(Trouble::NoCurl(_)) => Ok(Asked::NoWay),
            Err(trouble) => Err(trouble.into_error(&release.address("SHA256SUMS"))),
        }
    })
}

/// Brings a release's file for this system into `folder`, and checks it.
///
/// Gives back where it is. A file already there under that name is kept
/// if it is the right one and written over if it is not. What is being
/// brought down has another name until it has been checked, so nothing
/// half-arrived or wrong is ever left looking like the real thing.
pub fn fetch(release: &Release, folder: &Path) -> Result<PathBuf> {
    let name = release
        .file()
        .with_context(|| format!("{} has no file for this system; its page has all of them", release.tag))?;
    let sums = match &release.sums {
        Some(sums) => sums.clone(),
        None => String::from_utf8(get(&release.address("SHA256SUMS"), None)?)
            .context("the release's SHA256SUMS is not text")?,
    };
    let wanted = sum_for(&sums, &name)
        .with_context(|| format!("the release's SHA256SUMS does not name {name}, so it cannot be checked"))?
        .to_string();

    let place = folder.join(&name);
    if place.is_file() && sha256_of(&place).is_ok_and(|found| found == wanted) {
        return Ok(place);
    }

    std::fs::create_dir_all(folder).with_context(|| format!("could not make {}", folder.display()))?;
    let arriving = folder.join(format!("{name}.part"));
    let brought = get(&release.address(&name), Some(&arriving)).and_then(|_| {
        let found = sha256_of(&arriving)?;
        ensure!(
            found == wanted,
            "{name} did not arrive as it was published: its SHA-256 is {found} and the release says {wanted}"
        );
        std::fs::rename(&arriving, &place).with_context(|| format!("could not write {}", place.display()))
    });
    if brought.is_err() {
        let _ = std::fs::remove_file(&arriving);
    }
    brought.map(|()| place)
}

/// Why a request came back with nothing.
enum Trouble {
    /// `curl` could not be run at all.
    NoCurl(std::io::Error),
    /// The server answered, and the answer was no: what was asked for is
    /// not there.
    Refused(String),
    /// Anything else, which is nearly always the network.
    Failed(String),
}

impl Trouble {
    fn into_error(self, address: &str) -> anyhow::Error {
        match self {
            Trouble::NoCurl(err) => anyhow::Error::new(err)
                .context("curl is needed to bring a file down, and it could not be run"),
            Trouble::Refused(said) | Trouble::Failed(said) => {
                anyhow::anyhow!("{address} did not come down: {said}")
            }
        }
    }
}

/// One request, with `curl`: into a file, or into memory for something
/// small.
fn get(address: &str, into: Option<&Path>) -> Result<Vec<u8>> {
    ask(address, into).map_err(|trouble| trouble.into_error(address))
}

/// The same, saying which way it went wrong.
///
/// HTTPS and nothing else, at the first address and at every one it is
/// sent on to -- a release file is served from somewhere other than
/// where it is asked for.
fn ask(address: &str, into: Option<&Path>) -> std::result::Result<Vec<u8>, Trouble> {
    let mut curl = Command::new("curl");
    curl.args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "20",
        "--max-time",
        "900",
        "--output",
    ]);
    match into {
        Some(file) => curl.arg(file),
        None => curl.arg("-"),
    };
    curl.arg(address);
    quiet(&mut curl);
    let done = curl.output().map_err(Trouble::NoCurl)?;
    if done.status.success() {
        return Ok(done.stdout);
    }
    let said = String::from_utf8_lossy(&done.stderr);
    let said = said.trim().trim_start_matches("curl: ").to_string();
    // What `--fail` leaves with when the server said no.
    Err(match done.status.code() {
        Some(22) => Trouble::Refused(said),
        _ => Trouble::Failed(said),
    })
}

/// A program run from a window on Windows opens a black one of its own
/// unless it is told not to.
fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// The line of a `SHA256SUMS` that is about `name`: its sum.
///
/// `sha256sum` writes the sum, two spaces or a space and a star, and the
/// name. Only a whole name counts, and only a sum that is one.
pub fn sum_for<'a>(sums: &'a str, name: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (sum, file) = line.split_once(' ')?;
        let file = file.trim_start_matches([' ', '*']).trim_end();
        (file == name && sum.len() == 64 && sum.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(sum)
    })
}

/// Where a file somebody asked for goes: the folder the desktop calls
/// Downloads, whatever it is called in their language.
///
/// On Linux that is written down in `user-dirs.dirs`; everywhere else,
/// and where that file says nothing, it is `Downloads` under the home
/// folder, and failing that the home folder.
pub fn downloads() -> Option<PathBuf> {
    let home = crate::home::folder()?;
    let named = crate::home::config()
        .and_then(|ours| Some(ours.parent()?.join("user-dirs.dirs")))
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| downloads_in(&text, &home));
    let usual = home.join("Downloads");
    named
        .filter(|folder| folder.is_dir())
        .or_else(|| usual.is_dir().then_some(usual))
        .or(Some(home))
}

/// The `XDG_DOWNLOAD_DIR="$HOME/Descargas"` line of `user-dirs.dirs`.
fn downloads_in(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?
        .trim()
        .trim_matches('"');
    match value.strip_prefix("$HOME/") {
        Some(rest) => Some(home.join(rest)),
        None if value.starts_with('/') => Some(PathBuf::from(value)),
        None => None,
    }
}

/// The SHA-256 of a file, as `sha256sum` prints it.
pub fn sha256_of(file: &Path) -> Result<String> {
    let mut file = File::open(file).with_context(|| format!("could not read {}", file.display()))?;
    let mut sum = Sha256::new();
    let mut piece = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut piece).context("could not read what was brought down")?;
        if read == 0 {
            break;
        }
        sum.take(&piece[..read]);
    }
    Ok(sum.finish())
}

/// SHA-256, as FIPS 180-4 writes it down.
///
/// Here and not in a crate because it is this much, has not changed since
/// 2002, and the test beside it holds it to the published answers.
struct Sha256 {
    state: [u32; 8],
    /// What has arrived and does not yet fill a block.
    held: Vec<u8>,
    length: u64,
}

const ROUNDS: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            held: Vec::with_capacity(64),
            length: 0,
        }
    }

    fn take(&mut self, mut bytes: &[u8]) {
        self.length += bytes.len() as u64;
        if !self.held.is_empty() {
            let wanted = (64 - self.held.len()).min(bytes.len());
            self.held.extend_from_slice(&bytes[..wanted]);
            bytes = &bytes[wanted..];
            if self.held.len() < 64 {
                return;
            }
            let block: [u8; 64] = self.held[..].try_into().expect("sixty-four were counted");
            self.block(&block);
            self.held.clear();
        }
        let mut blocks = bytes.chunks_exact(64);
        for block in &mut blocks {
            self.block(block.try_into().expect("chunks of sixty-four"));
        }
        self.held.extend_from_slice(blocks.remainder());
    }

    fn finish(mut self) -> String {
        let bits = self.length.wrapping_mul(8);
        let mut tail = vec![0x80u8];
        let filled = (self.held.len() + 1) % 64;
        tail.resize(1 + (120 - filled) % 64, 0);
        tail.extend_from_slice(&bits.to_be_bytes());
        // Not counted: `length` was read above, and this is not the file.
        self.take(&tail);
        debug_assert!(self.held.is_empty());
        self.state.iter().map(|word| format!("{word:08x}")).collect()
    }

    fn block(&mut self, block: &[u8; 64]) {
        let mut words = [0u32; 64];
        for (word, bytes) in words.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes(bytes.try_into().expect("chunks of four"));
        }
        for at in 16..64 {
            let early = words[at - 15].rotate_right(7) ^ words[at - 15].rotate_right(18) ^ (words[at - 15] >> 3);
            let late = words[at - 2].rotate_right(17) ^ words[at - 2].rotate_right(19) ^ (words[at - 2] >> 10);
            words[at] = words[at - 16]
                .wrapping_add(early)
                .wrapping_add(words[at - 7])
                .wrapping_add(late);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for at in 0..64 {
            let mix = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let one = h
                .wrapping_add(mix)
                .wrapping_add(choice)
                .wrapping_add(ROUNDS[at])
                .wrapping_add(words[at]);
            let mix = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let two = mix.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(one);
            d = c;
            c = b;
            b = a;
            a = one.wrapping_add(two);
        }
        for (kept, new) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *kept = kept.wrapping_add(new);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("{text} is a version"))
    }

    fn sha256(bytes: &[u8]) -> String {
        let mut sum = Sha256::new();
        sum.take(bytes);
        sum.finish()
    }

    #[test]
    fn a_beta_comes_before_the_version_it_is_a_beta_of() {
        let order = [
            "0.1.0-alpha",
            "0.1.0-beta.1",
            "0.1.0-beta.2",
            "0.1.0-beta.10",
            "0.1.0-rc.1",
            "0.1.0",
            "0.1.1",
            "0.2.0-beta.1",
            "0.2.0",
            "1.0.0",
        ];
        for pair in order.windows(2) {
            assert!(version(pair[0]) < version(pair[1]), "{} is not before {}", pair[0], pair[1]);
        }
        // A tag is the same version with a letter in front.
        assert_eq!(version("v0.1.0-beta.1"), version("0.1.0-beta.1"));
        assert_eq!(version("v0.1.0-beta.1").to_string(), "0.1.0-beta.1");
        assert!(version("0.1.0-beta.1").is_beta());
        assert!(!version("0.1.0").is_beta());
    }

    #[test]
    fn what_is_not_a_version_is_not_read_as_one() {
        for text in ["", "v", "nightly", "1.2", "1.2.3.4", "1.2.x", "1.2.3-", "1.2.3-beta..1", "1..3", "-1.2.3"] {
            assert!(Version::parse(text).is_none(), "{text:?} was read as a version");
        }
        // And the one this was built as is.
        assert_eq!(Version::running().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_newest_tag_is_offered_and_betas_only_to_whoever_wants_them() {
        let tags = [
            "refs/heads/master",
            "refs/tags/v0.1.0-beta.1",
            "refs/tags/v0.1.0",
            "refs/tags/v0.1.0^{}",
            "refs/tags/v0.2.0-beta.1",
            "refs/tags/nightly",
            "refs/tags/0.9.0",
            "refs/pull/14/head",
        ];

        // On a release, with nothing said: the newest release, and it has
        // to be newer.
        assert_eq!(newest(tags, &version("0.0.9"), false).unwrap().tag, "v0.1.0");
        assert_eq!(newest(tags, &version("0.1.0"), false), None);
        // Having asked for betas, the beta of the next one.
        let beta = newest(tags, &version("0.1.0"), true).unwrap();
        assert_eq!(beta.tag, "v0.2.0-beta.1");
        assert_eq!(beta.page(), format!("{HOME}/releases/tag/v0.2.0-beta.1"));
        // Running a beta is having asked.
        assert_eq!(newest(tags, &version("0.1.0-beta.1"), false).unwrap().tag, "v0.2.0-beta.1");
        // Nothing is offered to the newest there is, or to a newer one
        // somebody built for themselves.
        assert_eq!(newest(tags, &version("0.2.0-beta.1"), true), None);
        assert_eq!(newest(tags, &version("3.0.0"), true), None);
    }

    #[test]
    fn a_tag_with_no_release_behind_it_is_passed_over() {
        let tags = ["refs/tags/v0.1.0", "refs/tags/v0.2.0", "refs/tags/v0.3.0"];
        let candidates = || newer(tags, &version("0.0.9"), false);
        assert_eq!(
            candidates().iter().map(|release| release.tag.as_str()).collect::<Vec<_>>(),
            ["v0.3.0", "v0.2.0", "v0.1.0"]
        );

        // The newest is only a tag so far: the one before it is offered,
        // with the sums it will be checked against.
        let found = first_published(candidates(), |release| {
            Ok(match release.tag.as_str() {
                "v0.3.0" => Asked::Nothing,
                _ => Asked::Sums(format!("sums of {}", release.tag)),
            })
        })
        .unwrap()
        .unwrap();
        assert_eq!(found.tag, "v0.2.0");
        assert_eq!(found.sums.as_deref(), Some("sums of v0.2.0"));

        // None of them published: nothing to say.
        assert_eq!(first_published(candidates(), |_| Ok(Asked::Nothing)).unwrap(), None);
        // No way of asking: the newest tag, as it is.
        let found = first_published(candidates(), |_| Ok(Asked::NoWay)).unwrap().unwrap();
        assert_eq!((found.tag.as_str(), &found.sums), ("v0.3.0", &None));
        // And no network is an error, not "nothing newer".
        assert!(first_published(candidates(), |_| anyhow::bail!("no network")).is_err());
        // With nothing newer, nobody is asked anything.
        assert_eq!(
            first_published(newer(tags, &version("1.0.0"), false), |_| panic!("asked")).unwrap(),
            None
        );
    }

    /// The names here are the ones `.github/workflows/release.yml` gives
    /// the files. They are written in both places and nowhere else.
    #[test]
    fn each_system_is_given_the_file_the_release_made_for_it() {
        let name = |system, processor, packaged| file_for("0.1.0-beta.2", system, processor, packaged);
        assert_eq!(name("linux", "x86_64", true).unwrap(), "gitrust_0.1.0-beta.2_amd64.deb");
        assert_eq!(name("linux", "x86_64", false).unwrap(), "gitrust-0.1.0-beta.2-linux-x86_64.tar.gz");
        assert_eq!(name("macos", "aarch64", false).unwrap(), "gitrust-0.1.0-beta.2-macos-universal.zip");
        assert_eq!(name("macos", "x86_64", false).unwrap(), "gitrust-0.1.0-beta.2-macos-universal.zip");
        assert_eq!(name("windows", "x86_64", false).unwrap(), "gitrust-0.1.0-beta.2-windows-x86_64.zip");
        // What the release does not build is not offered.
        assert_eq!(name("linux", "aarch64", false), None);
        assert_eq!(name("windows", "aarch64", false), None);
        assert_eq!(name("freebsd", "x86_64", false), None);

        let workflow = include_str!("../../../.github/workflows/release.yml");
        for written in [
            "gitrust-$version-linux-x86_64",
            "gitrust_${version}_${deb##*_}",
            "gitrust-$version-macos-universal.zip",
            "gitrust-$version-windows-x86_64",
        ] {
            assert!(workflow.contains(written), "the release no longer names a file {written}");
        }
    }

    #[test]
    fn a_sum_is_found_by_the_whole_name_and_only_if_it_is_one() {
        let sum = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        let sums = format!(
            "{sum}  gitrust-0.2.0-linux-x86_64.tar.gz\n\
             {}  gitrust_0.2.0_amd64.deb\n\
             {sum} *gitrust-0.2.0-windows-x86_64.zip\n\
             not-a-sum  gitrust-0.2.0-macos-universal.zip\n",
            "0".repeat(64)
        );
        assert_eq!(sum_for(&sums, "gitrust-0.2.0-linux-x86_64.tar.gz"), Some(sum));
        assert_eq!(sum_for(&sums, "gitrust-0.2.0-windows-x86_64.zip"), Some(sum));
        assert_eq!(sum_for(&sums, "gitrust_0.2.0_amd64.deb").unwrap(), "0".repeat(64));
        // Part of a name is not the name, and a line that is not a sum
        // vouches for nothing.
        assert_eq!(sum_for(&sums, "linux-x86_64.tar.gz"), None);
        assert_eq!(sum_for(&sums, "gitrust-0.2.0-macos-universal.zip"), None);
        assert_eq!(sum_for(&sums, "gitrust-0.3.0-linux-x86_64.tar.gz"), None);
    }

    /// The answers the standard publishes, and the lengths around a block
    /// where a mistake in the padding would be.
    #[test]
    fn sha256_gives_the_published_answers() {
        assert_eq!(sha256(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256(&[b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        for (length, wanted) in [
            (55, "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"),
            (56, "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"),
            (63, "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34"),
            (64, "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"),
            (65, "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0"),
        ] {
            assert_eq!(sha256(&vec![b'a'; length]), wanted, "{length} bytes");
        }

        // Handed over a piece at a time, as a file is, it is the same sum.
        let mut sum = Sha256::new();
        for piece in [b'a'; 1000].chunks(7) {
            sum.take(piece);
        }
        assert_eq!(sum.finish(), sha256(&[b'a'; 1000]));
    }

    #[test]
    fn the_downloads_folder_is_the_one_the_desktop_names() {
        let home = Path::new("/home/someone");
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Escritorio\"\nXDG_DOWNLOAD_DIR=\"$HOME/Descargas\"\n";
        assert_eq!(downloads_in(text, home), Some(PathBuf::from("/home/someone/Descargas")));
        assert_eq!(
            downloads_in("XDG_DOWNLOAD_DIR=\"/srv/incoming\"\n", home),
            Some(PathBuf::from("/srv/incoming"))
        );
        assert_eq!(downloads_in("XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n", home), None);
        assert_eq!(downloads_in("XDG_DOWNLOAD_DIR=\"somewhere\"\n", home), None);
    }
}
