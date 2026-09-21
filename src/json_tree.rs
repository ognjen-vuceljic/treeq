use crate::color::Color;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonScalar {
    Str(String),
    Number(String),
    Bool(bool),
    Null,
}

/// Escapes `\`, `"`, and C0 control characters (other than the printable
/// `\n`/`\r`/`\t` triad, which get their own short escapes) as `\u00XX`.
/// Without this, untrusted string content -- a JSON/YAML scalar value, but
/// also an object key, or an XML element name/attribute/text (any of which
/// can carry the same raw bytes) -- would be written to the terminal
/// verbatim, letting a crafted file inject terminal escape sequences (OSC
/// 52 clipboard writes, title-bar spoofing, ...) into whoever views it with
/// treeq. Used both for `JsonScalar::display()` and directly by callers
/// (`render.rs`, `tui/flatten.rs`) rendering any other untrusted text.
pub(crate) fn escape_display_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// `escape_display_str` plus escaping a literal `.` as `\.`, so a path
/// segment printed by `--paths` round-trips unambiguously back through
/// `--path`'s `.`-splitting (see `split_path_segments`) even when the
/// underlying key itself contains a dot (issue #61).
pub(crate) fn escape_path_segment(s: &str) -> String {
    escape_display_str(s).replace('.', "\\.")
}

/// Reverses `escape_path_segment` (and, for any segment typed directly by a
/// user rather than round-tripped from `--paths`, `escape_display_str`'s own
/// escapes) on one segment already isolated by `split_path_segments`. An
/// unrecognized `\X` passes `X` through literally rather than erroring, since
/// this only ever runs on a local CLI argument.
fn unescape_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut chars = segment.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('.') => out.push('.'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Splits a `--path` argument into raw segments on unescaped `.` -- a `.`
/// preceded by a backslash is part of an escaped literal dot (see
/// `escape_path_segment`) rather than a segment separator. Shared by
/// `find_json_path` and `find_xml_path`.
pub(crate) fn split_path_segments(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '.' => segments.push(std::mem::take(&mut current)),
            c => current.push(c),
        }
    }
    segments.push(current);
    segments.iter().map(|s| unescape_path_segment(s)).collect()
}

impl JsonScalar {
    /// Strings are quoted so the type of a value is recoverable from plain
    /// text alone (no ANSI color needed) — e.g. `"30"` (a string) reads
    /// differently from `30` (a number). Numbers/bools/null are already
    /// self-describing as bare identifiers and stay unquoted.
    pub fn display(&self) -> String {
        match self {
            JsonScalar::Str(s) => format!("\"{}\"", escape_display_str(s)),
            JsonScalar::Number(s) => s.clone(),
            JsonScalar::Bool(b) => b.to_string(),
            JsonScalar::Null => "null".to_string(),
        }
    }

    pub fn color(&self) -> Color {
        match self {
            JsonScalar::Str(_) => Color::Str,
            JsonScalar::Number(_) => Color::Number,
            JsonScalar::Bool(_) => Color::Bool,
            JsonScalar::Null => Color::Null,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsonNode {
    Object(Vec<(String, JsonNode)>),
    Array(Vec<JsonNode>),
    Scalar(JsonScalar),
}

impl JsonNode {
    pub fn from_value(value: &serde_json::Value) -> JsonNode {
        match value {
            serde_json::Value::Object(map) => JsonNode::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), JsonNode::from_value(v)))
                    .collect(),
            ),
            serde_json::Value::Array(items) => {
                JsonNode::Array(items.iter().map(JsonNode::from_value).collect())
            }
            serde_json::Value::String(s) => JsonNode::Scalar(JsonScalar::Str(s.clone())),
            serde_json::Value::Null => JsonNode::Scalar(JsonScalar::Null),
            serde_json::Value::Bool(b) => JsonNode::Scalar(JsonScalar::Bool(*b)),
            serde_json::Value::Number(n) => JsonNode::Scalar(JsonScalar::Number(n.to_string())),
        }
    }

    pub fn from_yaml_value(value: &serde_yaml::Value) -> Result<JsonNode, String> {
        match value {
            serde_yaml::Value::Null => Ok(JsonNode::Scalar(JsonScalar::Null)),
            serde_yaml::Value::Bool(b) => Ok(JsonNode::Scalar(JsonScalar::Bool(*b))),
            serde_yaml::Value::Number(n) => Ok(JsonNode::Scalar(JsonScalar::Number(n.to_string()))),
            serde_yaml::Value::String(s) => Ok(JsonNode::Scalar(JsonScalar::Str(s.clone()))),
            serde_yaml::Value::Sequence(items) => {
                let converted = items
                    .iter()
                    .map(JsonNode::from_yaml_value)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(JsonNode::Array(converted))
            }
            serde_yaml::Value::Mapping(map) => JsonNode::from_yaml_mapping(map),
            serde_yaml::Value::Tagged(_) => Err("YAML tags are not supported".to_string()),
        }
    }

    /// Note: YAML's `<<: *anchor` merge-key idiom is not merged — a `<<` key
    /// is kept as a literal object field pointing at the aliased mapping,
    /// same as any other key. Implementing real merge semantics (including
    /// `<<: [*a, *b]` and merge-vs-explicit-key precedence) is out of scope
    /// for this viewer; treeq shows the document's raw structure.
    fn from_yaml_mapping(map: &serde_yaml::Mapping) -> Result<JsonNode, String> {
        let mut fields = Vec::with_capacity(map.len());
        for (k, v) in map {
            let key = k
                .as_str()
                .ok_or_else(|| "YAML mapping keys must be strings".to_string())?;
            fields.push((key.to_string(), JsonNode::from_yaml_value(v)?));
        }
        Ok(JsonNode::Object(fields))
    }
}

