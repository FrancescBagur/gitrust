//! The terminal as it is seen and touched: the grid painted, the keyboard
//! and the mouse heard on the shell's behalf, and the bar above it.
//!
//! The grid is letters in boxes, and the boxes are the point. Text here is
//! not laid out, it is placed: a letter goes in the column the shell put
//! it in, whatever its neighbours are, because a program that draws a
//! table is counting columns and has no way to know how wide this window
//! thinks a letter is. So a run of letters that the mono face draws at its
//! own width is painted as one piece of text, and anything else -- a
//! symbol borrowed from another font, a letter two columns wide -- is
//! painted alone, in its own box, where it cannot push the rest along.
//!
//! Like every other view it changes nothing but what it is handed, which
//! here is the session: the keys go into it and the grid comes out of it.
//! What it cannot do itself -- fold the panel, end the shell, open a
//! browser -- it gives back as an [`Act`].

use std::collections::HashMap;
use std::ops::Range;

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{Color, CursorShape};
use eframe::egui::{
    self,
    emath::GuiRounding,
    text::{LayoutJob, TextFormat},
    Color32, Event, EventFilter, PointerButton, Sense, Stroke,
};
use gitrust_core::General;

use crate::terminal::{key_bytes, mix, Palette, Session};
use crate::theme;

/// What has been asked of the panel that the panel cannot do itself.
pub enum Act {
    /// Fold it down to its bar.
    Minimize,
    /// Open it up again.
    Expand,
    /// End the shell and put the panel away.
    Kill,
    /// Put the panel away and leave the shell running.
    Hide,
    /// Open an address somebody clicked on.
    Open(String),
}

/// How tall the bar above the terminal is.
pub const BAR: f32 = 26.0;

/// The room left between the edge of the panel and the letters.
const PAD: egui::Vec2 = egui::vec2(10.0, 6.0);

/// How wide the handle for scrolling back is.
const SCROLLER: f32 = 9.0;

/// How much of itself the terminal keeps while the keyboard is elsewhere.
const DIMMED: f32 = 0.55;

/// How wide the menu behind a right click is. Told, not asked for, like
/// every other menu here.
const MENU: f32 = 210.0;

/// What the painting remembers between frames about one shell.
#[derive(Default)]
pub struct Kept {
    /// Take the keyboard on the next frame: the panel has just been opened,
    /// and it was opened to be typed into.
    pub wants_focus: bool,
    had_focus: bool,
    /// The part of a turn of the wheel that was less than a line.
    wheel: f32,
    /// Which letters the mono face draws at its own width, found out once
    /// each, and the size they were asked about at.
    plain: HashMap<char, bool>,
    plain_size: f32,
    /// The next paste to arrive was asked for from the menu, and is a
    /// paste whatever is held down when it comes.
    paste_next: bool,
    /// The mouse button a program in the terminal is being told is down,
    /// and the box the pointer was last reported in.
    held: Option<u8>,
    reported: Option<(usize, usize)>,
}

/// Where everything is, worked out once a frame.
struct Layout {
    origin: egui::Pos2,
    cell: egui::Vec2,
    cols: usize,
    rows: usize,
}

impl Layout {
    /// The box a point on the screen is in, kept inside the grid, and
    /// which half of the box.
    fn at(&self, pos: egui::Pos2) -> (usize, usize, Side) {
        let across = ((pos.x - self.origin.x) / self.cell.x).max(0.0);
        let down = ((pos.y - self.origin.y) / self.cell.y).max(0.0);
        let col = (across as usize).min(self.cols - 1);
        let row = (down as usize).min(self.rows - 1);
        let side = match across.fract() < 0.5 || across as usize >= self.cols {
            true if (across as usize) < self.cols => Side::Left,
            _ => Side::Right,
        };
        (col, row, side)
    }

    fn corner(&self, col: usize, row: usize) -> egui::Pos2 {
        self.origin + egui::vec2(col as f32 * self.cell.x, row as f32 * self.cell.y)
    }
}

/// A run of boxes behind which something other than the ground is painted.
struct Band {
    row: usize,
    from: usize,
    to: usize,
    colour: Color32,
}

/// A piece of text and where it starts.
struct Piece {
    row: usize,
    col: usize,
    text: String,
    /// How many boxes it covers, for the lines drawn under and through it.
    wide: usize,
    ink: Color32,
    bold: bool,
    italic: bool,
    under: Option<Color32>,
    struck: bool,
    /// What is behind it, for the letters that are drawn rather than set.
    ground: Color32,
}

