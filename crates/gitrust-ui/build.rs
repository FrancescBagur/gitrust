//! Puts the icon inside the Windows binary.
//!
//! Everywhere else an icon is a file the desktop finds by name -- the
//! `.desktop` entry on Linux, the `.icns` in the bundle on macOS. Windows has
//! no such file. The icon is carried in the `.exe` itself, and a program that
//! does not carry one gets the blank sheet: in the taskbar, in Explorer, in
//! the switcher, and on the shortcut somebody makes. It is the first thing
//! anybody sees and we were shipping it empty.
//!
//! This builds the resource by hand rather than reaching for a crate that
//! does it, because what has to be built is short and the alternative is a
//! seventh dependency for one file on one platform. The two formats involved
//! are the same data twice over, which is what makes it short: an `.ico` is a
//! directory plus images, and the resources Windows wants are that same
//! directory under one name and those same images under another. Nothing is
//! decoded, resized or drawn here. The bytes are copied.
//!
//! The `.ico` comes from `assets/make.py`, with the rest of the derived art.
//! If it is missing the build goes on without it: a Windows build with no
//! icon is worth having, and a Linux build should not care at all.

use std::env;
use std::fs;
use std::path::PathBuf;

/// Windows resource types, from `winuser.h`.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;

/// The group the shell reads to pick an icon for the program.
///
/// It takes the lowest-numbered one it finds, so this is 1 and nothing else
/// in here is.
const GROUP_ID: u16 = 1;

fn main() {
    // The target, not the machine doing the building: a Mac can cross-compile
    // for Windows and this still has to run.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/icon.ico");
    println!("cargo:rerun-if-changed={}", root.display());

    // A .res opens with an entry that is all zeroes and says so: it is how a
    // reader tells this file from the 16-bit one that came before it.
    let mut res = vec![0u8; 32];
    res[4..8].copy_from_slice(&32u32.to_le_bytes());

    match fs::read(&root) {
        Ok(ico) => match icon(&mut res, &ico) {
            true => {}
            false => println!("cargo:warning=assets/icon.ico is not an icon file: skipping"),
        },
        Err(_) => println!("cargo:warning=no assets/icon.ico: the binary gets no icon"),
    }

    // Always, icon or no icon: without it, right-click, Properties, Details
    // is an empty table, and the question it answers -- which version is
    // this -- is the first one asked of anybody reporting a bug.
    entry(&mut res, RT_VERSION, 1, &version_info());

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("icon.res");
    fs::write(&out, res).expect("writing the resource file");
    // Both linkers take a .res as an object to link in.
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

/// Adds an `.ico`'s images and their group to the resources.
///
/// Gives back `false` without having touched `out` if the file is not an
/// icon, rather than guessing: half a resource is worse than none, and a
/// build that quietly linked rubbish into the binary would be found by
/// somebody else, later, on a machine none of us has.
fn icon(out: &mut Vec<u8>, ico: &[u8]) -> bool {
    match icon_resources(ico) {
        Some((images, group)) => {
            for (id, image) in images {
                entry(out, RT_ICON, id, image);
            }
            entry(out, RT_GROUP_ICON, GROUP_ID, &group);
            true
        }
        None => false,
    }
}

/// The images and the group, read out of the `.ico` and left for the caller
/// to add, so that a file that turns out not to be an icon halfway through
/// leaves nothing behind.
fn icon_resources(ico: &[u8]) -> Option<(Vec<(u16, &[u8])>, Vec<u8>)> {
    // Header: two zero words and the count. The first word is reserved and
    // the second says 1 for an icon, 2 for a cursor.
    if ico.len() < 6 || u16le(ico, 0)? != 0 || u16le(ico, 2)? != 1 {
        return None;
    }
    let count = u16le(ico, 4)? as usize;

    // The group: the same directory the .ico opens with, except that each
    // entry ends in the id of the image instead of where it sits in the file.
    // Fourteen bytes an entry here against sixteen there, and that two bytes
    // is the whole difference between the two formats.
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(count as u16).to_le_bytes());

    let mut images = Vec::new();
    for index in 0..count {
        let at = 6 + 16 * index;
        let entry = ico.get(at..at + 16)?;
        let size = u32le(entry, 8)? as usize;
        let offset = u32le(entry, 12)? as usize;
        let image = ico.get(offset..offset + size)?;

        // Numbered from one, and the group is one too -- they are different
        // kinds of resource, so the numbers do not collide.
        let id = (index + 1) as u16;

        group.extend_from_slice(&entry[0..8]);
        group.extend_from_slice(&(size as u32).to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());

        images.push((id, image));
    }

    Some((images, group))
}