pub fn find_json_path<'a>(node: &'a JsonNode, path: &str) -> Result<&'a JsonNode, String> {
    let mut current = node;
    for segment in split_path_segments(path) {
        current = match current {
            JsonNode::Object(fields) => fields
                .iter()
                .find(|(k, _)| *k == segment)
                .map(|(_, v)| v)
                .ok_or(segment)?,
            JsonNode::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|i| items.get(i))
                .ok_or(segment)?,
            JsonNode::Scalar(_) => return Err(segment),
        };
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_nested_object_and_array() {
        let value = json!({"name": "Alice", "tags": ["admin", "user"]});
        let node = JsonNode::from_value(&value);
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "name".to_string(),
                    JsonNode::Scalar(JsonScalar::Str("Alice".to_string()))
                ),
                (
                    "tags".to_string(),
                    JsonNode::Array(vec![
                        JsonNode::Scalar(JsonScalar::Str("admin".to_string())),
                        JsonNode::Scalar(JsonScalar::Str("user".to_string())),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn display_quotes_strings_and_escapes_embedded_quotes_and_backslashes() {
        assert_eq!(JsonScalar::Str("Alice".to_string()).display(), "\"Alice\"");
        assert_eq!(
            JsonScalar::Str("He said \"hi\"".to_string()).display(),
            "\"He said \\\"hi\\\"\""
        );
        assert_eq!(
            JsonScalar::Str("back\\slash".to_string()).display(),
            "\"back\\\\slash\""
        );
    }

    #[test]
    fn display_escapes_control_bytes_instead_of_passing_them_to_the_terminal_raw() {
        // A malicious JSON value could otherwise inject a real terminal
        // escape sequence (e.g. an OSC 52 clipboard write) into stdout.
        let evil = "before\u{1b}]52;c;aGFja2VkCg==\u{7}after";
        let out = JsonScalar::Str(evil.to_string()).display();
        assert!(
            !out.contains('\u{1b}'),
            "raw ESC byte must not survive display()"
        );
        assert!(
            !out.contains('\u{7}'),
            "raw BEL byte must not survive display()"
        );
        assert_eq!(out, "\"before\\u001b]52;c;aGFja2VkCg==\\u0007after\"");
    }

    #[test]
    fn display_uses_short_escapes_for_newline_tab_and_carriage_return() {
        assert_eq!(
            JsonScalar::Str("a\nb\tc\rd".to_string()).display(),
            "\"a\\nb\\tc\\rd\""
        );
    }

    #[test]
    fn display_leaves_number_bool_and_null_unquoted() {
        assert_eq!(JsonScalar::Number("30".to_string()).display(), "30");
        assert_eq!(JsonScalar::Bool(true).display(), "true");
        assert_eq!(JsonScalar::Null.display(), "null");
    }

    #[test]
    fn converts_number_bool_and_null_scalars() {
        let value = json!({"age": 30, "active": true, "middle_name": null});
        let node = JsonNode::from_value(&value);
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "age".to_string(),
                    JsonNode::Scalar(JsonScalar::Number("30".to_string()))
                ),
                (
                    "active".to_string(),
                    JsonNode::Scalar(JsonScalar::Bool(true))
                ),
                (
                    "middle_name".to_string(),
                    JsonNode::Scalar(JsonScalar::Null)
                ),
            ])
        );
    }

    #[test]
    fn finds_nested_path() {
        let value = json!({"user": {"tags": ["admin", "user"]}});
        let node = JsonNode::from_value(&value);
        let found = find_json_path(&node, "user.tags.1").unwrap();
        assert_eq!(
            found,
            &JsonNode::Scalar(JsonScalar::Str("user".to_string()))
        );
    }

    #[test]
    fn reports_first_unmatched_segment() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let err = find_json_path(&node, "user.missing.deeper").unwrap_err();
        assert_eq!(err, "missing");
    }

    #[test]
    fn resolves_a_key_containing_a_literal_dot_when_it_is_backslash_escaped() {
        let value = json!({"a.b": {"c": 1}});
        let node = JsonNode::from_value(&value);
        let found = find_json_path(&node, "a\\.b.c").unwrap();
        assert_eq!(
            found,
            &JsonNode::Scalar(JsonScalar::Number("1".to_string()))
        );
    }

    #[test]
    fn an_unescaped_dot_still_splits_a_dotted_key_into_two_segments() {
        // Without the `\.` escape, `a.b` reads as segments "a" then "b" --
        // this is the ambiguity issue #61 is about, not something this fix
        // changes: the escape is opt-in, not automatic disambiguation.
        let value = json!({"a.b": 1});
        let node = JsonNode::from_value(&value);
        let err = find_json_path(&node, "a.b").unwrap_err();
        assert_eq!(err, "a");
    }

    #[test]
    fn escape_path_segment_and_split_path_segments_round_trip_a_dotted_key() {
        let escaped = escape_path_segment("a.b");
        assert_eq!(escaped, "a\\.b");
        assert_eq!(split_path_segments(&escaped), vec!["a.b".to_string()]);
    }

    #[test]
    fn split_path_segments_round_trips_a_key_with_both_a_backslash_and_a_dot() {
        let escaped = escape_path_segment("a\\.b");
        assert_eq!(split_path_segments(&escaped), vec!["a\\.b".to_string()]);
    }

    #[test]
    fn converts_yaml_mapping_and_sequence() {
        let value: serde_yaml::Value =
            serde_yaml::from_str("name: Alice\ntags:\n  - admin\n  - user\n").unwrap();
        let node = JsonNode::from_yaml_value(&value).unwrap();
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "name".to_string(),
                    JsonNode::Scalar(JsonScalar::Str("Alice".to_string()))
                ),
                (
                    "tags".to_string(),
                    JsonNode::Array(vec![
                        JsonNode::Scalar(JsonScalar::Str("admin".to_string())),
                        JsonNode::Scalar(JsonScalar::Str("user".to_string())),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn converts_yaml_scalars() {
        let value: serde_yaml::Value =
            serde_yaml::from_str("age: 30\nactive: true\nmiddle_name: null\n").unwrap();
        let node = JsonNode::from_yaml_value(&value).unwrap();
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "age".to_string(),
                    JsonNode::Scalar(JsonScalar::Number("30".to_string()))
                ),
                (
                    "active".to_string(),
                    JsonNode::Scalar(JsonScalar::Bool(true))
                ),
                (
                    "middle_name".to_string(),
                    JsonNode::Scalar(JsonScalar::Null)
                ),
            ])
        );
    }

    #[test]
    fn errors_on_non_string_mapping_key() {
        let value: serde_yaml::Value = serde_yaml::from_str("1: one\n2: two\n").unwrap();
        let err = JsonNode::from_yaml_value(&value).unwrap_err();
        assert_eq!(err, "YAML mapping keys must be strings");
    }

    #[test]
    fn errors_on_yaml_tagged_value() {
        let value: serde_yaml::Value = serde_yaml::from_str("!Tag value").unwrap();
        let err = JsonNode::from_yaml_value(&value).unwrap_err();
        assert_eq!(err, "YAML tags are not supported");
    }
}
