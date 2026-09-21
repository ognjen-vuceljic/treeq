//! Formats parse errors as an input snippet with a caret, like rustc/eslint,
//! instead of forwarding the raw parser error string.

use std::fmt::Write as _;

const CONTEXT_LINES: usize = 2;
const TAB_WIDTH: usize = 4;
const MAX_LINE_LEN: usize = 200;

/// `serde_json`'s column is a *byte* offset, not a char count, so it's
/// marked accordingly for correct caret placement on multi-byte UTF-8.
pub fn json_error_context(input: &str, err: &serde_json::Error) -> String {
    format_error_context(
        "JSON",
        input,
        err.line(),
        err.column(),
        true,
        &err.to_string(),
    )
}

/// `roxmltree`'s column is already a *character* offset (unlike
/// `serde_json`'s byte offset), so it's marked accordingly.
pub fn xml_error_context(input: &str, err: &roxmltree::Error) -> String {
    let pos = err.pos();
    format_error_context(
        "XML",
        input,
        pos.row as usize,
        pos.col as usize,
        false,
        &err.to_string(),
    )
}

fn format_error_context(
    kind: &str,
    input: &str,
    line: usize,
    column: usize,
    column_is_byte_offset: bool,
    message: &str,
) -> String {
    let lines: Vec<&str> = input.lines().collect();
    // Clamp once and reuse everywhere: an EOF error reports a line number
    // one past the last real line.
    let display_line = line.min(lines.len().max(1));
    let mut out = String::new();
    let _ = writeln!(
        out,
        "error: invalid {kind} at line {display_line}, column {column}\n"
    );
    out.push_str(&render_snippet(
        &lines,
        display_line,
        column,
        column_is_byte_offset,
    ));
    let _ = writeln!(out);
    // The underlying parser can format an offending byte straight into its
    // error message (e.g. roxmltree's `InvalidChar` variants use `{}` on
    // the raw `char`), so this needs the same sanitization as the snippet
    // line above it, not just the source text.
    out.push_str(&sanitize_control_chars(message));
    out
}

fn render_snippet(
    lines: &[&str],
    line: usize,
    column: usize,
    column_is_byte_offset: bool,
) -> String {
    if lines.is_empty() || line == 0 {
        return String::new();
    }
    let (start, end) = context_range(lines.len(), line);
    let width = end.to_string().len();
    let mut out = String::new();
    for (i, text) in lines.iter().enumerate().take(end).skip(start) {
        let text = &sanitize_control_chars(text);
        let lineno = i + 1;
        if lineno == line {
            let char_col = char_column(text, column, column_is_byte_offset);
            let expanded = expand_tabs(text);
            let caret_col = expanded_char_column(text, char_col);
            let (display, caret_col) = truncate_for_display(&expanded, caret_col);
            let _ = writeln!(out, "{:>width$} | {}", lineno, display, width = width);
            out.push_str(&caret_line(width, caret_col));
        } else {
            let _ = writeln!(
                out,
                "{:>width$} | {}",
                lineno,
                expand_tabs(text),
                width = width
            );
        }
    }
    out
}

fn context_range(total: usize, line: usize) -> (usize, usize) {
    let start = line.saturating_sub(1).saturating_sub(CONTEXT_LINES);
    let end = (line + CONTEXT_LINES).min(total);
    (start, end)
}

/// `column` is 1-indexed; a byte offset is translated via char boundaries,
/// since a byte offset can split a multi-byte character's column count.
fn char_column(line: &str, column: usize, column_is_byte_offset: bool) -> usize {
    if !column_is_byte_offset {
        return column.saturating_sub(1);
    }
    let target_byte = column.saturating_sub(1);
    line.char_indices()
        .take_while(|(byte_idx, _)| *byte_idx < target_byte)
        .count()
}

fn expand_tabs(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB_WIDTH))
}

