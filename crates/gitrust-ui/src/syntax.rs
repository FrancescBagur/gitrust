//! Colouring code, without a dependency and without pretending to parse it.
//!
//! This is not a parser and does not want to be. It is a reader that knows
//! five or six shapes -- a comment, something in quotes, a number, a word
//! that is a keyword, a word that starts with a capital, a word with a
//! bracket after it -- and that is very nearly all the colour anybody reads
//! anyway. The alternative was a syntax crate with a grammar for two hundred
//! languages and a tree of its own, which is a great deal of machinery to
//! make `import` purple.
//!
//! What it will get wrong, and does not hide: a keyword inside a word it does
//! not know, a language whose comments are neither `//` nor `#`, and anything
//! clever with quotes. None of those is worse than no colour at all, which is
//! what was there before.
//!
//! # Where the colours come from
//!
//! From the lanes, every one of them. The theme's own rule is that colour
//! comes from the graph's family or it does not come, and a code view is
//! exactly the place a rule like that gets broken by somebody reaching for a
//! generic editor palette. Eight tones of even lightness turn out to be
//! plenty: violet for the words the language owns, turquoise for the names
//! you gave things, azure for what you call, brass for what is written in
//! quotes, rose for numbers, and grey for everything said to a human.

use eframe::egui::Color32;

use crate::theme;

/// What one run of characters is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Everything that is none of the others.
    Plain,
    /// A word the language owns.
    Keyword,
    /// A word that starts with a capital, which in nearly every language
    /// means a type, a class or something else somebody declared.
    Type,
    /// A word with a bracket straight after it.
    Call,
    /// Anything in quotes.
    Quoted,
    Number,
    /// Said to a human, not to the machine.
    Comment,
    /// Brackets, commas, operators: the scaffolding.
    Mark,
}

impl Kind {
    pub fn color(self) -> Color32 {
        match self {
            Kind::Plain => theme::TEXT,
            Kind::Keyword => theme::lane(6),
            Kind::Type => theme::lane(4),
            Kind::Call => theme::lane(0),
            Kind::Quoted => theme::lane(7),
            Kind::Number => theme::lane(3),
            Kind::Comment => theme::FAINT,
            Kind::Mark => theme::MUTED,
        }
    }
}

/// One run of characters that are all the same kind.
#[derive(Debug, Clone)]
pub struct Piece {
    pub text: String,
    pub kind: Kind,
}

/// Which shapes to look for, worked out from the file's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tongue {
    /// `//` and `/* */`, and a single quote that is one character long or it
    /// is not a quote at all -- which is what tells a char literal from a
    /// lifetime, and is why Rust is not lumped in with the rest.
    Rusty,
    /// `//` and `/* */`, and quotes of all three kinds.
    Curly,
    /// `#` to the end of the line.
    Hash,
    /// Headings and backticks.
    Markup,
    /// Nothing is coloured. A file this does not know is better read plain
    /// than read wrong.
    Plain,
}

impl Tongue {
    fn of(path: &str) -> Tongue {
        let name = path.rsplit('/').next().unwrap_or(path);
        let tail = name.rsplit_once('.').map(|(_, tail)| tail).unwrap_or("");

        match tail.to_ascii_lowercase().as_str() {
            "rs" => Tongue::Rusty,
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "json" | "java" | "kt" | "kts"
            | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "go" | "swift" | "scala" | "php"
            | "css" | "scss" | "less" | "dart" | "proto" | "sql" => Tongue::Curly,
            "py" | "sh" | "bash" | "zsh" | "rb" | "toml" | "yaml" | "yml" | "ini" | "cfg"
            | "conf" | "nix" | "pl" | "r" => Tongue::Hash,
            "md" | "markdown" => Tongue::Markup,
            _ => match name {
                "Makefile" | "Dockerfile" | ".gitignore" | ".gitattributes" | ".env" => {
                    Tongue::Hash
                }
                _ => Tongue::Plain,
            },
        }
    }
}

/// What is still open at the end of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carry {
    Nothing,
    /// Inside `/* */`.
    Comment,
    /// Inside a `` ` `` string, which is the only quote that may span lines.
    Quoted,
    /// Inside a fenced block in markdown.
    Fenced,
}