/// Paints the terminal into whatever room is left and carries what is
/// typed and clicked into the session.
pub fn show(ui: &mut egui::Ui, session: &mut Session, general: &General) -> Option<Act> {
    let mut act = None;
    session.set_caret(general.terminal_caret);

    let palette = Palette::current();
    let size = general.terminal_font as f32;
    let font = theme::mono(size);
    let (letter, natural) = ui.fonts(|fonts| (fonts.glyph_width(&font, 'M'), fonts.row_height(&font)));
    let cell = egui::vec2(letter, (natural * general.terminal_line as f32 / 100.0).ceil());

    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let inner = rect.shrink2(PAD);
    let layout = Layout {
        origin: inner.min.round_to_pixels(ui.pixels_per_point()),
        cell,
        cols: (((inner.width() - SCROLLER) / cell.x).floor() as usize).max(2),
        rows: ((inner.height() / cell.y).floor() as usize).max(1),
    };
    // A panel on its way open is a few points tall for a frame, and telling
    // the shell it has one line makes it redraw its prompt for one.
    if inner.width() > cell.x * 12.0 && inner.height() >= cell.y * 2.0 {
        session.fit(layout.cols, layout.rows, (cell.x.round() as u16, cell.y as u16));
    }

    if session.view.plain_size != size {
        session.view.plain.clear();
        session.view.plain_size = size;
    }

    // ── The keyboard ──────────────────────────────────────────────────────
    let pressed = response.drag_started()
        || (response.clicked() && ui.input(|input| input.pointer.any_released()));
    if std::mem::take(&mut session.view.wants_focus) || pressed {
        response.request_focus();
    }
    let focused = response.has_focus() && ui.input(|input| input.focused);
    let mode = session.mode();
    if focused {
        // Tab, the arrows and Escape are keys to a shell before they are
        // ways of moving round a window.
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
            );
        });
        typing(ui, session, mode);
    }
    if focused != session.view.had_focus {
        session.view.had_focus = focused;
        session.grid().is_focused = focused;
        // A program that asked to be told when it is being looked at.
        if mode.contains(TermMode::FOCUS_IN_OUT) {
            session.send(match focused {
                true => b"\x1b[I".as_slice(),
                false => b"\x1b[O".as_slice(),
            });
        }
    }

    // ── The mouse ─────────────────────────────────────────────────────────
    // A program that asked for the mouse gets it, and Shift is how to keep
    // it from that program: the way every terminal that does this does it.
    let shift = ui.input(|input| input.modifiers.shift);
    let reporting = mode.intersects(TermMode::MOUSE_MODE) && !shift;
    let lines = wheel(ui, session, &response, cell.y);
    if reporting {
        report(ui, session, &response, &layout, mode, lines);
    } else {
        session.view.held = None;
        scroll(session, mode, lines);
        if let Some(address) = select(ui, session, &response, &layout) {
            act = Some(Act::Open(address));
        }
        response.context_menu(|ui| {
            if let Some(chosen) = menu(ui, session) {
                act = Some(chosen);
            }
        });
    }

    // ── The paint ─────────────────────────────────────────────────────────
    let dim = general.terminal_dim && !focused;
    let hover = response.hover_pos().filter(|_| !reporting);
    let link = paint(ui, session, &layout, rect, &palette, &font, natural, focused, dim, hover);
    if link {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if response.hovered() && !reporting {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    scroller(ui, session, rect, response.hovered());

    act
}

/// Carries this frame's keys into the shell, and takes them out of the
/// frame: with the keyboard in the terminal, Control and F is the shell's,
/// and the search box over the graph is not to open because of it.
fn typing(ui: &mut egui::Ui, session: &mut Session, mode: TermMode) {
    let mac = cfg!(target_os = "macos");
    let events = ui.input_mut(|input| {
        let mut taken = Vec::new();
        input.events.retain(|event| {
            let ours = matches!(
                event,
                Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_)
            );
            if ours {
                taken.push(event.clone());
            }
            !ours
        });
        taken
    });
    let held = ui.input(|input| input.modifiers);

    for event in events {
        match event {
            // Alt and a letter is Escape and then the letter: what `bash`
            // calls Meta. Not on a Mac, where that key types the letters
            // the keyboard has no key for.
            Event::Text(text) => {
                let mut bytes = Vec::with_capacity(text.len() + 1);
                if held.alt && !mac {
                    bytes.push(0x1b);
                }
                bytes.extend_from_slice(text.as_bytes());
                session.type_in(bytes);
            }
            Event::Key { key, pressed: true, modifiers, .. } => {
                // Shift and a paging key is the window's and not the
                // shell's: it is how to read back without a mouse. Not
                // while a program has the whole screen, which has no back.
                if modifiers.shift && !modifiers.ctrl && !mode.contains(TermMode::ALT_SCREEN) {
                    let turn = match key {
                        egui::Key::PageUp => Some(Scroll::PageUp),
                        egui::Key::PageDown => Some(Scroll::PageDown),
                        egui::Key::Home => Some(Scroll::Top),
                        egui::Key::End => Some(Scroll::Bottom),
                        _ => None,
                    };
                    if let Some(turn) = turn {
                        session.grid().scroll_display(turn);
                        continue;
                    }
                }
                if let Some(bytes) = key_bytes(key, modifiers, mode) {
                    session.type_in(bytes);
                }
            }
            // Control and C is two things, and which one is whether
            // anything is selected: with a selection it is "copy", as in
            // every window there is, and without one it is what it has
            // always been to a shell. The selection goes once it is
            // copied, so that the second press is the interrupt.
            Event::Copy => {
                let chosen = session.grid().selection_to_string().filter(|text| !text.is_empty());
                match chosen {
                    Some(text) => {
                        ui.ctx().copy_text(text);
                        session.grid().selection = None;
                    }
                    None if !held.shift && !mac => session.type_in(vec![0x03]),
                    None => {}
                }
            }
            Event::Cut if !mac => session.type_in(vec![0x18]),
            // Control and V on its own belongs to the shell too, where it
            // means "the next key, as it is". Pasting is that with Shift,
            // or Shift and Insert, or the menu.
            Event::Paste(text) => {
                let asked = std::mem::take(&mut session.view.paste_next);
                match asked || held.shift || mac {
                    true => session.paste(&text),
                    false => session.type_in(vec![0x16]),
                }
            }
            _ => {}
        }
    }
}

/// How many whole lines the wheel has turned over the terminal this frame,
/// upwards positive.
fn wheel(ui: &egui::Ui, session: &mut Session, response: &egui::Response, line: f32) -> i32 {
    if !response.contains_pointer() {
        session.view.wheel = 0.0;
        return 0;
    }
    session.view.wheel += ui.input(|input| input.smooth_scroll_delta.y) / line;
    let whole = session.view.wheel.trunc();
    session.view.wheel -= whole;
    whole as i32
}

/// What the wheel does when no program has asked for the mouse.
fn scroll(session: &mut Session, mode: TermMode, lines: i32) {
    if lines == 0 {
        return;
    }
    // A program with the whole screen has nothing above it to scroll back
    // to, so the wheel is its arrow keys: that is how `less` and `man`
    // scroll in a terminal without either knowing what a wheel is.
    if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
        let key = match lines > 0 {
            true => egui::Key::ArrowUp,
            false => egui::Key::ArrowDown,
        };
        if let Some(bytes) = key_bytes(key, egui::Modifiers::NONE, mode) {
            session.send(bytes.repeat(lines.unsigned_abs() as usize));
        }
        return;
    }
    session.grid().scroll_display(Scroll::Delta(lines));
}

