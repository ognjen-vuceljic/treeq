/// Turns JSONC into plain JSON by blanking `//` and `/* */` comments and
/// trailing commas. Every removed byte becomes a space (newlines kept), so
/// line and column numbers in parse errors still point into the original.
pub fn strip(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut pending_comma: Option<usize> = None;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('/', Some('/')) => blank_line_comment(c, &mut chars, &mut out),
            ('/', Some('*')) => blank_block_comment(c, &mut chars, &mut out),
            ('"', _) => {
                in_string = true;
                pending_comma = None;
                out.push(c);
            }
            (',', _) => {
                pending_comma = Some(out.len());
                out.push(c);
            }
            ('}' | ']', _) => {
                if let Some(i) = pending_comma.take() {
                    out.replace_range(i..=i, " ");
                }
                out.push(c);
            }
            (c, _) => {
                if !c.is_whitespace() {
                    pending_comma = None;
                }
                out.push(c);
            }
        }
    }
    out
}

fn blank(c: char, out: &mut String) {
    if matches!(c, '\n' | '\r') {
        out.push(c);
    } else {
        out.extend(std::iter::repeat_n(' ', c.len_utf8()));
    }
}

fn blank_line_comment(
    first: char,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    out: &mut String,
) {
    blank(first, out);
    while let Some(&c) = chars.peek() {
        if c == '\n' || c == '\r' {
            break;
        }
        blank(c, out);
        chars.next();
    }
}

/// An unterminated block comment runs to the end of input, like most parsers.
fn blank_block_comment(
    first: char,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    out: &mut String,
) {
    blank(first, out);
    chars.next().inspect(|&star| blank(star, out));
    let mut prev = ' ';
    for c in chars.by_ref() {
        blank(c, out);
        if prev == '*' && c == '/' {
            break;
        }
        prev = c;
    }
}

/// True when `input` is an object/array document that is invalid JSON but
/// valid once stripped (a leading comment is fine): the sniff for
/// extension-less input, so truly broken JSON keeps its JSON error.
pub fn parses_after_strip(input: &str) -> bool {
    let stripped = strip(input);
    stripped.trim_start().starts_with(['{', '['])
        && serde_json::from_str::<serde::de::IgnoredAny>(&stripped).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> String {
        serde_json::from_str::<serde_json::Value>(&strip(src))
            .unwrap()
            .to_string()
    }

    #[test]
    fn strips_line_and_block_comments() {
        assert_eq!(parse("{\"a\":1, // c\n\"b\":/* x */2}"), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn strips_trailing_commas_in_objects_and_arrays_even_past_comments() {
        assert_eq!(parse("[1,2,]"), "[1,2]");
        assert_eq!(parse("{\"a\":[1,],}"), r#"{"a":[1]}"#);
        assert_eq!(parse("[1, // c\n]"), "[1]");
        assert_eq!(parse("[1, /* c */ ]"), "[1]");
    }

    #[test]
    fn comment_markers_and_commas_inside_strings_are_untouched() {
        assert_eq!(
            parse(r#"{"u":"http://x/*y*/","c":"a,]","q":"\" // z"}"#),
            r#"{"u":"http://x/*y*/","c":"a,]","q":"\" // z"}"#
        );
    }

    #[test]
    fn a_comma_followed_by_a_value_is_kept() {
        assert_eq!(parse("[1,/* c */2]"), "[1,2]");
    }

    #[test]
    fn block_comment_edge_cases() {
        assert_eq!(parse("[1,/**/2]"), "[1,2]");
        assert_eq!(parse("[1,/***/2]"), "[1,2]");
        // `/*/` does not close itself
        assert_eq!(strip("[/*/ ]"), "[     ");
        assert_eq!(strip("[1 /* never closed"), "[1                ");
    }

    #[test]
    fn positions_are_preserved_byte_for_byte() {
        let src = "{\"é\": 1, // ünï\n/* two\nlines */ \"b\": [1,],}";
        let out = strip(src);
        assert_eq!(out.len(), src.len());
        assert_eq!(out.matches('\n').count(), src.matches('\n').count());
    }

    #[test]
    fn crlf_line_endings_are_kept() {
        assert_eq!(strip("1 // c\r\n"), "1     \r\n");
    }

    #[test]
    fn plain_json_is_returned_unchanged() {
        let src = r#"{"a":[1,2,{"b":null}],"c":"d"}"#;
        assert_eq!(strip(src), src);
    }

    #[test]
    fn sniff_requires_stripping_to_help() {
        assert!(parses_after_strip("{\"a\":1,}"));
        assert!(!parses_after_strip("{\"a\":"));
        assert!(!parses_after_strip("{\"a\" 1}"));
        assert!(parses_after_strip("// header\n{\"a\":1}"));
        assert!(!parses_after_strip("// only a comment\n"));
        assert!(!parses_after_strip("42 // not an object"));
    }

    #[test]
    fn large_document_with_comments_strips_and_parses() {
        let body: String = (0..20_000).map(|i| format!("{i}, // n{i}\n")).collect();
        let v: serde_json::Value = serde_json::from_str(&strip(&format!("[{body}]"))).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 20_000);
    }
}
