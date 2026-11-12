//! A shell running in the window, and everything about it that is not paint.
//!
//! Three things make a terminal and this file holds all three: a
//! pseudo-terminal with a shell on the far end of it, something that reads
//! what the shell writes and keeps the grid of letters that writing
//! describes, and the bytes a key turns into on its way back. The painting
//! is next door in `terminal_view`, and knows nothing about processes.
//!
//! The first two are `alacritty_terminal`, the inside of the terminal of
//! that name, and they are the one part of this program that is somebody
//! else's and was never going to be otherwise. What a shell writes is
//! escape sequences forty years deep -- the alternate screen `vim` draws
//! on, scroll regions, the eleven ways of saying a colour -- and a reader
//! of them that is nearly right is a terminal where `git log` works and
//! `git rebase -i` does not. It also brings the pseudo-terminal itself on
//! all three systems, which on Windows is not a small thing.
//!
//! One session per tab, started the first time its panel is opened and in
//! the folder of that tab's repository. Putting the panel away does not
//! stop it; killing it does, and so does the shell exiting.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Rgb};
use eframe::egui::{self, Color32, Key, Modifiers};
use gitrust_core::Caret;

use crate::theme;

/// How many lines that have scrolled off the top are kept.
const HISTORY: usize = 5_000;

/// How long the shell has to say nothing before it is taken to have
/// finished saying it.
const QUIET: Duration = Duration::from_millis(400);

/// What a session is started with.
pub struct Start {
    /// The program and what it is handed. `None` is the shell this account
    /// logs in with.
    pub program: Option<(String, Vec<String>)>,
    /// Where it starts. A folder that has gone is not an error: the shell
    /// starts wherever the window was started.
    pub folder: Option<PathBuf>,
    /// What is put in its environment on top of the window's own.
    pub env: Vec<(String, String)>,
    pub caret: Caret,
}

/// Hands on what the terminal has to say to the window, and wakes the
/// window up to hear it. It is called from the thread that reads the shell.
#[derive(Clone)]
pub struct Proxy {
    events: Sender<Event>,
    ctx: egui::Context,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        let _ = self.events.send(event);
        self.ctx.request_repaint();
    }
}

/// How many letters fit, which is all the grid wants to know of a size.
struct Room {
    cols: usize,
    rows: usize,
}

impl Dimensions for Room {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// What the shell going quiet means for whoever is watching the repository.
#[derive(Debug, PartialEq, Eq)]
pub enum Quiet {
    /// Nothing has happened since the last look.
    Nothing,
    /// Something is still being said. Ask again after this long.
    Wait(Duration),
    /// Something was said and it has stopped. `asked` is whether a line was
    /// sent in before it: a command, as far as anybody outside can tell.
    Now { asked: bool },
}

/// One shell, and the grid it has written.
pub struct Session {
    term: Arc<FairMutex<Term<Proxy>>>,
    to_shell: EventLoopSender,
    events: Receiver<Event>,
    cols: usize,
    rows: usize,
    cell: (u16, u16),
    caret: Caret,
    title: Option<String>,
    over: bool,
    /// A line went in and nobody has yet looked at what it did.
    asked: bool,
    /// The shell has said something since the last look.
    stirred: bool,
    /// When it last did.
    heard: Instant,
    /// What the painting keeps between frames. It is here because it is
    /// about this shell and has to go when the shell does.
    pub view: crate::terminal_view::Kept,
}

impl Session {
    /// Starts the shell. What comes back when it cannot is what the system
    /// said, which is usually that the program is not there.
    pub fn start(ctx: &egui::Context, start: Start) -> io::Result<Self> {
        let (cols, rows) = (80, 30);
        let size = WindowSize { num_cols: cols as u16, num_lines: rows as u16, cell_width: 8, cell_height: 16 };

        // Said in the environment handed over rather than in the window's
        // own: that one is read by every thread here and by every `git` and
        // `gh` this program runs, none of which is drawing on a terminal.
        let mut env: HashMap<String, String> = HashMap::from([
            ("TERM".to_string(), "xterm-256color".to_string()),
            ("COLORTERM".to_string(), "truecolor".to_string()),
            ("TERM_PROGRAM".to_string(), "gitrust".to_string()),
        ]);
        env.extend(start.env);

        let options = tty::Options {
            shell: start
                .program
                .map(|(program, args)| tty::Shell::new(program, args)),
            working_directory: start.folder.filter(|folder| folder.is_dir()),
            env,
            ..tty::Options::default()
        };
        let pty = tty::new(&options, size, 0)?;

        let (events, heard) = mpsc::channel();
        let proxy = Proxy { events, ctx: ctx.clone() };
        let config = Config {
            scrolling_history: HISTORY,
            default_cursor_style: style_of(start.caret),
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &Room { cols, rows }, proxy.clone())));