/// Tells a program that asked for the mouse what the mouse is doing.
fn report(
    ui: &egui::Ui,
    session: &mut Session,
    response: &egui::Response,
    layout: &Layout,
    mode: TermMode,
    lines: i32,
) {
    let held = ui.input(|input| input.modifiers);
    let extra = 8 * held.alt as u8 + 16 * held.ctrl as u8;
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let say = |session: &Session, code: u8, col: usize, row: usize, down: bool| {
        if sgr {
            let end = if down { 'M' } else { 'm' };
            session.send(format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes());
        } else if col < 222 && row < 222 {
            // The old way has no "up" for a button: there is one release,
            // and it is button three.
            let code = if down { code } else { 3 + extra };
            session.send(vec![0x1b, b'[', b'M', 32 + code, 33 + col as u8, 33 + row as u8]);
        }
    };

    let over = response.contains_pointer();
    let events = ui.input(|input| input.events.clone());
    for event in events {
        match event {
            Event::PointerButton { pos, button, pressed, .. } => {
                let code = match button {
                    PointerButton::Primary => 0,
                    PointerButton::Middle => 1,
                    PointerButton::Secondary => 2,
                    _ => continue,
                };
                // A press counts where it lands; a release counts wherever
                // it lands, or the program is left holding a button.
                if (pressed && !over) || (!pressed && session.view.held != Some(code)) {
                    continue;
                }
                let (col, row, _) = layout.at(pos);
                session.view.held = pressed.then_some(code);
                session.view.reported = Some((col, row));
                say(session, code + extra, col, row, pressed);
            }
            Event::PointerMoved(pos) => {
                let wanted = match session.view.held {
                    Some(_) => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
                    None => over && mode.contains(TermMode::MOUSE_MOTION),
                };
                let (col, row, _) = layout.at(pos);
                if wanted && session.view.reported != Some((col, row)) {
                    session.view.reported = Some((col, row));
                    say(session, 32 + session.view.held.unwrap_or(3) + extra, col, row, true);
                }
            }
            _ => {}
        }
    }

    if lines != 0 {
        if let Some(pos) = response.hover_pos() {
            let (col, row, _) = layout.at(pos);
            let code = if lines > 0 { 64 } else { 65 };
            for _ in 0..lines.unsigned_abs() {
                say(session, code + extra, col, row, true);
            }
        }
    }
}

/// Selecting with the mouse, and the click that is not a selection.
///
/// Gives back an address when what was clicked was one.
fn select(
    ui: &egui::Ui,
    session: &mut Session,
    response: &egui::Response,
    layout: &Layout,
) -> Option<String> {
    let mut term = session.grid();
    let back = term.grid().display_offset() as i32;
    let spot = |pos: egui::Pos2| {
        let (col, row, side) = layout.at(pos);
        (Point::new(Line(row as i32 - back), Column(col)), side)
    };
    let primary = PointerButton::Primary;

    if response.drag_started_by(primary) {
        if let Some(pos) = ui.input(|input| input.pointer.press_origin()) {
            let (point, side) = spot(pos);
            // Held with Control it is a rectangle: a column out of a table.
            let kind = match ui.input(|input| input.modifiers.ctrl) {
                true => SelectionType::Block,
                false => SelectionType::Simple,
            };
            term.selection = Some(Selection::new(kind, point, side));
        }
    }
    if response.dragged_by(primary) {
        if let Some(pos) = response.interact_pointer_pos() {
            // Past the top or the bottom the view follows, a line a frame,
            // so that a selection can be longer than the panel is tall.
            let rect = response.rect;
            if pos.y < rect.top() {
                term.scroll_display(Scroll::Delta(1));
                ui.ctx().request_repaint();
            } else if pos.y > rect.bottom() {
                term.scroll_display(Scroll::Delta(-1));
                ui.ctx().request_repaint();
            }
            let back = term.grid().display_offset() as i32;
            let (col, row, side) = layout.at(pos);
            let point = Point::new(Line(row as i32 - back), Column(col));
            if let Some(selection) = &mut term.selection {
                selection.update(point, side);
            }
        }
    }

    // A key held down with the keyboard in here is also a "click", as far
    // as the response can tell: that is how a focused button is pressed
    // with Space. A real one let go of a button this frame.
    let let_go = ui.input(|input| input.pointer.primary_released());
    let pos = response.interact_pointer_pos().filter(|_| let_go)?;
    let (point, side) = spot(pos);
    if response.triple_clicked() {
        term.selection = Some(Selection::new(SelectionType::Lines, point, side));
    } else if response.double_clicked() {
        term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
    } else if response.clicked() {
        term.selection = None;
        return address_at(&term, point, layout.cols).map(|(address, _)| address);
    }
    None
}

/// The address under a box, when there is one, and the columns it runs
/// across on that line.
///
/// Two kinds. A program can say outright that a stretch of text is a link
/// and where it goes; failing that, text that starts like an address is
/// taken for one, up to the first thing an address cannot have in it.
fn address_at<T>(
    term: &alacritty_terminal::Term<T>,
    point: Point,
    cols: usize,
) -> Option<(String, Range<usize>)> {
    let row = &term.grid()[point.line];
    if let Some(link) = row[point.column].hyperlink() {
        let same = |col: usize| row[Column(col)].hyperlink().as_ref() == Some(&link);
        let from = (0..=point.column.0).rev().take_while(|col| same(*col)).last()?;
        let to = (point.column.0..cols).take_while(|col| same(*col)).last()? + 1;
        return Some((link.uri().to_string(), from..to));
    }
    let letters: Vec<char> = (0..cols).map(|col| row[Column(col)].c).collect();
    let span = address_in(&letters, point.column.0)?;
    Some((letters[span.clone()].iter().collect(), span))
}

