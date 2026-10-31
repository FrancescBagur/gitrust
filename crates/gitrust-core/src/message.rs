//! Putting the commit message together from the subject and the description.
//!
//! Git treats the first line as the subject and expects a blank line before
//! the body. Keeping them apart in the interface means nobody forgets that.

/// The length past which the subject is called long.
///
/// 72 is the width at which `git log` indents without overflowing an 80-column
/// terminal. It is not a hard limit: we warn, we do not stop you.
pub const SUBJECT_LIMIT: usize = 72;

/// Joins subject and description with the blank line Git expects.
pub fn compose(subject: &str, body: &str) -> String {
    let subject = subject.trim();
    let body = body.trim();

    if body.is_empty() {
        subject.to_string()
    } else {
        format!("{subject}\n\n{body}")
    }
}

/// A piece of the body of a commit message.
///
/// Bodies come wrapped by hand to about seventy columns, which is what a
/// terminal asks for but not what a panel measures. Re-wrapping that text to
/// whatever width there is leaves a short line behind every long one, so the
/// prose's own line breaks have to be undone first. What is not prose -- a
/// list, a piece of code, a *trailer* -- is left alone, because there a line
/// break means something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Running prose: it can be wrapped again to whatever width there is.
    Prose(String),
    /// Text that is kept exactly as it came.
    Literal(String),
}

/// Does this line lose its meaning if it is joined to the next one?
fn is_literal(line: &str) -> bool {
    // Indented: nearly always code, or a list carrying on.
    if line.starts_with(' ') || line.starts_with('\t') {
        return true;
    }

    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return false;
    }

    // Bullets, quotes, headings and tables.
    if let Some(first) = trimmed.chars().next() {
        if matches!(first, '-' | '*' | '+' | '•' | '>' | '#' | '|' | '=') {
            return true;
        }
    }

    // Numbered lists: "1. something", "2) something".
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() {
        if let Some(after) = trimmed[digits.len()..].chars().next() {
            if matches!(after, '.' | ')') {
                return true;
            }
        }
    }

    // A single word, but only if it looks like a path or an identifier: the
    // last line of a wrapped paragraph is often a lone word, and that one does
    // have to be joined back.
    if !trimmed.contains(' ')
        && (trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("::"))
    {
        return true;
    }

    // A Git trailer: "Co-Authored-By: ...", "Signed-off-by: ...".
    if let Some((key, _)) = trimmed.split_once(": ") {
        if !key.is_empty()
            && !key.contains(' ')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return true;
        }
    }

    false
}

/// Cuts the body into blocks, joining the lines of the prose back together.
///
/// A whole block is kept as it came the moment a single one of its lines asks
/// for it: in a mixed paragraph, leaving it alone beats joining what should not.
pub fn blocks(body: &str) -> Vec<Block> {
    let mut out = Vec::new();

    for chunk in body.split("\n\n") {
        let lines: Vec<&str> = chunk.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }

        if lines.iter().any(|line| is_literal(line)) {
            out.push(Block::Literal(
                lines
                    .iter()
                    .map(|l| l.trim_end())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        } else {
            out.push(Block::Prose(
                lines
                    .iter()
                    .map(|l| l.trim())
                    .collect::<Vec<_>>()
                    .join(" "),
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_description_the_message_is_just_the_subject() {
        assert_eq!(compose("feat: something", ""), "feat: something");
        assert_eq!(compose("  feat: something  ", "   "), "feat: something");
    }

    #[test]
    fn the_body_goes_after_a_blank_line() {
        assert_eq!(
            compose("feat: something", "Why it was needed."),
            "feat: something\n\nWhy it was needed."
        );
    }

    #[test]
    fn the_bodys_own_line_breaks_are_kept() {
        let body = "First paragraph.\n\n- a bullet\n- another";
        let message = compose("fix: the other thing", body);

        assert!(message.starts_with("fix: the other thing\n\n"));
        assert!(message.ends_with("- another"));
        assert_eq!(message.lines().count(), 6);
    }

    #[test]
    fn prose_wrapped_by_hand_is_joined_back_up() {
        let body = "Compares two periods from the console\nand leaves the report out of the repo.";
        assert_eq!(
            blocks(body),
            vec![Block::Prose(
                "Compares two periods from the console and leaves the report out of the repo."
                    .to_string()
            )]
        );
    }

    #[test]
    fn a_list_keeps_its_line_breaks() {
        let body = "- adds the command\n- documents the credentials";
        assert_eq!(
            blocks(body),
            vec![Block::Literal(
                "- adds the command\n- documents the credentials".to_string()
            )]
        );
    }

    #[test]
    fn paragraphs_and_trailers_come_out_apart() {
        let body = "First an\nexplanation.\n\n- a point\n\nCo-Authored-By: Somebody <a@b.c>";
        assert_eq!(
            blocks(body),
            vec![
                Block::Prose("First an explanation.".to_string()),
                Block::Literal("- a point".to_string()),
                Block::Literal("Co-Authored-By: Somebody <a@b.c>".to_string()),
            ]
        );
    }

    #[test]
    fn a_block_with_one_literal_line_is_kept_whole() {
        // If a paragraph mixes prose and code, joining it would break the code.
        let body = "This fixes it:\n    cargo test --workspace";
        assert!(matches!(blocks(body).as_slice(), [Block::Literal(_)]));
    }

    #[test]
    fn a_lone_path_is_not_joined_to_the_paragraph_but_a_word_is() {
        assert!(matches!(
            blocks("The file is\ncrates/core/src/lib.rs").as_slice(),
            [Block::Literal(_)]
        ));
        assert_eq!(
            blocks("A sentence that ends\nhere."),
            vec![Block::Prose("A sentence that ends here.".to_string())]
        );
    }

    #[test]
    fn an_empty_body_gives_no_blocks() {
        assert!(blocks("").is_empty());
        assert!(blocks("\n\n  \n").is_empty());
    }
}