        let reading = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let to_shell = reading.channel();
        // Not kept: the thread ends when it is told to or when the shell
        // does, and takes the pseudo-terminal with it either way.
        let _ = reading.spawn();

        Ok(Self {
            term,
            to_shell,
            events: heard,
            cols,
            rows,
            cell: (8, 16),
            caret: start.caret,
            title: None,
            over: false,
            asked: false,
            stirred: false,
            heard: Instant::now(),
            view: crate::terminal_view::Kept::default(),
        })
    }

    /// Hears what the terminal has had to say since the last frame.
    ///
    /// Most of it is the terminal asking for something only a window has:
    /// the clipboard, a colour, its own size in pixels. A program that asks
    /// and gets no answer waits for one, which is how `vim` comes to take a
    /// second to open in a terminal that is nearly finished.
    pub fn pump(&mut self, ctx: &egui::Context) {
        let palette = Palette::current();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Wakeup => {
                    self.stirred = true;
                    self.heard = Instant::now();
                }
                Event::PtyWrite(text) => self.send(text.into_bytes()),
                Event::Title(title) => self.title = Some(title),
                Event::ResetTitle => self.title = None,
                Event::ClipboardStore(_, text) => ctx.copy_text(text),
                Event::ColorRequest(index, answer) => {
                    let colour = self.term.lock().colors()[index]
                        .unwrap_or_else(|| rgb_of(palette.indexed(index)));
                    self.send(answer(colour).into_bytes());
                }
                Event::TextAreaSizeRequest(answer) => {
                    self.send(answer(self.size()).into_bytes());
                }
                Event::Exit | Event::ChildExit(_) => self.over = true,
                // Reading the clipboard for a program that asks is off in
                // the terminal's own settings, so this is never sent; a
                // bell is a noise, and nobody asked the window for one.
                Event::ClipboardLoad(..)
                | Event::Bell
                | Event::MouseCursorDirty
                | Event::CursorBlinkingChange => {}
            }
        }
    }

    /// Whether the shell has gone. There is nothing to do with a session
    /// after that but let go of it.
    pub fn is_over(&self) -> bool {
        self.over
    }

    /// What the shell calls itself just now, when it says.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref().filter(|title| !title.trim().is_empty())
    }

    /// Whether what the shell was saying has stopped, asked once per frame
    /// by whoever wants to look at the repository afterwards.
    ///
    /// Nothing here knows when a command ends: a shell does not say. What it
    /// does is write, and then stop writing, and a prompt is the last thing
    /// it writes. So a burst that has been over for a moment is taken for a
    /// finished command, which is wrong about `sleep 60` and costs one look
    /// at a repository that has not changed.
    pub fn quiet(&mut self) -> Quiet {
        if !self.stirred && !self.asked {
            return Quiet::Nothing;
        }
        let since = self.heard.elapsed();
        if since < QUIET {
            return Quiet::Wait(QUIET - since);
        }
        let asked = std::mem::take(&mut self.asked);
        self.stirred = false;
        Quiet::Now { asked }
    }

    /// Sends bytes to the shell as they are.
    pub fn send(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        // Nothing at all is not nothing to the far end: it reads it as the
        // end of the input.
        if !bytes.is_empty() {
            let _ = self.to_shell.send(Msg::Input(bytes));
        }
    }

    /// Sends what somebody typed, which is the same bytes and two things
    /// more: the view goes back to the bottom, where the typing is, and
    /// what was selected is let go of, because it was selected in a screen
    /// that is about to move.
    pub fn type_in(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if bytes.iter().any(|byte| *byte == b'\r' || *byte == b'\n') {
            self.asked = true;
            self.heard = Instant::now();
        }
        {
            let mut term = self.term.lock();
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        }
        self.send(bytes);
    }

    /// Pastes: the text, wrapped the way a program that asked to be told
    /// has asked, so that a pasted paragraph is a paragraph to it and not
    /// forty lines typed very fast.
    pub fn paste(&mut self, text: &str) {
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        let bytes = match bracketed {
            // The marks themselves are taken out of what is pasted. Text
            // that carries the closing one would otherwise end the paste
            // early and have the rest of itself run as typing.
            true => {
                let clean = text.replace("\x1b[200~", "").replace("\x1b[201~", "");
                format!("\x1b[200~{clean}\x1b[201~").into_bytes()
            }
            // A line ends with a carriage return when it is typed, and a
            // paste nobody asked to be told about is typing.
            false => text.replace("\r\n", "\r").replace('\n', "\r").into_bytes(),
        };
        self.type_in(bytes);
    }

    /// Tells the grid and the shell how many letters there is room for.
    pub fn fit(&mut self, cols: usize, rows: usize, cell: (u16, u16)) {
        if (cols, rows, cell) == (self.cols, self.rows, self.cell) {
            return;
        }
        (self.cols, self.rows, self.cell) = (cols, rows, cell);
        let _ = self.to_shell.send(Msg::Resize(self.size()));
        self.term.lock().resize(Room { cols, rows });
    }

    fn size(&self) -> WindowSize {
        WindowSize {
            num_cols: self.cols as u16,
            num_lines: self.rows as u16,
            cell_width: self.cell.0,
            cell_height: self.cell.1,
        }
    }

    /// Takes on the caret the preferences name, for a shell already open.
    pub fn set_caret(&mut self, caret: Caret) {
        if caret == self.caret {
            return;
        }
        self.caret = caret;
        self.term.lock().set_options(Config {
            scrolling_history: HISTORY,
            default_cursor_style: style_of(caret),
            ..Config::default()
        });
    }

    /// What the program in the terminal has asked of it: which keys it
    /// wants spelt differently, whether it wants the mouse.
    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// The grid, held still for as long as what comes back is kept.
    pub fn grid(&self) -> impl std::ops::DerefMut<Target = Term<Proxy>> + '_ {
        self.term.lock()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The thread reading the shell owns the pseudo-terminal, and closes
        // it on its way out; the shell is told to go when it does.
        let _ = self.to_shell.send(Msg::Shutdown);
    }
}

