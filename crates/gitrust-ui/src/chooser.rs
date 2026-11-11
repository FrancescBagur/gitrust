//! Asking the desktop to put its own folder chooser up.
//!
//! # Why this is not a subprocess
//!
//! It was, and it did not work. The first go at this ran `gdbus` twice --
//! once to ask `xdg-desktop-portal` for a folder, once to watch the bus for
//! the answer -- on the grounds that the pull requests are read by running
//! `gh`, so a dialog could be asked for the same way. The call went through
//! and the dialog came up. The answer never arrived.
//!
//! The reason is worth writing down, because it is not a bug anybody can
//! fix: the portal's reply is a *directed* signal, addressed to the bus name
//! of whoever asked. `gdbus call` gets a handle and exits, so by the time
//! somebody has chosen a folder there is nobody at that address. And
//! `gdbus monitor` never sees it either -- measured, with a process that
//! stayed alive getting the reply at the same moment the monitor printed
//! nothing at all.
//!
//! So the caller has to stay on the connection, which means a D-Bus client
//! in this process, which means a dependency.
//!
//! # What it costs
//!
//! Three crates: `rfd`, and `pollster` and `block2` underneath it. Not the
//! tree that number usually implies, because the D-Bus machinery is already
//! in this binary -- eframe pulls `ashpd` and `zbus` in for Wayland -- so
//! this is a small thing sitting on top of something already paid for.
//!
//! It is still the sixth dependency of a project that had five on purpose,
//! and that was a decision, not an oversight.

use std::path::PathBuf;

use anyhow::Result;

/// Puts the desktop's folder chooser up and waits for an answer.
///
/// `None` is an ordinary answer: it is what cancelling looks like.
///
/// This blocks for as long as somebody is looking at the dialog, which can be
/// minutes, so it belongs on a thread of its own and not on the one painting
/// the window.
pub fn folder(title: &str) -> Result<Option<PathBuf>> {
    // No parent window is given. Telling the portal which window we are would
    // mean handing it an X11 or Wayland handle, and eframe does not give one
    // out. The dialog comes up unparented, which costs it being tied to the
    // window and nothing else.
    Ok(rfd::FileDialog::new().set_title(title).pick_folder())
}

/// The same for a single file, which is what an SSH key is.
///
/// It opens where the keys are when there is such a folder, because a
/// chooser that starts in the home folder is a chooser that cannot see
/// `.ssh` at all: it is hidden, and nobody should have to know the shortcut
/// for showing hidden files to pick their own key.
pub fn file(title: &str, from: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if let Some(from) = from.filter(|from| from.is_dir()) {
        dialog = dialog.set_directory(from);
    }
    Ok(dialog.pick_file())
}