/// One resource, header and data, padded out to a four-byte boundary.
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    // Type and name are each either a string or, as here, 0xFFFF followed by
    // a number. Four bytes each, so the header is a fixed thirty-two.
    const HEADER: u32 = 32;

    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&HEADER.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // data version
    out.extend_from_slice(&0x1010u16.to_le_bytes()); // moveable and discardable
    out.extend_from_slice(&0u16.to_le_bytes()); // language: neutral
    out.extend_from_slice(&0u32.to_le_bytes()); // version
    out.extend_from_slice(&0u32.to_le_bytes()); // characteristics

    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn u16le(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// The table behind right-click, Properties, Details.
///
/// Everything in it is read from Cargo.toml at build time rather than
/// written out again here, so there is one place a version lives and it is
/// the same one `--version` prints.
fn version_info() -> Vec<u8> {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    // Four numbers, because the fixed part of this structure has room for
    // four and nothing else. A crate has three; the fourth is zero and says
    // nothing, which is the honest thing for it to say.
    let mut parts = version.split('.').map(|n| n.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    let high = ((major as u32) << 16) | minor as u32;
    let low = (patch as u32) << 16;

    // The authors field carries an address, which belongs in a repository
    // and not in a dialog box on somebody's desktop.
    let authors = env::var("CARGO_PKG_AUTHORS").unwrap_or_default();
    let who = authors
        .split(':')
        .next()
        .unwrap_or("")
        .split(" <")
        .next()
        .unwrap_or("")
        .to_string();
    let licence = env::var("CARGO_PKG_LICENSE").unwrap_or_default();

    let mut fixed = Vec::new();
    for word in [
        0xFEEF_04BDu32, // the signature every reader checks for first
        0x0001_0000,    // the version of this structure, not of the program
        high,
        low, // the file's version
        high,
        low,         // and the product's, which here are one thing
        0x0000_003F, // which of the flags below are meant
        0x0000_0000, // none of them: not a debug build, not a patched one
        0x0004_0004, // made for Windows NT, as a 32-bit-or-later program
        0x0000_0001, // an application, as opposed to a driver or a font
        0x0000_0000, // which has no sub-type
        0x0000_0000, // and no date, which is how nearly everything says it
        0x0000_0000,
    ] {
        fixed.extend_from_slice(&word.to_le_bytes());
    }

    // 0409 is US English and 04B0 is 1200, which is UTF-16 -- the pair names
    // the table below, and has to be repeated under VarFileInfo or the shell
    // does not believe the table is there.
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", who.as_str()),
        ("FileDescription", &env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default()),
        ("FileVersion", version.as_str()),
        ("InternalName", "gitrust.exe"),
        ("LegalCopyright", &format!("\u{a9} {who}. {licence}.")),
        ("OriginalFilename", "gitrust.exe"),
        ("ProductName", "Gitrust"),
        ("ProductVersion", version.as_str()),
    ]
    .iter()
    .map(|(key, value)| block(key, Value::Text(value), &[]))
    .collect();

    let table = block("040904B0", Value::None, &strings);
    let string_info = block("StringFileInfo", Value::None, &[table]);
    let translation = block("Translation", Value::Binary(&0x04B0_0409u32.to_le_bytes()), &[]);
    let var_info = block("VarFileInfo", Value::None, &[translation]);

    block(
        "VS_VERSION_INFO",
        Value::Binary(&fixed),
        &[string_info, var_info],
    )
}

/// What sits between a block's key and its children.
enum Value<'a> {
    None,
    /// Counted in bytes.
    Binary(&'a [u8]),
    /// Counted in characters, terminator included, which is the one place
    /// this format changes its mind about what it is measuring.
    Text(&'a str),
}

/// One node of a version resource: a key, a value, and children.
///
/// They nest four deep and every level has the same shape, so it is written
/// once. The length written into each one leaves out the padding that
/// follows it -- the reader rounds up to the next four bytes itself, and a
/// length that already included the padding would make it skip four bytes
/// into the next block.
fn block(key: &str, value: Value, children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0u8; 2]; // the length, once it is known
    let (value_len, kind, bytes): (u16, u16, Vec<u8>) = match value {
        Value::None => (0, 1, Vec::new()),
        Value::Binary(data) => (data.len() as u16, 0, data.to_vec()),
        Value::Text(text) => {
            let mut wide: Vec<u8> = Vec::new();
            for unit in text.encode_utf16() {
                wide.extend_from_slice(&unit.to_le_bytes());
            }
            wide.extend_from_slice(&0u16.to_le_bytes());
            ((wide.len() / 2) as u16, 1, wide)
        }
    };
    out.extend_from_slice(&value_len.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    for unit in key.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());

    align(&mut out);
    out.extend_from_slice(&bytes);
    for child in children {
        align(&mut out);
        out.extend_from_slice(child);
    }

    let length = out.len() as u16;
    out[0..2].copy_from_slice(&length.to_le_bytes());
    out
}

fn align(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}
