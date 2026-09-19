//! Formats parse errors as an input snippet with a caret, similar to
//! compiler/linter diagnostics (rustc, eslint), instead of forwarding the
//! raw parser error string.

use std::fmt::Write as _;

/// Number of context lines to show before/after the erroring line.
const CONTEXT_LINES: usize = 2;

/// Builds a snippet + caret diagnostic for a `serde_json` parse error.
pub fn json_error_context(input: &str, err: &serde_json::Error) -> String {
    format_error_context("JSON", input, err.line(), err.column(), &err.to_string())
}

/// Builds a snippet + caret diagnostic for a `roxmltree` parse error.
///
/// `roxmltree::Error::pos()` returns a 1-indexed row/column, mirroring
/// `serde_json`'s `.line()`/`.column()`, so we can reuse the same renderer.
pub fn xml_error_context(input: &str, err: &roxmltree::Error) -> String {
    let pos = err.pos();
    format_error_context(
        "XML",
        input,
        pos.row as usize,
        pos.col as usize,
        &err.to_string(),
    )
}

fn format_error_context(
    kind: &str,
    input: &str,
    line: usize,
    column: usize,
    message: &str,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "error: invalid {kind} at line {line}, column {column}\n"
    );
    out.push_str(&render_snippet(input, line, column));
    let _ = writeln!(out);
    out.push_str(message);
    out
}

fn render_snippet(input: &str, line: usize, column: usize) -> String {
    let lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() || line == 0 {
        return String::new();
    }
    let line = line.min(lines.len());
    let (start, end) = context_range(lines.len(), line);
    let width = end.to_string().len();
    let mut out = String::new();
    for (i, text) in lines.iter().enumerate().take(end).skip(start) {
        let lineno = i + 1;
        let _ = writeln!(out, "{:>width$} | {}", lineno, text, width = width);
        if lineno == line {
            out.push_str(&caret_line(width, column));
        }
    }
    out
}

/// Returns the (inclusive-exclusive) range of line indices to display,
/// clamped to `[0, total)`.
fn context_range(total: usize, line: usize) -> (usize, usize) {
    let start = line.saturating_sub(1).saturating_sub(CONTEXT_LINES);
    let end = (line + CONTEXT_LINES).min(total);
    (start, end)
}

fn caret_line(width: usize, column: usize) -> String {
    let indent = width + 3 + column.saturating_sub(1);
    format!("{:indent$}^\n", "", indent = indent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json_err(input: &str) -> serde_json::Error {
        serde_json::from_str::<serde_json::Value>(input).unwrap_err()
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
        // The erroring line and at least one neighboring context line show up.
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
        assert!(out.contains(&format!("line {}", err.line())));
        assert!(out.contains(&err.to_string()));
    }

    #[test]
    fn does_not_panic_on_empty_input() {
        let input = "";
        let err = parse_json_err(input);
        let out = json_error_context(input, &err);
        assert!(out.contains(&err.to_string()));
    }
}