fn style_of(caret: Caret) -> CursorStyle {
    CursorStyle {
        shape: match caret {
            Caret::Bar => CursorShape::Beam,
            Caret::Block => CursorShape::Block,
            Caret::Underline => CursorShape::Underline,
        },
        blinking: false,
    }
}

fn rgb_of(colour: Color32) -> Rgb {
    Rgb { r: colour.r(), g: colour.g(), b: colour.b() }
}

/// The colours a terminal is drawn in.
///
/// Sixteen of them have names every program agrees on, and those come from
/// the theme. The other two hundred and forty are arithmetic: a cube of six
/// steps a side and a ramp of greys, the same in every terminal there is.
pub struct Palette {
    /// What is behind the letters.
    pub ground: Color32,
    /// The letters, when a program has not said.
    pub ink: Color32,
    pub caret: Color32,
    /// What is behind the letters somebody has selected.
    pub selection: Color32,
    ansi: [Color32; 16],
}

impl Palette {
    pub fn current() -> Self {
        let ground = theme::terminal_ground();
        let lane = theme::lane(0);
        Self {
            ground,
            ink: theme::text(),
            caret: lane,
            selection: mix(ground, lane, 0.32),
            ansi: theme::terminal(),
        }
    }

    /// One of the two hundred and fifty-six, or what stands in for the few
    /// named ones past them.
    pub fn indexed(&self, index: usize) -> Color32 {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let step = |n: usize| match n {
                    0 => 0,
                    n => (55 + 40 * n) as u8,
                };
                let index = index - 16;
                Color32::from_rgb(step(index / 36), step(index / 6 % 6), step(index % 6))
            }
            232..=255 => {
                let grey = (8 + 10 * (index - 232)) as u8;
                Color32::from_rgb(grey, grey, grey)
            }
            // Foreground, background and the caret, in the order the
            // terminal numbers them.
            256 => self.ink,
            257 => self.ground,
            258 => self.caret,
            // The dim eight and the dim foreground: the colour itself, let
            // down towards what is behind it.
            259..=266 => mix(self.ground, self.ansi[index - 259], 0.66),
            267 => self.ansi[15],
            _ => mix(self.ground, self.ink, 0.66),
        }
    }
}