/// Where in a line of letters the web address covering `col` is.
fn address_in(letters: &[char], col: usize) -> Option<Range<usize>> {
    let starts = |at: usize, word: &str| {
        letters[at..].iter().take(word.len()).copied().eq(word.chars())
    };
    let mut at = 0;
    while at < letters.len() {
        if !(starts(at, "https://") || starts(at, "http://")) {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < letters.len()
            && !letters[end].is_whitespace()
            && !matches!(letters[end], '\0' | '"' | '\'' | '<' | '>' | '`' | '│' | '|')
        {
            end += 1;
        }
        // What an address ends with in a sentence is the sentence's.
        while end > at && matches!(letters[end - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
            end -= 1;
        }
        if (at..end).contains(&col) {
            return Some(at..end);
        }
        at = end.max(at + 1);
    }
    None
}

/// The menu behind a right click.
fn menu(ui: &mut egui::Ui, session: &mut Session) -> Option<Act> {
    ui.set_width(MENU);
    let mac = cfg!(target_os = "macos");
    let mut act = None;
    let item = |ui: &mut egui::Ui, name: &str, keys: &str, enabled: bool| {
        let button = egui::Button::new(name).shortcut_text(
            egui::RichText::new(keys).font(theme::body(11.0)).color(theme::faint()),
        );
        let pressed = ui.add_enabled(enabled, button).clicked();
        if pressed {
            ui.close_menu();
        }
        pressed
    };

    let chosen = session.grid().selection_to_string().filter(|text| !text.is_empty());
    if item(ui, "Copy", if mac { "Cmd+C" } else { "Ctrl+C" }, chosen.is_some()) {
        if let Some(text) = chosen {
            ui.ctx().copy_text(text);
        }
    }
    if item(ui, "Paste", if mac { "Cmd+V" } else { "Ctrl+Shift+V" }, true) {
        // The clipboard is the window's to read, and it hands over what it
        // read as a paste on the next frame.
        session.view.paste_next = true;
        session.view.wants_focus = true;
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
    }
    if item(ui, "Select All", "", true) {
        let mut term = session.grid();
        let (top, bottom, last) =
            (term.grid().topmost_line(), term.grid().bottommost_line(), term.grid().last_column());
        let mut all = Selection::new(SelectionType::Simple, Point::new(top, Column(0)), Side::Left);
        all.update(Point::new(bottom, last), Side::Right);
        term.selection = Some(all);
    }
    ui.separator();
    if item(ui, "Delete Line", "Ctrl+U", true) {
        session.type_in(vec![0x15]);
    }
    if item(ui, "Clear", "Ctrl+L", true) {
        // What has scrolled off goes too, which the key alone leaves.
        session.grid().grid_mut().clear_history();
        session.type_in(vec![0x0c]);
    }
    ui.separator();
    if item(ui, "Minimize Terminal", "", true) {
        act = Some(Act::Minimize);
    }
    if item(ui, "Kill Terminal", "", true) {
        act = Some(Act::Kill);
    }
    if item(ui, "Hide Terminal", "", true) {
        act = Some(Act::Hide);
    }
    act
}

/// Paints the grid. Says whether the pointer is over an address.
#[allow(clippy::too_many_arguments)]
fn paint(
    ui: &egui::Ui,
    session: &mut Session,
    layout: &Layout,
    rect: egui::Rect,
    palette: &Palette,
    font: &egui::FontId,
    natural: f32,
    focused: bool,
    dim: bool,
    hover: Option<egui::Pos2>,
) -> bool {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, palette.ground);

    // Taken out for the length of the painting and put back: the grid is
    // borrowed from the session the whole time, and this is the session's.
    let mut plain = std::mem::take(&mut session.view.plain);
    let mut is_plain = |letter: char| match letter {
        ' '..='~' => true,
        // The frames and the blocks are drawn, each in its own box.
        '\u{2500}'..='\u{259f}' => false,
        _ => *plain.entry(letter).or_insert_with(|| {
            let width = ui.fonts(|fonts| fonts.glyph_width(font, letter));
            (width - layout.cell.x).abs() < 0.01
        }),
    };

    let term = session.grid();
    let content = term.renderable_content();
    let back = content.display_offset as i32;
    let quiet = |colour: Color32| match dim {
        true => mix(palette.ground, colour, DIMMED),
        false => colour,
    };
    let named = |colour: Color, bold: bool| match colour {
        Color::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        Color::Indexed(index) => match content.colors[index as usize] {
            Some(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
            None => palette.indexed(index as usize),
        },
        Color::Named(name) => {
            // Bold in one of the eight is the bright one of it, which is
            // what bold was before there were bold faces to draw it in.
            let name = match (bold, name as usize) {
                (true, 0..=7) => name.to_bright(),
                _ => name,
            };
            match content.colors[name] {
                Some(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
                None => palette.indexed(name as usize),
            }
        }
    };

    // The address the pointer is over, found before the cells are walked so
    // that they can be underlined as they go by.
    let link = hover.and_then(|pos| {
        let (col, row, _) = layout.at(pos);
        let point = Point::new(Line(row as i32 - back), Column(col));
        let inside = layout.corner(0, 0).x <= pos.x
            && pos.x < layout.corner(layout.cols, 0).x
            && layout.corner(0, 0).y <= pos.y
            && pos.y < layout.corner(0, layout.rows).y;
        address_at(&term, point, layout.cols).filter(|_| inside).map(|(_, span)| (row, span))
    });

    let mut bands: Vec<Band> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    // The run of plain letters being gathered, if one is.
    let mut run: Option<Piece> = None;
    let end_run = |run: &mut Option<Piece>, pieces: &mut Vec<Piece>| {
        if let Some(mut piece) = run.take() {
            // Blank boxes at the end of a run are nothing to draw, unless
            // there is a line under them.
            if piece.under.is_none() && !piece.struck {
                let kept = piece.text.trim_end().len();
                piece.text.truncate(kept);
                piece.wide = piece.text.chars().count();
            }
            if !piece.text.is_empty() {
                pieces.push(piece);
            }
        }
    };

    for indexed in content.display_iter {
        let row = indexed.point.line.0 + back;
        if row < 0 || row as usize >= layout.rows || indexed.point.column.0 >= layout.cols {
            continue;
        }
        let (row, col) = (row as usize, indexed.point.column.0);
        let cell = indexed.cell;
        let flags = cell.flags;
        if col == 0 {
            end_run(&mut run, &mut pieces);
        }
        // The second half of a letter two boxes wide: the first half
        // painted it.
        if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            end_run(&mut run, &mut pieces);
            continue;
        }

        let bold = flags.contains(Flags::BOLD);
        let mut ink = named(cell.fg, bold);
        let mut ground = named(cell.bg, false);
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut ink, &mut ground);
        }
        if flags.contains(Flags::DIM) {
            ink = mix(ground, ink, 0.66);
        }
        let chosen = content.selection.is_some_and(|range| range.contains(indexed.point));
        if chosen {
            ground = palette.selection;
        }
        let wide = if flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };

        if ground != palette.ground {
            let colour = if chosen { ground } else { quiet(ground) };
            match bands.last_mut() {
                Some(band) if band.row == row && band.to == col && band.colour == colour => {
                    band.to = col + wide;
                }
                _ => bands.push(Band { row, from: col, to: col + wide, colour }),
            }
        }

        let linked = link.as_ref().is_some_and(|(at, span)| *at == row && span.contains(&col));
        let under = match (linked, flags.intersects(Flags::ALL_UNDERLINES)) {
            (true, _) => Some(quiet(ink)),
            (_, true) => Some(quiet(cell.underline_color().map_or(ink, |colour| named(colour, false)))),
            _ => None,
        };
        let struck = flags.contains(Flags::STRIKEOUT);
        let italic = flags.contains(Flags::ITALIC);
        let ink = quiet(ink);
        let behind = match chosen {
            true => ground,
            false => quiet(ground),
        };
        let letter = match flags.contains(Flags::HIDDEN) || cell.c == '\0' {
            true => ' ',
            false => cell.c,
        };
        let marks = cell.zerowidth().filter(|marks| !marks.is_empty());

        if marks.is_none() && wide == 1 && is_plain(letter) {
            let fits = run.as_ref().is_some_and(|piece| {
                piece.row == row
                    && piece.col + piece.wide == col
                    && piece.ink == ink
                    && piece.bold == bold
                    && piece.italic == italic
                    && piece.under == under
                    && piece.struck == struck
            });
            if !fits {
                end_run(&mut run, &mut pieces);
                // A run does not begin with nothing.
                if letter == ' ' && under.is_none() && !struck {
                    continue;
                }
                run = Some(Piece {
                    row,
                    col,
                    text: String::new(),
                    wide: 0,
                    ink,
                    bold,
                    italic,
                    under,
                    struck,
                    ground: behind,
                });
            }
            if let Some(piece) = &mut run {
                piece.text.push(letter);
                piece.wide += 1;
            }
            continue;
        }

        // Anything else is painted alone in its own box.
        end_run(&mut run, &mut pieces);
        let mut text = String::from(letter);
        text.extend(marks.into_iter().flatten());
        pieces.push(Piece { row, col, text, wide, ink, bold, italic, under, struck, ground: behind });
    }
    end_run(&mut run, &mut pieces);

    let caret = content.cursor;
    let caret_spot = {
        let row = caret.point.line.0 + back;
        (row >= 0 && (row as usize) < layout.rows && caret.point.column.0 < layout.cols)
            .then(|| (caret.point.column.0, row as usize))
    };
    let under_caret = term.grid()[caret.point].c;
    drop(term);
    session.view.plain = plain;

    for band in &bands {
        painter.rect_filled(
            egui::Rect::from_min_max(
                layout.corner(band.from, band.row),
                layout.corner(band.to, band.row + 1),
            ),
            0,
            band.colour,
        );
    }

    // One pixel of the screen, which is how far over the second striking
    // of a bold letter goes: the mono face here comes in one weight.
    let pixel = 1.0 / ui.ctx().pixels_per_point();
    let lift = ((layout.cell.y - natural) / 2.0).round();
    let write = |text: &str, col: usize, row: usize, ink: Color32, bold: bool, italic: bool| {
        let format = TextFormat { font_id: font.clone(), color: ink, italics: italic, ..Default::default() };
        let galley = painter.layout_job(LayoutJob::single_section(text.to_string(), format));
        let at = layout.corner(col, row) + egui::vec2(0.0, lift);
        if bold {
            painter.galley(at + egui::vec2(pixel, 0.0), galley.clone(), ink);
        }
        painter.galley(at, galley, ink);
    };
    for piece in &pieces {
        let mut letters = piece.text.chars();
        let alone = letters.next().filter(|_| letters.next().is_none());
        let spot = egui::Rect::from_min_max(
            layout.corner(piece.col, piece.row),
            layout.corner(piece.col + 1, piece.row + 1),
        );
        if alone.is_some_and(|letter| drawn(&painter, spot, letter, piece.ink, piece.ground)) {
            continue;
        }
        write(&piece.text, piece.col, piece.row, piece.ink, piece.bold, piece.italic);
        let (left, right) = (
            layout.corner(piece.col, piece.row).x,
            layout.corner(piece.col + piece.wide, piece.row).x,
        );
        let top = layout.corner(piece.col, piece.row).y;
        if let Some(colour) = piece.under {
            let y = (top + lift + natural - 1.5).min(top + layout.cell.y - 1.0).round() + 0.5;
            painter.hline(left..=right, y, Stroke::new(1.0, colour));
        }
        if piece.struck {
            let y = (top + layout.cell.y / 2.0).round() + 0.5;
            painter.hline(left..=right, y, Stroke::new(1.0, piece.ink));
        }
    }

    // The caret, last, over whatever it is on. Without the keyboard it is
    // an outline: the place is still marked, and nothing typed will go
    // there.
    if let Some((col, row)) = caret_spot {
        let colour = quiet(palette.caret);
        let spot = egui::Rect::from_min_max(layout.corner(col, row), layout.corner(col + 1, row + 1));
        let shape = match (caret.shape, focused) {
            (CursorShape::Hidden, _) => CursorShape::Hidden,
            (_, false) => CursorShape::HollowBlock,
            (shape, true) => shape,
        };
        match shape {
            CursorShape::Block => {
                painter.rect_filled(spot, 0, colour);
                if under_caret != ' ' && under_caret != '\0' {
                    write(&under_caret.to_string(), col, row, palette.ground, false, false);
                }
            }
            CursorShape::Beam => {
                let width = (layout.cell.x * 0.16).clamp(1.0, 2.0);
                painter.rect_filled(spot.with_max_x(spot.left() + width), 0, colour);
            }
            CursorShape::Underline => {
                painter.rect_filled(spot.with_min_y(spot.bottom() - 2.0), 0, colour);
            }
            CursorShape::HollowBlock => {
                painter.rect_stroke(spot, 0, Stroke::new(1.0, colour), egui::StrokeKind::Inside);
            }
            CursorShape::Hidden => {}
        }
    }

    link.is_some()
}

