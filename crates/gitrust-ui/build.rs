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

    let Ok(ico) = fs::read(&root) else {
        println!("cargo:warning=no assets/icon.ico: the binary gets no icon");
        return;
    };
    let Some(res) = resources(&ico) else {
        println!("cargo:warning=assets/icon.ico is not an icon file: skipping");
        return;
    };

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("icon.res");
    fs::write(&out, res).expect("writing the resource file");
    // Both linkers take a .res as an object to link in.
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

/// Turns an `.ico` into the bytes of a `.res`.
///
/// Returns `None` if the file is not an icon, rather than guessing: half a
/// resource is worse than none, and a build that quietly linked rubbish into
/// the binary would be found by somebody else, later, on a machine none of
/// us has.
fn resources(ico: &[u8]) -> Option<Vec<u8>> {
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

    // A .res opens with an entry that is all zeroes and says so: it is how a
    // reader tells this file from the 16-bit one that came before it.
    let mut out = Vec::new();
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&[0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00]);
    out.extend_from_slice(&[0; 16]);
    // Overwrite the two sizes the null entry has to carry: no data, and a
    // header of the thirty-two bytes just written.
    out[0..4].copy_from_slice(&0u32.to_le_bytes());
    out[4..8].copy_from_slice(&32u32.to_le_bytes());

    for (id, image) in images {
        entry(&mut out, RT_ICON, id, image);
    }
    entry(&mut out, RT_GROUP_ICON, GROUP_ID, &group);

    Some(out)
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
