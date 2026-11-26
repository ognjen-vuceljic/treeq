//! `--diff <OTHER>`: structural diff of two documents. Object key order is
//! ignored; arrays are compared index by index. Numbers compare by their
//! written digits (`1` and `1.0` differ).

use crate::json_tree::{JsonNode, to_compact_json};
use crate::paths::display_path;

#[derive(Debug, PartialEq)]
pub enum Change {
    Added {
        path: String,
        value: String,
    },
    Removed {
        path: String,
        value: String,
    },
    Changed {
        path: String,
        from: String,
        to: String,
    },
}

pub fn diff(a: &JsonNode, b: &JsonNode) -> Vec<Change> {
    let mut out = Vec::new();
    walk(a, b, &mut Vec::new(), &mut out);
    out
}

fn walk(a: &JsonNode, b: &JsonNode, path: &mut Vec<String>, out: &mut Vec<Change>) {
    match (a, b) {
        (JsonNode::Object(x), JsonNode::Object(y)) => diff_objects(x, y, path, out),
        (JsonNode::Array(x), JsonNode::Array(y)) => diff_arrays(x, y, path, out),
        (JsonNode::Scalar(x), JsonNode::Scalar(y)) if x == y => {}
        _ => out.push(Change::Changed {
            path: shown(path),
            from: preview(a),
            to: preview(b),
        }),
    }
}

fn diff_objects(
    x: &[(String, JsonNode)],
    y: &[(String, JsonNode)],
    path: &mut Vec<String>,
    out: &mut Vec<Change>,
) {
    let other: std::collections::HashMap<&str, &JsonNode> =
        y.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let mine: std::collections::HashSet<&str> = x.iter().map(|(k, _)| k.as_str()).collect();
    for (key, va) in x {
        path.push(key.clone());
        match other.get(key.as_str()) {
            Some(vb) => walk(va, vb, path, out),
            None => out.push(Change::Removed {
                path: shown(path),
                value: preview(va),
            }),
        }
        path.pop();
    }
    for (key, vb) in y.iter().filter(|(k, _)| !mine.contains(k.as_str())) {
        path.push(key.clone());
        out.push(Change::Added {
            path: shown(path),
            value: preview(vb),
        });
        path.pop();
    }
}

fn diff_arrays(x: &[JsonNode], y: &[JsonNode], path: &mut Vec<String>, out: &mut Vec<Change>) {
    for i in 0..x.len().max(y.len()) {
        path.push(i.to_string());
        match (x.get(i), y.get(i)) {
            (Some(a), Some(b)) => walk(a, b, path, out),
            (Some(a), None) => out.push(Change::Removed {
                path: shown(path),
                value: preview(a),
            }),
            (None, Some(b)) => out.push(Change::Added {
                path: shown(path),
                value: preview(b),
            }),
            (None, None) => {}
        }
        path.pop();
    }
}

fn shown(path: &[String]) -> String {
    if path.is_empty() {
        "(root)".to_string()
    } else {
        display_path(path)
    }
}

const PREVIEW_MAX: usize = 80;

/// Full compact JSON; only the text output shortens it (see `clip`).
fn preview(node: &JsonNode) -> String {
    to_compact_json(node)
}

fn clip(full: &str) -> String {
    match full.char_indices().nth(PREVIEW_MAX) {
        Some((cut, _)) => format!("{}…", &full[..cut]),
        None => full.to_string(),
    }
}

pub fn text_line(c: &Change) -> String {
    match c {
        Change::Added { path, value } => format!("+ {path}: {}", clip(value)),
        Change::Removed { path, value } => format!("- {path}: {}", clip(value)),
        Change::Changed { path, from, to } => {
            format!("~ {path}: {} -> {}", clip(from), clip(to))
        }
    }
}

fn value(compact: &str) -> serde_json::Value {
    serde_json::from_str(compact).unwrap_or_else(|_| compact.into())
}

pub fn to_json(c: &Change) -> serde_json::Value {
    match c {
        Change::Added { path, value: v } => {
            serde_json::json!({"op": "added", "path": path, "value": value(v)})
        }
        Change::Removed { path, value: v } => {
            serde_json::json!({"op": "removed", "path": path, "value": value(v)})
        }
        Change::Changed { path, from, to } => {
            serde_json::json!({"op": "changed", "path": path, "from": value(from), "to": value(to)})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(a: &str, b: &str) -> Vec<String> {
        let (a, b): (JsonNode, JsonNode) = (
            serde_json::from_str(a).unwrap(),
            serde_json::from_str(b).unwrap(),
        );
        diff(&a, &b).iter().map(text_line).collect()
    }

    #[test]
    fn identical_documents_have_no_changes() {
        assert!(d(r#"{"a":[1,{"b":null}]}"#, r#"{"a":[1,{"b":null}]}"#).is_empty());
    }

    #[test]
    fn key_order_is_ignored() {
        assert!(d(r#"{"a":1,"b":2}"#, r#"{"b":2,"a":1}"#).is_empty());
    }

    #[test]
    fn reports_added_removed_and_changed_in_document_order() {
        assert_eq!(
            d(
                r#"{"a":1,"gone":true,"s":"x"}"#,
                r#"{"a":1,"s":"y","new":[1]}"#
            ),
            ["- gone: true", "~ s: \"x\" -> \"y\"", "+ new: [1]"]
        );
    }

    #[test]
    fn type_changes_are_reported_as_changed() {
        assert_eq!(d(r#"{"a":1}"#, r#"{"a":"1"}"#), ["~ a: 1 -> \"1\""]);
        assert_eq!(d(r#"{"a":[]}"#, r#"{"a":{}}"#), ["~ a: [] -> {}"]);
        assert_eq!(d(r#"{"a":null}"#, r#"{"a":0}"#), ["~ a: null -> 0"]);
    }

    #[test]
    fn arrays_compare_by_index_so_reorders_and_length_changes_show() {
        assert_eq!(d("[1,2,3]", "[1,3]"), ["~ 1: 2 -> 3", "- 2: 3"]);
        assert_eq!(d("[1]", "[1,2]"), ["+ 1: 2"]);
        assert_eq!(d("[1,2]", "[2,1]"), ["~ 0: 1 -> 2", "~ 1: 2 -> 1"]);
    }

    #[test]
    fn root_scalar_change_uses_the_root_label() {
        assert_eq!(d("1", "2"), ["~ (root): 1 -> 2"]);
    }

    #[test]
    fn numbers_compare_by_written_digits() {
        assert_eq!(d("1", "1.0"), ["~ (root): 1 -> 1.0"]);
        assert!(d("12345678901234567890", "12345678901234567890").is_empty());
    }

    #[test]
    fn paths_escape_dots_like_the_paths_listing() {
        assert_eq!(d(r#"{"a.b":1}"#, r#"{"a.b":2}"#), ["~ a\\.b: 1 -> 2"]);
    }

    #[test]
    fn long_values_are_clipped_in_text_but_whole_in_json() {
        let big = format!(r#"{{"k":"{}"}}"#, "x".repeat(200));
        let a: JsonNode = serde_json::from_str("{}").unwrap();
        let b: JsonNode = serde_json::from_str(&big).unwrap();
        let changes = diff(&a, &b);
        assert!(text_line(&changes[0]).ends_with('…'));
        assert_eq!(to_json(&changes[0])["value"].as_str().unwrap().len(), 200);
    }

    #[test]
    fn huge_arrays_diff_without_trouble() {
        let a = format!(
            "[{}]",
            (0..50_000)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let b = a.replace("49999", "0");
        assert_eq!(d(&a, &b), ["~ 49999: 49999 -> 0"]);
    }
}