/// Cuts a whole file into lines of coloured pieces.
///
/// The whole file at once rather than a line at a time, because a comment
/// that opened three lines up is only knowable from here.
pub fn read(text: &str, path: &str) -> Vec<Vec<Piece>> {
    let lines: Vec<&str> = text.lines().collect();
    read_lines(&lines, path)
}

/// The same, for somebody who already has the lines.
///
/// This is the one the callers with lines in hand use, and not for tidiness:
/// joining lines with a newline and letting this split them again loses a
/// last line that is empty, and every one of these results is read back by
/// position. One line in, one line out, always.
pub fn read_lines(lines: &[&str], path: &str) -> Vec<Vec<Piece>> {
    let tongue = Tongue::of(path);
    let mut carry = Carry::Nothing;
    lines
        .iter()
        .map(|line| cut(line, tongue, &mut carry))
        .collect()
}

fn cut(line: &str, tongue: Tongue, carry: &mut Carry) -> Vec<Piece> {
    if tongue == Tongue::Plain {
        return vec![piece(line, Kind::Plain)];
    }
    if tongue == Tongue::Markup {
        return markup(line, carry);
    }

    let chars: Vec<char> = line.chars().collect();
    let mut out = Pieces::default();
    let mut at = 0;

    // Whatever was left open at the end of the line before runs to wherever
    // it closes on this one, or to the end of this one.
    match *carry {
        Carry::Comment => at = run_to(&chars, 0, "*/", &mut out, Kind::Comment, carry),
        Carry::Quoted => at = quoted_on(&chars, 0, '`', &mut out, carry),
        _ => {}
    }

    while at < chars.len() {
        let here = chars[at];
        let next = chars.get(at + 1).copied();

        // Said to a human: everything to the end of the line.
        let line_comment = match tongue {
            Tongue::Hash => here == '#',
            _ => here == '/' && next == Some('/'),
        };
        if line_comment {
            out.push(&chars[at..], Kind::Comment);
            break;
        }

        if tongue != Tongue::Hash && here == '/' && next == Some('*') {
            // The opener belongs to the comment, and is pushed before the
            // run that follows it: a piece dropped here is a piece missing
            // from the line on screen.
            out.push(&chars[at..at + 2], Kind::Comment);
            *carry = Carry::Comment;
            at = run_to(&chars, at + 2, "*/", &mut out, Kind::Comment, carry);
            continue;
        }

        if here == '"' || (here == '`' && tongue != Tongue::Hash) || single(here, tongue, &chars, at)
        {
            if here == '`' {
                out.push(&chars[at..at + 1], Kind::Quoted);
                *carry = Carry::Quoted;
                at = quoted_on(&chars, at + 1, '`', &mut out, carry);
                continue;
            }
            at = quoted(&chars, at, here, &mut out);
            continue;
        }

        if here.is_ascii_digit() {
            let was = at;
            while at < chars.len()
                && (chars[at].is_ascii_alphanumeric() || chars[at] == '.' || chars[at] == '_')
            {
                at += 1;
            }
            out.push(&chars[was..at], Kind::Number);
            continue;
        }

        if here.is_alphabetic() || here == '_' {
            let was = at;
            while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                at += 1;
            }
            let word: String = chars[was..at].iter().collect();
            out.push(&chars[was..at], word_kind(&word, &chars, at));
            continue;
        }

        if here.is_whitespace() {
            let was = at;
            while at < chars.len() && chars[at].is_whitespace() {
                at += 1;
            }
            out.push(&chars[was..at], Kind::Plain);
            continue;
        }

        out.push(&chars[at..at + 1], Kind::Mark);
        at += 1;
    }

    out.done()
}

/// What a bare word is: a keyword, a name somebody declared, something being
/// called, or none of those.
fn word_kind(word: &str, chars: &[char], after: usize) -> Kind {
    if keyword(word) {
        return Kind::Keyword;
    }
    if word.starts_with(char::is_uppercase) {
        return Kind::Type;
    }
    // A bracket straight after it, with nothing but spaces in between.
    let called = chars[after..]
        .iter()
        .find(|c| !c.is_whitespace())
        .is_some_and(|c| *c == '(');
    match called {
        true => Kind::Call,
        false => Kind::Plain,
    }
}