/// `part` of the way from one colour to another.
pub fn mix(from: Color32, to: Color32, part: f32) -> Color32 {
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * part).round() as u8;
    Color32::from_rgb(blend(from.r(), to.r()), blend(from.g(), to.g()), blend(from.b(), to.b()))
}

/// What a key is to a shell.
///
/// Only the keys that are not letters: a letter arrives as the text it
/// types, already through the keyboard layout, and is sent as that. What is
/// left is the keys with no letter of their own and the ones held with
/// Control, and each of those is a few bytes agreed on long before there
/// were windows. `None` is a key that says nothing by itself.
pub fn key_bytes(key: Key, held: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    // What xterm adds to a key held with something: one, plus one for
    // Shift, two for Alt and four for Control.
    let extra = 1 + held.shift as u8 + 2 * held.alt as u8 + 4 * held.ctrl as u8;
    let plain = extra == 1;
    let app = mode.contains(TermMode::APP_CURSOR);

    // An arrow, Home or End: one letter, said three ways.
    let lettered = |letter: char| match (plain, app) {
        (true, true) => format!("\x1bO{letter}"),
        (true, false) => format!("\x1b[{letter}"),
        (false, _) => format!("\x1b[1;{extra}{letter}"),
    };
    // A key that is a number and a tilde.
    let numbered = |number: u8| match plain {
        true => format!("\x1b[{number}~"),
        false => format!("\x1b[{number};{extra}~"),
    };
    // Alt before a key is Escape before its bytes.
    let met = |bytes: &[u8]| {
        let mut all = Vec::with_capacity(bytes.len() + 1);
        if held.alt {
            all.push(0x1b);
        }
        all.extend_from_slice(bytes);
        all
    };

    let text = match key {
        Key::ArrowUp => lettered('A'),
        Key::ArrowDown => lettered('B'),
        Key::ArrowRight => lettered('C'),
        Key::ArrowLeft => lettered('D'),
        Key::Home => lettered('H'),
        Key::End => lettered('F'),
        Key::Insert => numbered(2),
        Key::Delete => numbered(3),
        Key::PageUp => numbered(5),
        Key::PageDown => numbered(6),
        Key::F1 | Key::F2 | Key::F3 | Key::F4 => {
            let letter = match key {
                Key::F1 => 'P',
                Key::F2 => 'Q',
                Key::F3 => 'R',
                _ => 'S',
            };
            match plain {
                true => format!("\x1bO{letter}"),
                false => format!("\x1b[1;{extra}{letter}"),
            }
        }
        Key::F5 => numbered(15),
        Key::F6 => numbered(17),
        Key::F7 => numbered(18),
        Key::F8 => numbered(19),
        Key::F9 => numbered(20),
        Key::F10 => numbered(21),
        Key::F11 => numbered(23),
        Key::F12 => numbered(24),

        Key::Enter => return Some(met(b"\r")),
        Key::Escape => return Some(met(b"\x1b")),
        Key::Tab if held.shift => return Some(b"\x1b[Z".to_vec()),
        Key::Tab => return Some(met(b"\t")),
        // Rubout, which is what a terminal has sent for this key since
        // the key stopped meaning "move left". With Control it is the
        // older one, which shells read as "the whole word".
        Key::Backspace if held.ctrl => return Some(met(b"\x08")),
        Key::Backspace => return Some(met(b"\x7f")),

        // Held with Control, a key is itself with its top bits taken off:
        // that is what Control was. A letter without it is text, and comes
        // by the other road.
        _ if held.ctrl => {
            let byte = match key {
                Key::Space | Key::Num2 => 0x00,
                Key::OpenBracket | Key::Num3 => 0x1b,
                Key::Backslash | Key::Num4 => 0x1c,
                Key::CloseBracket | Key::Num5 => 0x1d,
                Key::Num6 => 0x1e,
                Key::Slash | Key::Minus | Key::Num7 => 0x1f,
                Key::Num8 => 0x7f,
                key => {
                    let name = key.name();
                    match name.as_bytes() {
                        [letter @ b'A'..=b'Z'] => letter & 0x1f,
                        _ => return None,
                    }
                }
            };
            return Some(met(&[byte]));
        }
        _ => return None,
    };
    Some(text.into_bytes())
}