// ── Lines and blocks ───────────────────────────────────────────────────────
// The letters programs draw frames with are not letters, and are not
// painted as letters. A font draws `│` to fit its own line, and a terminal's
// line is taller than that by whatever the preferences say, so a frame set
// in the font is a column of dashes with gaps between them. Here each one
// is drawn to the edges of its box: arms out from the middle, as thick as
// the letter says, which is all any of them is.

/// The arms of each letter from U+2500 to U+257F, in the order up, right,
/// down, left: nought for none, one for a light line, two for a heavy one
/// and three for a double. A dashed line is drawn whole, and a rounded
/// corner square: at this size the difference is a pixel.
#[rustfmt::skip]
const ARMS: [[u8; 4]; 128] = [
    [0, 1, 0, 1], [0, 2, 0, 2], [1, 0, 1, 0], [2, 0, 2, 0], [0, 1, 0, 1], [0, 2, 0, 2], [1, 0, 1, 0], [2, 0, 2, 0], // 2500 ─━│┃┄┅┆┇
    [0, 1, 0, 1], [0, 2, 0, 2], [1, 0, 1, 0], [2, 0, 2, 0], [0, 1, 1, 0], [0, 2, 1, 0], [0, 1, 2, 0], [0, 2, 2, 0], // 2508 ┈┉┊┋┌┍┎┏
    [0, 0, 1, 1], [0, 0, 1, 2], [0, 0, 2, 1], [0, 0, 2, 2], [1, 1, 0, 0], [1, 2, 0, 0], [2, 1, 0, 0], [2, 2, 0, 0], // 2510 ┐┑┒┓└┕┖┗
    [1, 0, 0, 1], [1, 0, 0, 2], [2, 0, 0, 1], [2, 0, 0, 2], [1, 1, 1, 0], [1, 2, 1, 0], [2, 1, 1, 0], [1, 1, 2, 0], // 2518 ┘┙┚┛├┝┞┟
    [2, 1, 2, 0], [2, 2, 1, 0], [1, 2, 2, 0], [2, 2, 2, 0], [1, 0, 1, 1], [1, 0, 1, 2], [2, 0, 1, 1], [1, 0, 2, 1], // 2520 ┠┡┢┣┤┥┦┧
    [2, 0, 2, 1], [2, 0, 1, 2], [1, 0, 2, 2], [2, 0, 2, 2], [0, 1, 1, 1], [0, 1, 1, 2], [0, 2, 1, 1], [0, 2, 1, 2], // 2528 ┨┩┪┫┬┭┮┯
    [0, 1, 2, 1], [0, 1, 2, 2], [0, 2, 2, 1], [0, 2, 2, 2], [1, 1, 0, 1], [1, 1, 0, 2], [1, 2, 0, 1], [1, 2, 0, 2], // 2530 ┰┱┲┳┴┵┶┷
    [2, 1, 0, 1], [2, 1, 0, 2], [2, 2, 0, 1], [2, 2, 0, 2], [1, 1, 1, 1], [1, 1, 1, 2], [1, 2, 1, 1], [1, 2, 1, 2], // 2538 ┸┹┺┻┼┽┾┿
    [2, 1, 1, 1], [1, 1, 2, 1], [2, 1, 2, 1], [2, 1, 1, 2], [2, 2, 1, 1], [1, 1, 2, 2], [1, 2, 2, 1], [2, 2, 1, 2], // 2540 ╀╁╂╃╄╅╆╇
    [1, 2, 2, 2], [2, 1, 2, 2], [2, 2, 2, 1], [2, 2, 2, 2], [0, 1, 0, 1], [0, 2, 0, 2], [1, 0, 1, 0], [2, 0, 2, 0], // 2548 ╈╉╊╋╌╍╎╏
    [0, 3, 0, 3], [3, 0, 3, 0], [0, 3, 1, 0], [0, 1, 3, 0], [0, 3, 3, 0], [0, 0, 1, 3], [0, 0, 3, 1], [0, 0, 3, 3], // 2550 ═║╒╓╔╕╖╗
    [1, 3, 0, 0], [3, 1, 0, 0], [3, 3, 0, 0], [1, 0, 0, 3], [3, 0, 0, 1], [3, 0, 0, 3], [1, 3, 1, 0], [3, 1, 3, 0], // 2558 ╘╙╚╛╜╝╞╟
    [3, 3, 3, 0], [1, 0, 1, 3], [3, 0, 3, 1], [3, 0, 3, 3], [0, 3, 1, 3], [0, 1, 3, 1], [0, 3, 3, 3], [1, 3, 0, 3], // 2560 ╠╡╢╣╤╥╦╧
    [3, 1, 0, 1], [3, 3, 0, 3], [1, 3, 1, 3], [3, 1, 3, 1], [3, 3, 3, 3], [0, 1, 1, 0], [0, 0, 1, 1], [1, 0, 0, 1], // 2568 ╨╩╪╫╬╭╮╯
    [1, 1, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 1], [1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], // 2570 ╰╱╲╳╴╵╶╷
    [0, 0, 0, 2], [2, 0, 0, 0], [0, 2, 0, 0], [0, 0, 2, 0], [0, 2, 0, 1], [1, 0, 2, 0], [0, 1, 0, 2], [2, 0, 1, 0], // 2578 ╸╹╺╻╼╽╾╿
];

