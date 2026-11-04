//! Asking the desktop to put its own folder chooser up.
//!
//! Not a dependency: a subprocess, which is the same way the pull requests
//! are read. `gh` is run to ask GitHub a question; `gdbus` is run to ask the
//! desktop one. A file-dialog crate would be the sixth thing in a Cargo.toml
//! that has five in it on purpose, and the whole of what it would do here is
//! this.
//!
//! # Which dialog
//!
//! The desktop's own, through `xdg-desktop-portal`. That is the interface
//! every modern Linux desktop answers -- COSMIC, GNOME, KDE each ship their
//! own backend behind it -- and it is the one a sandboxed application would
//! have to use anyway. It is asked over D-Bus, and `gdbus` is the command
//! that speaks D-Bus from a shell. It comes with glib, which is on every
//! desktop that has a portal to talk to.
//!
//! # What it costs
//!
//! Honesty about the shape of this: it reads text that `gdbus monitor`
//! prints, and that text is a debugging format, not an interface anybody
//! promised to keep. It works here and it will keep working for a long time,
//! but it is the kind of thing that breaks quietly one day. Everything it can
//! do wrong ends the same way -- no folder chosen -- and the box you can type
//! a path into never goes away, so nothing is ever stuck behind it.
//!
//! On anything that is not a Linux desktop there is no portal and this says
//! so. Typing the path still works everywhere.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

/// Puts the desktop's folder chooser up and waits for an answer.
///
/// `None` is an ordinary answer: it is what cancelling looks like.
///
/// This blocks for as long as somebody is looking at the dialog, which can be
/// minutes, so it belongs on a thread of its own and not on the one painting
/// the window.
pub fn folder(title: &str) -> Result<Option<PathBuf>> {
    // Listening starts before asking. The other way round there is a window,
    // however small, in which the answer arrives before anybody is there to
    // hear it, and then this waits for a reply that already came.
    let mut monitor = Command::new("gdbus")
        .args(["monitor", "--session", "--dest", "org.freedesktop.portal.Desktop"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("gdbus is not here, so the desktop cannot be asked for a folder")?;

    let Some(out) = monitor.stdout.take() else {
        let _ = monitor.kill();
        anyhow::bail!("gdbus said nothing at all");
    };

    let asked = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.FileChooser.OpenFile",
            // No parent window. Telling the portal which window we are would
            // mean handing it an X11 or Wayland handle, and eframe does not
            // give one out. The dialog comes up unparented, which costs it
            // being tied to the window and nothing else.
            "",
            title,
            "{'directory': <true>, 'modal': <true>}",
        ])
        .output();

    let asked = match asked {
        Ok(asked) if asked.status.success() => asked,
        other => {
            let _ = monitor.kill();
            let why = other
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stderr).trim().to_string())
                .filter(|why| !why.is_empty())
                .unwrap_or_else(|| "the desktop has no file chooser to offer".to_string());
            anyhow::bail!("{why}");
        }
    };

    // The call hands back the path of the request this answer will come on.
    // Without it any other application's dialog closing would look like ours.
    let handle = between(&String::from_utf8_lossy(&asked.stdout), "objectpath '", "'");

    let mut found = None;
    for line in BufReader::new(out).lines().map_while(Result::ok) {
        if !line.contains("::Response") {
            continue;
        }
        if let Some(handle) = &handle {
            if !line.contains(handle.as_str()) {
                continue;
            }
        }
        // Cancelling answers too, with a code and no address in it. Either
        // way the waiting is over.
        found = between(&line, "file://", "'").and_then(|uri| unescape(&uri)).map(PathBuf::from);
        break;
    }

    let _ = monitor.kill();
    let _ = monitor.wait();
    Ok(found)
}

/// Whether there is any point offering the button.
///
/// Asked once, when the screen opens. It is a question with a stable answer:
/// a desktop does not grow a portal while somebody is looking at a window.
pub fn there() -> bool {
    Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.DBus.Properties.Get",
            "org.freedesktop.portal.FileChooser",
            "version",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// What sits between two markers, the first time both are found.
fn between(text: &str, after: &str, before: &str) -> Option<String> {
    let start = text.find(after)? + after.len();
    let rest = &text[start..];
    let end = rest.find(before)?;
    Some(rest[..end].to_string())
}

/// Turns `%20` and its friends back into the characters they stand for.
///
/// A URI is what the portal answers with, and a folder called `My Projects`
/// comes back as `My%20Projects`. Anything that is not a well-formed escape
/// is left exactly as it was: a path is somebody's folder name and mangling
/// it quietly is worse than handing back something that does not exist.
fn unescape(uri: &str) -> Option<String> {
    let mut out = Vec::with_capacity(uri.len());
    let bytes = uri.as_bytes();
    let mut at = 0;

    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&uri[at + 1..at + 3], 16) {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }

    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_pulled_out_of_what_the_portal_prints() {
        let line = "/org/freedesktop/portal/desktop/request/1_9/t: \
                    org.freedesktop.portal.Request::Response (uint32 0, \
                    {'uris': <['file:///home/someone/Projects/picstar']>})";
        assert_eq!(
            between(line, "file://", "'").as_deref(),
            Some("/home/someone/Projects/picstar")
        );
    }

    #[test]
    fn a_folder_with_a_space_in_its_name_comes_back_with_the_space() {
        assert_eq!(
            unescape("/home/someone/My%20Projects/the%20site").as_deref(),
            Some("/home/someone/My Projects/the site")
        );
    }

    /// A name with a per-cent sign in it is a name, not a broken escape.
    #[test]
    fn something_that_is_not_an_escape_is_left_where_it_is() {
        for path in ["/tmp/100%", "/tmp/a%zz/b", "/tmp/%"] {
            assert_eq!(unescape(path).as_deref(), Some(path), "{path}");
        }
    }

    #[test]
    fn cancelling_leaves_no_address_to_find() {
        let line = "/org/freedesktop/portal/desktop/request/1_9/t: \
                    org.freedesktop.portal.Request::Response (uint32 1, {})";
        assert_eq!(between(line, "file://", "'"), None);
    }
}