/// A string as one word of a shell command, whatever is in it.
pub fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The command that opens a shell in a checkout on another machine.
///
/// It is `ssh`, the program, in a terminal on this machine, and not the
/// connection the tab already has. That one is a library's and is busy
/// being a Git client; this one has to be somebody typing, with their own
/// `known_hosts` and their own agent and a passphrase asked for where they
/// can answer it. The key is the profile's when it has one, and no other
/// is offered then: which key opens the door is which client you are.
pub fn remote_shell(
    host: &str,
    user: &str,
    port: u16,
    folder: &str,
    key: Option<&std::path::Path>,
    who: Option<(&str, &str)>,
) -> (String, Vec<String>) {
    let mut args = vec!["-t".to_string(), "-p".to_string(), port.to_string()];
    if let Some(key) = key {
        args.extend(["-i".to_string(), key.display().to_string()]);
        args.extend(["-o".to_string(), "IdentitiesOnly=yes".to_string()]);
    }
    args.push(format!("{user}@{host}"));

    // The name goes along the way it goes along with every Git command
    // this program runs there: said for this one shell, and written into
    // nothing on that machine.
    let mut there = format!("cd -- {} && ", quoted(folder));
    if let Some((name, email)) = who {
        for (variable, value) in [
            ("GIT_AUTHOR_NAME", name),
            ("GIT_AUTHOR_EMAIL", email),
            ("GIT_COMMITTER_NAME", name),
            ("GIT_COMMITTER_EMAIL", email),
        ] {
            there.push_str(&format!("export {variable}={} && ", quoted(value)));
        }
    }
    there.push_str("exec \"${SHELL:-/bin/sh}\" -l");
    args.push(there);
    ("ssh".to_string(), args)
}