/// Paints a line-drawing or block letter into its box. Says whether it was
/// one: anything else is the font's to draw.
fn drawn(
    painter: &egui::Painter,
    spot: egui::Rect,
    letter: char,
    ink: Color32,
    ground: Color32,
) -> bool {
    let code = letter as u32;
    if !(0x2500..=0x259f).contains(&code) {
        return false;
    }
    // Everything lands on whole pixels, and a box ends where the next one
    // begins: two boxes that each stop a third of a pixel short of the
    // same edge leave a seam down every frame.
    let scale = painter.pixels_per_point();
    let snap = |value: f32| (value * scale).round() / scale;
    let (left, right) = (snap(spot.left()), snap(spot.right()));
    let (top, bottom) = (snap(spot.top()), snap(spot.bottom()));
    let fill = |x: (f32, f32), y: (f32, f32), colour: Color32| {
        if x.1 > x.0 && y.1 > y.0 {
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(x.0, y.0), egui::pos2(x.1, y.1)),
                0,
                colour,
            );
        }
    };
    let (wide, tall) = (right - left, bottom - top);

    if code >= 0x2580 {
        // Part of the box, filled: `across` and `down` are the part, as
        // fractions of it.
        let part = |across: (f32, f32), down: (f32, f32), colour: Color32| {
            fill(
                (snap(left + wide * across.0), snap(left + wide * across.1)),
                (snap(top + tall * down.0), snap(top + tall * down.1)),
                colour,
            );
        };
        let eighths = |count: u32| count as f32 / 8.0;
        let quarter = |upper: bool, leftward: bool| {
            let across = if leftward { (0.0, 0.5) } else { (0.5, 1.0) };
            let down = if upper { (0.0, 0.5) } else { (0.5, 1.0) };
            part(across, down, ink);
        };
        match code {
            0x2580 => part((0.0, 1.0), (0.0, 0.5), ink),
            0x2581..=0x2588 => part((0.0, 1.0), (1.0 - eighths(code - 0x2580), 1.0), ink),
            0x2589..=0x258f => part((0.0, eighths(0x2590 - code)), (0.0, 1.0), ink),
            0x2590 => part((0.5, 1.0), (0.0, 1.0), ink),
            // The three shades: the ink, a quarter, a half and three
            // quarters of the way up from the ground.
            0x2591..=0x2593 => {
                let depth = (code - 0x2590) as f32 * 0.25;
                part((0.0, 1.0), (0.0, 1.0), mix(ground, ink, depth));
            }
            0x2594 => part((0.0, 1.0), (0.0, eighths(1)), ink),
            0x2595 => part((eighths(7), 1.0), (0.0, 1.0), ink),
            _ => {
                // The quarters, as four bits: upper left, upper right,
                // lower left, lower right.
                let which: u8 = match code {
                    0x2596 => 0b0010,
                    0x2597 => 0b0001,
                    0x2598 => 0b1000,
                    0x2599 => 0b1011,
                    0x259a => 0b1001,
                    0x259b => 0b1110,
                    0x259c => 0b1101,
                    0x259d => 0b0100,
                    0x259e => 0b0110,
                    _ => 0b0111,
                };
                for (bit, upper, leftward) in
                    [(8, true, true), (4, true, false), (2, false, true), (1, false, false)]
                {
                    if which & bit != 0 {
                        quarter(upper, leftward);
                    }
                }
            }
        }
        return true;
    }

    // A light line is a pixel at the size a terminal is usually read at,
    // and grows with the letters.
    let light = ((wide * scale / 7.0).round().max(1.0)) / scale;
    let (cx, cy) = (snap(left + wide / 2.0 - light / 2.0) + light / 2.0, snap(top + tall / 2.0 - light / 2.0) + light / 2.0);

    // The diagonals are the three that are not arms.
    if (0x2571..=0x2573).contains(&code) {
        let stroke = Stroke::new(light, ink);
        if code != 0x2572 {
            painter.line_segment([egui::pos2(left, bottom), egui::pos2(right, top)], stroke);
        }
        if code != 0x2571 {
            painter.line_segment([egui::pos2(left, top), egui::pos2(right, bottom)], stroke);
        }
        return true;
    }

    let [up, east, down, west] = ARMS[(code - 0x2500) as usize];
    // One arm: a bar `half` either side of the middle line, from `near`
    // the middle out to the edge of the box.
    let arm = |which: usize, half: f32, near: f32, colour: Color32| match which {
        0 => fill((cx - half, cx + half), (top, cy + near), colour),
        1 => fill((cx - near, right), (cy - half, cy + half), colour),
        2 => fill((cx - half, cx + half), (cy - near, bottom), colour),
        _ => fill((left, cx + near), (cy - half, cy + half), colour),
    };
    let arms = [up, east, down, west];

    // A double line is a thick one with its middle taken back out, which
    // gets every corner and crossing of them right without a case for
    // each: the outline of two thick bars meeting is two thin lines
    // turning together. As thick as a heavy line, and what is taken out is
    // a light one, so the two lines and the gap between them are a pixel
    // each and all three sit on whole pixels.
    let crossed = |which: usize| arms[(which + 1) % 4] == 3 || arms[(which + 3) % 4] == 3;
    let (outer, inner) = (light * 1.5, light * 0.5);
    for which in 0..4 {
        if arms[which] == 3 {
            arm(which, outer, if crossed(which) { outer } else { 0.0 }, ink);
        }
    }
    for which in 0..4 {
        if arms[which] == 3 {
            arm(which, inner, if crossed(which) { inner } else { 0.0 }, ground);
        }
    }
    for which in 0..4 {
        let half = match arms[which] {
            1 => light / 2.0,
            2 => light * 1.5,
            _ => continue,
        };
        arm(which, half, half, ink);
    }
    true
}