/// Replaces C0/DEL control characters (other than `\t`, which `expand_tabs`
/// handles) with a single visible placeholder, one-for-one so caret column
/// math stays correct. A malformed file's offending line is echoed verbatim
/// into this error snippet, so without this a crafted invalid JSON/XML file
/// could inject a raw terminal escape sequence via its own parse error.
fn sanitize_control_chars(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c != '\t' && (c.is_control() || c == '\u{7f}') {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

fn expanded_char_column(original_line: &str, char_col: usize) -> usize {
    expand_tabs(&original_line.chars().take(char_col).collect::<String>())
        .chars()
        .count()
}

fn truncate_for_display(line: &str, caret_col: usize) -> (String, usize) {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= MAX_LINE_LEN {
        return (line.to_string(), caret_col);
    }
    let half = MAX_LINE_LEN / 2;
    let start = caret_col
        .saturating_sub(half)
        .min(chars.len().saturating_sub(MAX_LINE_LEN));
    let end = (start + MAX_LINE_LEN).min(chars.len());
    let mut display: String = chars[start..end].iter().collect();
    let mut adjusted = caret_col.saturating_sub(start);
    if start > 0 {
        display = format!("…{display}");
        adjusted += 1;
    }
    if end < chars.len() {
        display.push('…');
    }
    (display, adjusted)
}

fn caret_line(width: usize, char_col: usize) -> String {
    let indent = width + 3 + char_col;
    format!("{:indent$}^\n", "", indent = indent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json_err(input: &str) -> serde_json::Error {
        serde_json::from_str::<serde_json::Value>(input).unwrap_err()
    }

    fn parse_xml_err(input: &str) -> roxmltree::Error {
        roxmltree::Document::parse(input).unwrap_err()
    }

    #[test]
    fn reports_line_and_column_header() {
        let input = "{\n  \"a\": 1,\n  \"b\": 2\n  \"c\": 3\n}";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        assert!(out.contains(&format!("line {}", err.line())));
        assert!(out.contains(&format!("column {}", err.column())));
    }

    #[test]
    fn includes_context_lines_and_caret() {
        let input = "{\n  \"a\": 1,\n  \"b\": 2\n  \"c\": 3\n}";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        assert!(out.contains(&format!("{} | ", err.line())));
        assert!(out.contains('^'));
    }

    #[test]
    fn includes_raw_message() {
        let input = "{ \"a\": }";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        assert!(out.contains(&err.to_string()));
    }

    #[test]
    fn handles_error_on_first_line() {
        let input = "{,}";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        assert!(out.contains("line 1"));
        assert!(out.contains('^'));
    }

    #[test]
    fn handles_unclosed_brace_at_eof() {
        let input = "{\n  \"a\": 1\n";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        // EOF errors report a line number one past the last real line.
        let lines: Vec<&str> = input.lines().collect();
        let display_line = err.line().min(lines.len().max(1));
        assert!(out.contains(&format!("line {display_line}")));
        assert!(out.contains(&format!("{display_line} | ")));
        assert!(out.contains(&err.to_string()));
    }

    #[test]
    fn sanitizes_raw_control_bytes_in_the_offending_line_instead_of_echoing_them() {
        // A malformed file could otherwise inject a real terminal escape
        // sequence into stderr via its own parse-error snippet.
        let input = "{\"a\": \u{1b}]0;PWNED\u{7}bad}";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        assert!(
            !out.contains('\u{1b}'),
            "raw ESC byte must not survive the snippet"
        );
        assert!(
            !out.contains('\u{7}'),
            "raw BEL byte must not survive the snippet"
        );
    }

    #[test]
    fn sanitizes_raw_control_bytes_that_the_underlying_parser_formats_into_its_own_message() {
        // roxmltree's InvalidChar-family errors format the offending byte
        // straight into the message text via `{}` on the raw `char`, a
        // separate leak from the echoed source snippet above.
        let input = "<root a=\u{1b}\"v\"/>";
        let err = parse_xml_err(input);
        let out = xml_error_context(input, &err);
        assert!(
            !out.contains('\u{1b}'),
            "raw ESC byte from the parser's own message must not survive"
        );
    }

    #[test]
    fn does_not_panic_on_empty_input() {
        let input = "";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        assert!(out.contains(&err.to_string()));
    }

    #[test]
    fn caret_accounts_for_multi_byte_utf8_characters_before_the_error() {
        // "日本語" is 3 chars / 9 bytes; caret must use char position, not byte count.
        let input = "{\n  \"a\": \"日本語\", x\n}";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        let caret_line = out.lines().find(|l| l.contains('^')).unwrap();
        let error_line = out
            .lines()
            .find(|l| l.trim_start().starts_with("2 |"))
            .unwrap();
        let caret_col = caret_line.chars().position(|c| c == '^').unwrap();
        let x_col = error_line.chars().position(|c| c == 'x').unwrap();
        assert_eq!(caret_col, x_col, "caret must align under 'x': {out}");
    }

    #[test]
    fn caret_accounts_for_leading_tabs() {
        let input = "{\n\t\"a\": bad\n}";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        let caret_line = out.lines().find(|l| l.contains('^')).unwrap();
        let error_line = out
            .lines()
            .find(|l| l.trim_start().starts_with("2 |"))
            .unwrap();
        let caret_col = caret_line.chars().position(|c| c == '^').unwrap();
        let b_col = error_line.chars().position(|c| c == 'b').unwrap();
        assert_eq!(caret_col, b_col, "caret must align under 'bad': {out}");
    }

    #[test]
    fn truncates_pathologically_long_lines_around_the_caret() {
        let long_value = "x".repeat(500);
        let input = format!("{{\"a\": \"{long_value}, bad}}");
        let err = parse_json_err(&input);
        let out = json_error_context(&input, &err);
        assert!(
            out.lines().all(|l| l.chars().count() < 300),
            "no line should dump the full 500+ char value: {out}"
        );
        assert!(out.contains('…'));
    }

    #[test]
    fn xml_error_context_renders_a_snippet_and_caret() {
        let input = "<root><unclosed></root>";
        let err = parse_xml_err(input);
        let out = xml_error_context(input, &err);
        assert!(out.contains("invalid XML"));
        assert!(out.contains('^'));
        assert!(out.contains("1 | "));
    }
}