/// What a profile puts in a shell's environment on this machine.
///
/// The same two things it decides everywhere else. A commit made by typing
/// `git commit` in this terminal is made in this window, and one that went
/// out under another client's name because it was typed rather than
/// clicked would be the mistake profiles are here to stop.
pub fn profile_env(
    who: Option<(&str, &str)>,
    key: Option<&std::path::Path>,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some((name, email)) = who {
        for (variable, value) in [
            ("GIT_AUTHOR_NAME", name),
            ("GIT_AUTHOR_EMAIL", email),
            ("GIT_COMMITTER_NAME", name),
            ("GIT_COMMITTER_EMAIL", email),
        ] {
            env.push((variable.to_string(), value.to_string()));
        }
    }
    if let Some(key) = key {
        env.push((
            "GIT_SSH_COMMAND".to_string(),
            format!("ssh -i {} -o IdentitiesOnly=yes", quoted(&key.display().to_string())),
        ));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(key: Key, held: Modifiers, mode: TermMode) -> Option<String> {
        key_bytes(key, held, mode).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    #[test]
    fn an_arrow_is_spelt_the_way_the_program_asked() {
        let none = TermMode::empty();
        assert_eq!(bytes(Key::ArrowUp, Modifiers::NONE, none).as_deref(), Some("\x1b[A"));
        // `vim` and `less` ask for the other spelling, and move by nothing
        // at all when they are sent the first.
        assert_eq!(
            bytes(Key::ArrowUp, Modifiers::NONE, TermMode::APP_CURSOR).as_deref(),
            Some("\x1bOA")
        );
        // Held with something, it is the long form whichever was asked for.
        assert_eq!(bytes(Key::ArrowLeft, Modifiers::CTRL, none).as_deref(), Some("\x1b[1;5D"));
        assert_eq!(
            bytes(Key::ArrowRight, Modifiers::SHIFT | Modifiers::ALT, TermMode::APP_CURSOR)
                .as_deref(),
            Some("\x1b[1;4C")
        );
    }

    #[test]
    fn control_takes_the_top_off_a_letter() {
        let none = TermMode::empty();
        assert_eq!(key_bytes(Key::D, Modifiers::CTRL, none), Some(vec![0x04]));
        assert_eq!(key_bytes(Key::Z, Modifiers::CTRL, none), Some(vec![0x1a]));
        assert_eq!(key_bytes(Key::Space, Modifiers::CTRL, none), Some(vec![0x00]));
        // And Alt puts an Escape in front of whatever that came to.
        assert_eq!(
            key_bytes(Key::B, Modifiers::CTRL | Modifiers::ALT, none),
            Some(vec![0x1b, 0x02])
        );
        // A letter on its own is text, and is not this function's to send.
        assert_eq!(key_bytes(Key::D, Modifiers::NONE, none), None);
    }

    #[test]
    fn the_keys_with_no_letter_say_what_they_always_said() {
        let none = TermMode::empty();
        assert_eq!(key_bytes(Key::Enter, Modifiers::NONE, none), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::Backspace, Modifiers::NONE, none), Some(vec![0x7f]));
        assert_eq!(key_bytes(Key::Backspace, Modifiers::ALT, none), Some(vec![0x1b, 0x7f]));
        assert_eq!(key_bytes(Key::Tab, Modifiers::SHIFT, none), Some(b"\x1b[Z".to_vec()));
        assert_eq!(bytes(Key::Delete, Modifiers::NONE, none).as_deref(), Some("\x1b[3~"));
        assert_eq!(bytes(Key::PageDown, Modifiers::NONE, none).as_deref(), Some("\x1b[6~"));
        assert_eq!(bytes(Key::F1, Modifiers::NONE, none).as_deref(), Some("\x1bOP"));
        assert_eq!(bytes(Key::F5, Modifiers::NONE, none).as_deref(), Some("\x1b[15~"));
        assert_eq!(bytes(Key::F5, Modifiers::SHIFT, none).as_deref(), Some("\x1b[15;2~"));
    }

    #[test]
    fn the_cube_and_the_greys_are_the_ones_every_terminal_has() {
        let palette = Palette::current();
        assert_eq!(palette.indexed(16), Color32::from_rgb(0, 0, 0));
        assert_eq!(palette.indexed(196), Color32::from_rgb(255, 0, 0));
        assert_eq!(palette.indexed(231), Color32::from_rgb(255, 255, 255));
        assert_eq!(palette.indexed(232), Color32::from_rgb(8, 8, 8));
        assert_eq!(palette.indexed(255), Color32::from_rgb(238, 238, 238));
    }

    #[test]
    fn a_folder_with_a_quote_in_it_is_still_one_word() {
        assert_eq!(quoted("/srv/it's here"), "'/srv/it'\\''s here'");
    }

    #[test]
    fn a_shell_on_another_machine_opens_in_the_checkout_as_the_profile() {
        let key = std::path::Path::new("/home/someone/.ssh/acme");
        let (program, args) = remote_shell(
            "host.example",
            "dev",
            2222,
            "/srv/my project",
            Some(key),
            Some(("Some One", "one@acme.example")),
        );
        assert_eq!(program, "ssh");
        assert_eq!(
            &args[..7],
            ["-t", "-p", "2222", "-i", "/home/someone/.ssh/acme", "-o", "IdentitiesOnly=yes"]
        );
        assert_eq!(args[7], "dev@host.example");
        assert!(args[8].starts_with("cd -- '/srv/my project' && "));
        assert!(args[8].contains("export GIT_AUTHOR_NAME='Some One' && "));
        assert!(args[8].ends_with("exec \"${SHELL:-/bin/sh}\" -l"));

        // With no profile there is nothing to say but where.
        let (_, args) = remote_shell("host.example", "dev", 22, "/srv/p", None, None);
        assert_eq!(args, ["-t", "-p", "22", "dev@host.example", "cd -- '/srv/p' && exec \"${SHELL:-/bin/sh}\" -l"]);
    }

    #[test]
    fn a_profile_is_who_a_typed_commit_is_from() {
        let env = profile_env(
            Some(("Some One", "one@acme.example")),
            Some(std::path::Path::new("/keys/acme key")),
        );
        assert!(env.contains(&("GIT_COMMITTER_EMAIL".to_string(), "one@acme.example".to_string())));
        assert!(env.contains(&(
            "GIT_SSH_COMMAND".to_string(),
            "ssh -i '/keys/acme key' -o IdentitiesOnly=yes".to_string()
        )));
        // A profile that says neither leaves the shell as it would have been.
        assert!(profile_env(None, None).is_empty());
    }
}