/// The handle down the right-hand edge for reading back.
///
/// There only while there is something above to read, and faint until the
/// pointer is in the panel: the terminal is the one place in this window
/// where the edge of the text is the edge of somebody else's layout.
fn scroller(ui: &mut egui::Ui, session: &mut Session, rect: egui::Rect, near: bool) {
    let (history, back, rows) = {
        let term = session.grid();
        (term.grid().history_size(), term.grid().display_offset(), term.grid().screen_lines())
    };
    if history == 0 {
        return;
    }
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.right() - SCROLLER, rect.top() + 2.0),
        egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
    );
    let length = (track.height() * rows as f32 / (rows + history) as f32).max(22.0);
    let travel = (track.height() - length).max(1.0);
    let top = track.top() + travel * (1.0 - back as f32 / history as f32);
    let thumb = egui::Rect::from_min_size(egui::pos2(track.left(), top), egui::vec2(track.width(), length));

    let handle = ui.interact(track, ui.id().with("terminal-scroller"), Sense::click_and_drag());
    if let Some(pos) = handle.interact_pointer_pos().filter(|_| handle.dragged() || handle.clicked()) {
        // Where the middle of the handle is asked to be, as lines back.
        let part = 1.0 - ((pos.y - length / 2.0 - track.top()) / travel).clamp(0.0, 1.0);
        let wanted = (part * history as f32).round() as i32;
        session.grid().scroll_display(Scroll::Delta(wanted - back as i32));
    }
    let live = handle.hovered() || handle.dragged();
    if live || near || back > 0 {
        let colour = match live {
            true => theme::muted(),
            false => theme::tint(theme::muted(), if back > 0 { 110 } else { 60 }),
        };
        ui.painter().rect_filled(thumb.shrink2(egui::vec2(1.0, 0.0)), 3, colour);
    }
}

/// The bar above the terminal: what it is on the left, and on the right
/// the three things that can be done to the panel.
///
/// The bar is all there is of a folded panel, which is why it is a function
/// of its own: it is drawn whether or not there is a grid under it.
pub fn bar(ui: &mut egui::Ui, minimized: bool, title: Option<&str>) -> Option<Act> {
    let mut act = None;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), BAR),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add_space(10.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
            prompt(ui.painter(), rect, theme::muted());
            ui.label(
                egui::RichText::new("Terminal").font(theme::medium(12.0)).color(theme::text()),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(6.0);
                if icon(ui, Icon::Cross, "Hide Terminal") {
                    act = Some(Act::Hide);
                }
                if icon(ui, Icon::Bin, "Kill Terminal") {
                    act = Some(Act::Kill);
                }
                match minimized {
                    true => {
                        if icon(ui, Icon::Up, "Expand Terminal") {
                            act = Some(Act::Expand);
                        }
                    }
                    false => {
                        if icon(ui, Icon::Down, "Minimize Terminal") {
                            act = Some(Act::Minimize);
                        }
                    }
                }
                // What the shell calls itself -- usually who and where --
                // in whatever room is left, cut short rather than wrapped.
                if let Some(title) = title {
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(title)
                                    .font(theme::mono(10.5))
                                    .color(theme::faint()),
                            )
                            .truncate(),
                        );
                    });
                }
            });
        },
    );
    act
}

