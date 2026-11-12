//! Keeping the window up to date with what other programs do.
//!
//! A repository is not only changed from here. An editor saves a file, a
//! terminal makes a commit, an agent does both for an hour, and a window
//! that goes on showing what was true when it last read is one that has
//! to be told to look -- with a button, by somebody who has to know it
//! is stale to press it.
//!
//! GitKraken is told. It asks the system to watch the working tree and
//! `.git`, waits two seconds for the events to stop, and reads again
//! whichever part they were about. That is a watch on every folder in
//! the tree, the ignored ones included, and it comes with the three
//! messages GitKraken has for when it does not work: the watcher would
//! not start, the system ran out of watches, the events never arrived.
//!
//! This looks instead. Off the thread that paints, it reads the
//! repository's [`Pulse`] -- the walk `git status` makes, and a list of
//! the references -- and says so when it is not the one it read last.
//! The window compares that with the pulse of what it is showing and
//! reads again only the half that moved, so a look that finds nothing
//! costs the window nothing at all, and what the window itself has just
//! done is not news to it.
//!
//! The same two seconds, while the window is the one being typed into;
//! five when it is not, and at once when it becomes so or another tab is
//! brought to the front. A walk that takes long is made less often, so
//! that looking is never more than a twentieth of the time.
//!
//! Only the repository in front, and only one on this machine. A
//! checkout on another machine is a round trip per look over a
//! connection the window is using; its reload button is where it was.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use gitrust_core::{Pulse, Repo};

/// How long between looks while the window is the one in use.
const NEAR: Duration = Duration::from_secs(2);
/// And while it is not, but may be in sight beside whatever is.
const FAR: Duration = Duration::from_secs(5);
/// However long the walk takes, it is looked at this often at least.
const SLOWEST: Duration = Duration::from_secs(60);

/// What the watcher was last told: where to look, and how closely.
#[derive(Clone, PartialEq, Eq, Default)]
struct Told {
    /// `None` is nowhere: no repository in front, or the window put away.
    path: Option<PathBuf>,
    near: bool,
}

/// The thread that looks, and the two ends of talking to it.
pub struct Watch {
    tell: Sender<Told>,
    heard: Receiver<(PathBuf, Pulse)>,
    told: Told,
}

impl Watch {
    /// Starts looking at nothing. `ctx` is how the window is woken when
    /// there is something to hear: it sleeps while nothing moves.
    pub fn start(ctx: egui::Context) -> Self {
        let (tell, told) = mpsc::channel();
        let (say, heard) = mpsc::channel();
        std::thread::spawn(move || run(told, say, ctx));
        Self { tell, heard, told: Told::default() }
    }

    /// Says where to look from now on. Said every frame and passed on
    /// only when it is not what was said before -- and then looked at
    /// straight away, which is what makes coming back to the window or
    /// to a tab find it current.
    pub fn look_at(&mut self, path: Option<&Path>, near: bool) {
        let told = Told { path: path.map(Path::to_path_buf), near };
        if told != self.told {
            self.told = told.clone();
            let _ = self.tell.send(told);
        }
    }

    /// What has been seen since this was last asked.
    pub fn heard(&self) -> Vec<(PathBuf, Pulse)> {
        self.heard.try_iter().collect()
    }
}

/// How long to wait before the next look, after one that took `cost`.
pub fn pace(cost: Duration, near: bool) -> Duration {
    let wanted = if near { NEAR } else { FAR };
    wanted.max(cost * 20).min(SLOWEST)
}

fn run(told: Receiver<Told>, say: Sender<(PathBuf, Pulse)>, ctx: egui::Context) {
    let mut now = Told::default();
    let mut last: Option<(PathBuf, Pulse)> = None;
    let mut wait = NEAR;
    loop {
        // With nowhere to look there is nothing to wake up for.
        let news = match now.path {
            Some(_) => told.recv_timeout(wait),
            None => told.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match news {
            Ok(next) => now = next,
            Err(RecvTimeoutError::Timeout) => {}
            // The window has gone.
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Only the last thing said counts.
        while let Ok(next) = told.try_recv() {
            now = next;
        }
        let Some(path) = now.path.clone() else {
            continue;
        };

        // Opened each time. It costs next to nothing beside the walk, and
        // a repository held open is one more thing that can disagree with
        // the disk.
        let started = Instant::now();
        let pulse = Repo::open(&path).and_then(|repo| repo.pulse());
        wait = pace(started.elapsed(), now.near);

        // A repository that cannot be read just now -- another program is
        // halfway through writing it -- is looked at again next time.
        let Ok(pulse) = pulse else {
            continue;
        };
        if last.as_ref() != Some(&(path.clone(), pulse)) {
            last = Some((path.clone(), pulse));
            if say.send((path, pulse)).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quick_look_is_made_every_two_seconds_and_a_slow_one_less_often() {
        assert_eq!(pace(Duration::from_millis(3), true), NEAR);
        assert_eq!(pace(Duration::from_millis(3), false), FAR);
        // Half a second of walking is ten seconds of leaving it alone.
        assert_eq!(pace(Duration::from_millis(500), true), Duration::from_secs(10));
        assert_eq!(pace(Duration::from_millis(500), false), Duration::from_secs(10));
        // And nothing is left unlooked-at for more than a minute.
        assert_eq!(pace(Duration::from_secs(30), true), SLOWEST);
    }

    #[test]
    fn a_change_made_by_something_else_is_heard_once() {
        let folder = std::env::temp_dir().join(format!("gitrust-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        gitrust_core::start::init(&folder, None).unwrap();

        let mut watch = Watch::start(egui::Context::default());
        watch.look_at(Some(&folder), true);
        let hear = |watch: &Watch| {
            for _ in 0..200 {
                let heard = watch.heard();
                if !heard.is_empty() {
                    return heard;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Vec::new()
        };

        // The first look is said, whatever it finds: the window is the one
        // that knows what it is showing.
        let first = hear(&watch);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, folder);

        std::fs::write(folder.join("note.txt"), "written by something else\n").unwrap();
        let second = hear(&watch);
        assert_eq!(second.len(), 1, "the change was not heard");
        assert_eq!(second[0].1.git, first[0].1.git);
        assert_ne!(second[0].1.tree, first[0].1.tree);

        // Told to look nowhere, it says nothing more, whatever happens.
        watch.look_at(None, true);
        std::thread::sleep(Duration::from_millis(100));
        let _ = watch.heard();
        std::fs::write(folder.join("note.txt"), "and again\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(watch.heard().is_empty());

        let _ = std::fs::remove_dir_all(&folder);
    }
}