/// A single quote is a string in most languages and a lifetime in Rust. The
/// difference is whether it closes almost immediately.
fn single(here: char, tongue: Tongue, chars: &[char], at: usize) -> bool {
    if here != '\'' {
        return false;
    }
    match tongue {
        Tongue::Rusty => {
            // 'a' or '\n' or '\u{1F600}'. Anything longer is a lifetime.
            let close = chars[at + 1..].iter().take(12).position(|c| *c == '\'');
            // One character between the quotes, or an escape of any length.
            close == Some(1) || (chars.get(at + 1) == Some(&'\\') && close.is_some())
        }
        _ => true,
    }
}

/// A run in quotes that closes on this line, or runs off the end of it.
fn quoted(chars: &[char], from: usize, quote: char, out: &mut Pieces) -> usize {
    let mut at = from + 1;
    while at < chars.len() {
        if chars[at] == '\\' {
            at += 2;
            continue;
        }
        if chars[at] == quote {
            at += 1;
            break;
        }
        at += 1;
    }
    let end = at.min(chars.len());
    out.push(&chars[from..end], Kind::Quoted);
    end
}

/// The same, for the one quote that may carry on to the next line.
fn quoted_on(chars: &[char], from: usize, quote: char, out: &mut Pieces, carry: &mut Carry) -> usize {
    let mut at = from;
    while at < chars.len() {
        if chars[at] == '\\' {
            at += 2;
            continue;
        }
        if chars[at] == quote {
            at += 1;
            *carry = Carry::Nothing;
            break;
        }
        at += 1;
    }
    let end = at.min(chars.len());
    out.push(&chars[from..end], Kind::Quoted);
    end
}

/// Everything up to a closing mark, or to the end of the line.
fn run_to(
    chars: &[char],
    from: usize,
    close: &str,
    out: &mut Pieces,
    kind: Kind,
    carry: &mut Carry,
) -> usize {
    let close: Vec<char> = close.chars().collect();
    let mut at = from;
    while at < chars.len() {
        if chars[at..].starts_with(&close[..]) {
            at += close.len();
            *carry = Carry::Nothing;
            break;
        }
        at += 1;
    }
    let end = at.min(chars.len());
    out.push(&chars[from..end], kind);
    end
}

/// Markdown, read for the two things that carry its shape: what is a heading
/// and what is code.
fn markup(line: &str, carry: &mut Carry) -> Vec<Piece> {
    if line.trim_start().starts_with("```") {
        *carry = match *carry {
            Carry::Fenced => Carry::Nothing,
            _ => Carry::Fenced,
        };
        return vec![piece(line, Kind::Quoted)];
    }
    if *carry == Carry::Fenced {
        return vec![piece(line, Kind::Plain)];
    }
    if line.starts_with('#') {
        return vec![piece(line, Kind::Keyword)];
    }

    // Backticks, which is the only thing left worth telling apart.
    let mut out = Pieces::default();
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '`' {
            let was = at;
            at += 1;
            while at < chars.len() && chars[at] != '`' {
                at += 1;
            }
            at = (at + 1).min(chars.len());
            out.push(&chars[was..at], Kind::Quoted);
            continue;
        }
        let was = at;
        while at < chars.len() && chars[at] != '`' {
            at += 1;
        }
        out.push(&chars[was..at], Kind::Plain);
    }
    out.done()
}

fn piece(text: &str, kind: Kind) -> Piece {
    Piece { text: text.to_string(), kind }
}

/// Pieces being gathered, with runs of the same kind joined as they arrive.
///
/// Joining matters: every piece becomes a section of a laid-out line, and a
/// line cut into forty one-character pieces lays out forty times slower than
/// one cut into six.
#[derive(Default)]
struct Pieces(Vec<Piece>);

impl Pieces {
    fn push(&mut self, chars: &[char], kind: Kind) {
        if chars.is_empty() {
            return;
        }
        let text: String = chars.iter().collect();
        match self.0.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(&text),
            _ => self.0.push(Piece { text, kind }),
        }
    }

    fn done(self) -> Vec<Piece> {
        self.0
    }
}