/// The button in a repository's bar that opens the panel and puts it away.
///
/// Drawn rather than built out of a button and a label, for the prompt in
/// front of the word: the fonts here have a `>` and a `_`, but set side by
/// side as text they are two characters, and a prompt is one sign.
pub fn toggle(ui: &mut egui::Ui, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        "Terminal".to_string(),
        theme::medium(12.5),
        Color32::PLACEHOLDER,
    );
    let pad = ui.spacing().button_padding;
    let sign = 14.0;
    let size = egui::vec2(
        pad.x * 2.0 + sign + 6.0 + galley.size().x,
        ui.spacing().interact_size.y.max(galley.size().y + pad.y * 2.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.style().interact_selectable(&response, on);
    let painter = ui.painter();
    painter.rect(
        rect,
        visuals.corner_radius,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let ink = visuals.text_color();
    let at = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad.x, rect.center().y - sign / 2.0),
        egui::vec2(sign, sign),
    );
    prompt(painter, at, ink);
    painter.galley(
        egui::pos2(at.right() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
}

/// A prompt: the angle a shell waits behind, and the line where the next
/// letter goes.
fn prompt(painter: &egui::Painter, rect: egui::Rect, colour: Color32) {
    let stroke = Stroke::new(1.4, colour);
    let (left, middle) = (rect.left() + 1.5, rect.center().y);
    painter.line_segment([egui::pos2(left, middle - 3.6), egui::pos2(left + 4.0, middle)], stroke);
    painter.line_segment([egui::pos2(left + 4.0, middle), egui::pos2(left, middle + 3.6)], stroke);
    painter.line_segment(
        [egui::pos2(left + 6.5, middle + 4.0), egui::pos2(rect.right() - 1.0, middle + 4.0)],
        stroke,
    );
}

/// The four little drawings in the bar.
#[derive(Clone, Copy)]
enum Icon {
    Down,
    Up,
    Bin,
    Cross,
}

/// One of them, as something to press. Says whether it was.
fn icon(ui: &mut egui::Ui, which: Icon, hint: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 20.0), Sense::click());
    let colour = match response.hovered() {
        true => theme::text(),
        false => theme::muted(),
    };
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::hover());
    }
    let painter = ui.painter();
    let stroke = Stroke::new(1.3, colour);
    let centre = rect.center();
    let line = |from: (f32, f32), to: (f32, f32)| {
        painter.line_segment(
            [centre + egui::vec2(from.0, from.1), centre + egui::vec2(to.0, to.1)],
            stroke,
        );
    };
    match which {
        Icon::Down => {
            line((-4.0, -2.0), (0.0, 2.2));
            line((0.0, 2.2), (4.0, -2.0));
        }
        Icon::Up => {
            line((-4.0, 2.0), (0.0, -2.2));
            line((0.0, -2.2), (4.0, 2.0));
        }
        Icon::Cross => {
            line((-3.6, -3.6), (3.6, 3.6));
            line((-3.6, 3.6), (3.6, -3.6));
        }
        // A lid with its handle, and the can under it.
        Icon::Bin => {
            line((-4.6, -3.2), (4.6, -3.2));
            line((-1.6, -3.2), (-1.6, -5.0));
            line((-1.6, -5.0), (1.6, -5.0));
            line((1.6, -5.0), (1.6, -3.2));
            line((-3.4, -3.2), (-2.8, 5.0));
            line((-2.8, 5.0), (2.8, 5.0));
            line((2.8, 5.0), (3.4, -3.2));
            line((-1.0, -1.0), (-1.0, 3.0));
            line((1.0, -1.0), (1.0, 3.0));
        }
    }
    response.on_hover_text(hint).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(line: &str, col: usize) -> Option<String> {
        let letters: Vec<char> = line.chars().collect();
        address_in(&letters, col).map(|span| letters[span].iter().collect())
    }

    #[test]
    fn an_address_is_found_under_the_pointer_and_only_there() {
        let line = "remote: see https://example.org/pull/7 for it";
        assert_eq!(found(line, 14).as_deref(), Some("https://example.org/pull/7"));
        assert_eq!(found(line, 37).as_deref(), Some("https://example.org/pull/7"));
        assert_eq!(found(line, 3), None);
        assert_eq!(found(line, 40), None);
    }

    #[test]
    fn what_ends_a_sentence_is_not_part_of_the_address() {
        assert_eq!(
            found("(read http://example.org/a.html).", 10).as_deref(),
            Some("http://example.org/a.html")
        );
        assert_eq!(found("go to 'https://example.org' now", 9).as_deref(), Some("https://example.org"));
    }

    #[test]
    fn two_addresses_on_a_line_are_two_addresses() {
        let line = "https://one.example https://two.example";
        assert_eq!(found(line, 2).as_deref(), Some("https://one.example"));
        assert_eq!(found(line, 25).as_deref(), Some("https://two.example"));
        assert_eq!(found(line, 19), None);
    }

    #[test]
    fn a_point_on_the_screen_is_a_box_in_the_grid() {
        let layout = Layout {
            origin: egui::pos2(10.0, 20.0),
            cell: egui::vec2(8.0, 16.0),
            cols: 80,
            rows: 24,
        };
        assert!(matches!(layout.at(egui::pos2(10.0, 20.0)), (0, 0, Side::Left)));
        assert!(matches!(layout.at(egui::pos2(31.0, 37.0)), (2, 1, Side::Right)));
        // Off the edges it is the nearest box, not no box.
        assert!(matches!(layout.at(egui::pos2(-50.0, -50.0)), (0, 0, Side::Left)));
        assert!(matches!(layout.at(egui::pos2(9_000.0, 9_000.0)), (79, 23, Side::Right)));
    }
}
