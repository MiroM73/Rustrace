//! Text shown in the message cell of the table.
//!
//! The cell normally shows the beginning of the first line of the message. When a search match
//! lies outside of it (on a later line, past the length cut or past the right edge of the
//! column), the UI shows an excerpt starting shortly before the match instead
//! ([`around`]), so the highlighted match is visible in the table.

/// Characters of context kept before a search match in an excerpt.
pub const CONTEXT_CHARS: usize = 24;

/// Marker appended when the message has more lines than the cell shows.
const MORE_LINES: &str = "  ↵";
/// Marker for text that was left out.
const ELLIPSIS: &str = "…";

/// `s` cut to at most `max` bytes at a char boundary; `true` when something was cut off.
fn cut(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

/// The line of `message` containing byte offset `at`: (start, end) without the line break
/// (a trailing `\r` is left out as well).
fn line_bounds(message: &str, at: usize) -> (usize, usize) {
    let start = message[..at].rfind('\n').map_or(0, |p| p + 1);
    let mut end = message[at..].find('\n').map_or(message.len(), |p| at + p);
    if end > start && message.as_bytes()[end - 1] == b'\r' && end > at {
        end -= 1;
    }
    (start, end)
}

/// Default cell text: the first line cut to `max` bytes ("…" when cut), "  ↵" when the message
/// has more lines.
pub fn first_line(message: &str, max: usize) -> String {
    let first = message.lines().next().unwrap_or("");
    let (shown, was_cut) = cut(first, max);
    let mut text = shown.to_owned();
    if was_cut {
        text.push_str(ELLIPSIS);
    }
    if message.contains('\n') {
        text.push_str(MORE_LINES);
    }
    text
}

/// Excerpt of the line of `message` that contains byte offset `at` (start of a search match):
/// it starts up to `context` characters before `at` – after a space in that window when there is
/// one, so it does not begin in the middle of a word – with "…" in front when anything before it
/// was left out. Cut to `max` bytes after the start, "  ↵" when the message has more lines.
///
/// Returns the text and the byte offset of `at` in it.
pub fn around(message: &str, at: usize, context: usize, max: usize) -> (String, usize) {
    let (line_start, line_end) = line_bounds(message, at);
    let before = &message[line_start..at];
    // Go back `context` characters from the match.
    let mut start = before
        .char_indices()
        .rev()
        .nth(context.saturating_sub(1))
        .map_or(0, |(i, _)| i);
    if context == 0 {
        start = before.len();
    }
    if start > 0
        && let Some(sp) = before[start..].find(char::is_whitespace)
    {
        // Begin after the first space of the window (only when it does not swallow the match).
        let after = start + sp + 1;
        if after <= before.len() {
            start = after;
        }
    }
    let abs_start = line_start + start;
    let mut text = String::new();
    if abs_start > 0 {
        text.push_str(ELLIPSIS);
    }
    let pos = text.len() + (at - abs_start);
    let (shown, was_cut) = cut(&message[abs_start..line_end], max);
    text.push_str(shown);
    if was_cut {
        text.push_str(ELLIPSIS);
    }
    if message.contains('\n') {
        text.push_str(MORE_LINES);
    }
    (text, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_cut_and_more_lines() {
        assert_eq!(first_line("short", 100), "short");
        assert_eq!(first_line("abcdef", 3), "abc…");
        assert_eq!(first_line("one\r\ntwo", 100), "one  ↵");
        // Cut at a char boundary (č is 2 bytes).
        assert_eq!(first_line("ččč", 3), "č…");
    }

    #[test]
    fn around_keeps_context_before_the_match() {
        let msg = format!("{} needle tail", "word ".repeat(40));
        let at = msg.find("needle").unwrap();
        let (text, pos) = around(&msg, at, 12, 1000);
        assert!(text.starts_with('…'), "{text}");
        assert_eq!(&text[pos..pos + 6], "needle");
        // Starts after a space, not in the middle of a word.
        assert!(text["…".len()..].starts_with("word"), "{text}");
        assert!(text.ends_with("needle tail"));
    }

    #[test]
    fn around_at_line_start_has_no_ellipsis() {
        let (text, pos) = around("needle here", 0, 24, 1000);
        assert_eq!(text, "needle here");
        assert_eq!(pos, 0);
        // Short prefix: whole line from its start.
        let (text, pos) = around("ab needle", 3, 24, 1000);
        assert_eq!(text, "ab needle");
        assert_eq!(pos, 3);
    }

    #[test]
    fn around_match_on_a_later_line() {
        let msg = "first line\r\nsecond with needle\r\nthird";
        let at = msg.find("needle").unwrap();
        let (text, pos) = around(msg, at, 24, 1000);
        assert_eq!(text, "…second with needle  ↵");
        assert_eq!(&text[pos..pos + 6], "needle");
    }

    #[test]
    fn around_without_context_and_with_cut() {
        let msg = format!("{}needle{}", "x".repeat(50), "y".repeat(50));
        let at = msg.find("needle").unwrap();
        let (text, pos) = around(&msg, at, 0, 10);
        assert_eq!(text, "…needleyyyy…");
        assert_eq!(pos, "…".len());
    }

    #[test]
    fn around_multibyte_context() {
        let msg = "žžžžžžžžžž needle";
        let at = msg.find("needle").unwrap();
        let (text, pos) = around(msg, at, 3, 1000);
        assert_eq!(&text[pos..pos + 6], "needle");
        assert!(text.is_char_boundary(pos));
    }
}