fn keyword(word: &str) -> bool {
    matches!(
        word,
        "abstract" | "and" | "as" | "assert" | "async" | "await" | "bool" | "break" | "case"
            | "catch" | "char" | "class" | "const" | "constructor" | "continue" | "crate"
            | "declare" | "def" | "default" | "defer" | "del" | "delete" | "do" | "double"
            | "dyn" | "elif" | "else" | "enum" | "except" | "export" | "extends" | "extern"
            | "final" | "finally" | "float" | "fn" | "for" | "from" | "func" | "function"
            | "go" | "goto" | "if" | "impl" | "implements" | "import" | "in" | "instanceof"
            | "int" | "interface" | "is" | "lambda" | "let" | "long" | "loop" | "match"
            | "mod" | "module" | "move" | "mut" | "namespace" | "new" | "nil" | "not"
            | "or" | "package" | "pass" | "private" | "protected" | "pub" | "public"
            | "raise" | "readonly" | "ref" | "return" | "select" | "static" | "struct"
            | "super" | "switch" | "sync" | "this" | "throw" | "throws" | "trait" | "try"
            | "type" | "typeof" | "union" | "unsafe" | "use" | "var" | "void" | "where"
            | "while" | "with" | "yield" | "self" | "true" | "false" | "null" | "undefined"
            | "None" | "True" | "False"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str, path: &str) -> Vec<Vec<(String, Kind)>> {
        read(text, path)
            .into_iter()
            .map(|line| line.into_iter().map(|p| (p.text, p.kind)).collect())
            .collect()
    }

    fn only(text: &str, path: &str) -> Vec<(String, Kind)> {
        kinds(text, path).into_iter().next().unwrap_or_default()
    }

    #[test]
    fn a_keyword_a_type_and_a_call_are_told_apart() {
        let line = only("let thing = Repo::open(path);", "a.rs");
        assert!(line.iter().any(|(t, k)| t == "let" && *k == Kind::Keyword));
        assert!(line.iter().any(|(t, k)| t == "Repo" && *k == Kind::Type));
        assert!(line.iter().any(|(t, k)| t == "open" && *k == Kind::Call));
        assert!(line.iter().any(|(t, k)| t.contains("thing") && *k == Kind::Plain));
    }

    #[test]
    fn what_is_said_to_a_human_runs_to_the_end_of_the_line() {
        let line = only("let x = 1; // let y = 2;", "a.rs");
        let comment: String = line
            .iter()
            .filter(|(_, k)| *k == Kind::Comment)
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(comment, "// let y = 2;");
        // And the keyword inside it is not one any more.
        assert_eq!(line.iter().filter(|(t, k)| t == "let" && *k == Kind::Keyword).count(), 1);
    }

    #[test]
    fn a_comment_that_opens_on_one_line_closes_on_another() {
        let lines = kinds("a /* one\ntwo\nthree */ b", "a.ts");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].iter().all(|(_, k)| *k == Kind::Comment), "{:?}", lines[1]);
        assert!(lines[2].iter().any(|(t, k)| t.contains("three") && *k == Kind::Comment));
        assert!(lines[2].iter().any(|(t, k)| t.trim() == "b" && *k == Kind::Plain));
    }

    /// The one that made Rust its own tongue: `'a` is a lifetime and `'a'` is
    /// a character, and colouring the first one as a string paints half the
    /// file brass.
    #[test]
    fn a_lifetime_is_not_a_string_and_a_char_is() {
        let line = only("fn f<'a>(s: &'a str) -> char { 'x' }", "a.rs");
        assert!(
            !line.iter().any(|(t, k)| t.contains("'a") && *k == Kind::Quoted),
            "{line:?}"
        );
        assert!(line.iter().any(|(t, k)| t == "'x'" && *k == Kind::Quoted), "{line:?}");
    }

    #[test]
    fn a_single_quoted_string_is_a_string_everywhere_else() {
        let line = only("import { auth } from '@/server/auth'", "a.ts");
        assert!(
            line.iter().any(|(t, k)| t == "'@/server/auth'" && *k == Kind::Quoted),
            "{line:?}"
        );
        assert!(line.iter().any(|(t, k)| t == "import" && *k == Kind::Keyword));
    }

    #[test]
    fn a_hash_is_a_comment_where_a_hash_is_a_comment_and_not_where_it_is_not() {
        let python = only("x = 1  # and here is why", "a.py");
        assert!(python.iter().any(|(t, k)| t.starts_with('#') && *k == Kind::Comment));

        // In Rust a hash is an attribute, not a comment.
        let rust = only("#[derive(Debug)]", "a.rs");
        assert!(!rust.iter().any(|(_, k)| *k == Kind::Comment), "{rust:?}");
    }

    #[test]
    fn a_file_this_does_not_know_is_left_alone() {
        let line = only("let this look like code", "notes.whatever");
        assert_eq!(line.len(), 1);
        assert_eq!(line[0].1, Kind::Plain);
    }

    #[test]
    fn a_heading_and_a_fenced_block_are_told_apart_in_markdown() {
        let lines = kinds("# Title\n\n```\nlet x = 1;\n```\nand `code` here", "README.md");
        assert_eq!(lines[0][0].1, Kind::Keyword);
        assert_eq!(lines[2][0].1, Kind::Quoted, "the fence");
        assert_eq!(lines[3][0].1, Kind::Plain, "inside it, left alone");
        assert!(lines[5].iter().any(|(t, k)| t == "`code`" && *k == Kind::Quoted));
    }

    /// Colour may be wrong; the text may not. Anything dropped on the way
    /// through is a character missing from the file on screen, which is the
    /// one mistake here that is not merely ugly.
    #[test]
    fn every_line_comes_back_exactly_as_it_went_in() {
        let samples = [
            ("let x = /* a */ 1; // done\nnext 'a' and 'x'", "a.rs"),
            ("import { a } from 'b'\nconst t = `one ${two}\nthree`\n", "a.ts"),
            ("x = 1  # why\ny = 'two'", "a.py"),
            ("# Title\n\n```\ncode\n```\nand `this`", "README.md"),
            ("anything at all", "notes.whatever"),
            ("", "a.rs"),
        ];
        for (text, path) in samples {
            let back: Vec<String> = read(text, path)
                .into_iter()
                .map(|line| line.into_iter().map(|p| p.text).collect())
                .collect();
            let want: Vec<String> = text.lines().map(str::to_string).collect();
            assert_eq!(back, want, "in {path}");
        }
    }

    /// The reason `read_lines` exists. A hunk whose last line is empty used
    /// to come back one line shorter, and everything that reads these by
    /// position would then be colouring the wrong line.
    #[test]
    fn one_line_in_is_always_one_line_out() {
        for lines in [
            vec!["a", ""],
            vec!["", ""],
            vec![""],
            vec!["let x = 1;", "", "", "let y = 2;", ""],
            vec![],
        ] {
            assert_eq!(read_lines(&lines, "a.rs").len(), lines.len(), "{lines:?}");
        }
    }

    /// A hunk is a fragment, and a fragment cannot know what opened before
    /// it. Said here so that it is a decision and not a surprise.
    #[test]
    fn a_fragment_does_not_know_about_a_comment_that_opened_above_it() {
        let inside = read_lines(&["still inside the comment", "*/ let x = 1;"], "a.rs");
        assert_ne!(inside[0][0].kind, Kind::Comment, "it cannot know: {inside:?}");
        assert!(inside[1].iter().any(|p| p.text == "let" && p.kind == Kind::Keyword));
    }

    #[test]
    fn runs_of_one_kind_arrive_as_one_piece() {
        let line = only("a + b + c", "a.rs");
        // Not nine pieces of one character each.
        assert!(line.len() <= 5, "{line:?}");
    }

    #[test]
    fn a_number_is_a_number_and_a_word_starting_with_one_is_not_split() {
        let line = only("let n = 1_000.5;", "a.rs");
        assert!(line.iter().any(|(t, k)| t == "1_000.5" && *k == Kind::Number), "{line:?}");
    }
}
