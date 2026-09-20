use super::state::Line;
use crate::color::Color as TqColor;
use crate::json_tree::{JsonNode, JsonScalar};
use crate::xml_tree::XmlNode;
use std::collections::HashSet;

fn plural_suffix(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn json_type_label(node: &JsonNode) -> String {
    match node {
        JsonNode::Object(fields) => {
            format!(
                "object ({} field{})",
                fields.len(),
                plural_suffix(fields.len())
            )
        }
        JsonNode::Array(items) => {
            format!("array ({} item{})", items.len(), plural_suffix(items.len()))
        }
        JsonNode::Scalar(JsonScalar::Str(s)) => {
            let len = s.chars().count();
            format!("string ({len} char{})", plural_suffix(len))
        }
        JsonNode::Scalar(JsonScalar::Number(_)) => "number".to_string(),
        JsonNode::Scalar(JsonScalar::Bool(_)) => "boolean".to_string(),
        JsonNode::Scalar(JsonScalar::Null) => "null".to_string(),
    }
}

fn xml_type_label(node: &XmlNode) -> String {
    if !node.children.is_empty() {
        let n = node.children.len();
        let word = if n == 1 { "child" } else { "children" };
        format!("element ({n} {word})")
    } else if let Some(text) = &node.text {
        let len = text.chars().count();
        format!("string ({len} char{})", plural_suffix(len))
    } else {
        "empty element".to_string()
    }
}

const ARRAY_PREVIEW_LIMIT: usize = 200;

/// Cosmetic only — `Line::is_array_summary`, not this text, is what
/// `keys::handle_key` checks, so a real key spelled the same way can't collide.
const ARRAY_TRUNCATION_LABEL: &str = "\u{2026}more";

pub(super) fn flatten_json(
    node: &JsonNode,
    path: &[String],
    depth: usize,
    collapsed: &HashSet<Vec<String>>,
    array_overrides: &HashSet<Vec<String>>,
    out: &mut Vec<Line>,
) {
    let is_array = matches!(node, JsonNode::Array(_));
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), v))
            .collect(),
        JsonNode::Scalar(_) => return,
    };
    let total = entries.len();
    let visible = if is_array && !array_overrides.contains(path) {
        ARRAY_PREVIEW_LIMIT.min(total)
    } else {
        total
    };
    let truncated = total - visible;
    for (label, child) in entries.into_iter().take(visible) {
        let mut child_path = path.to_vec();
        child_path.push(label.clone());
        let (value, has_children) = match child {
            JsonNode::Scalar(s) => (Some((s.display(), s.color())), false),
            _ => (None, true),
        };
        out.push(Line {
            depth,
            key: label,
            value,
            path: child_path.clone(),
            has_children,
            is_array_summary: false,
            type_label: json_type_label(child),
        });
        if has_children && !collapsed.contains(&child_path) {
            flatten_json(
                child,
                &child_path,
                depth + 1,
                collapsed,
                array_overrides,
                out,
            );
        }
    }
    if truncated > 0 {
        let mut marker_path = path.to_vec();
        marker_path.push(ARRAY_TRUNCATION_LABEL.to_string());
        out.push(Line {
            depth,
            key: ARRAY_TRUNCATION_LABEL.to_string(),
            value: Some((
                format!("{truncated} more (Tab to show all)"),
                TqColor::Structural,
            )),
            path: marker_path,
            has_children: false,
            is_array_summary: true,
            type_label: "array preview marker".to_string(),
        });
    }
}

pub(super) fn flatten_xml(
    node: &XmlNode,
    path: &[String],
    depth: usize,
    collapsed: &HashSet<Vec<String>>,
    out: &mut Vec<Line>,
) {
    for child in &node.children {
        let mut child_path = path.to_vec();
        child_path.push(child.name.clone());
        let has_children = !child.children.is_empty();
        let value = child.text.clone().map(|t| (t, TqColor::Str));
        out.push(Line {
            depth,
            key: child.name.clone(),
            value,
            path: child_path.clone(),
            has_children,
            is_array_summary: false,
            type_label: xml_type_label(child),
        });
        if has_children && !collapsed.contains(&child_path) {
            flatten_xml(child, &child_path, depth + 1, collapsed, out);
        }
    }
}

pub(super) fn collect_container_paths_json(
    node: &JsonNode,
    path: &[String],
    out: &mut HashSet<Vec<String>>,
) {
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), v))
            .collect(),
        JsonNode::Scalar(_) => return,
    };
    for (label, child) in entries {
        if matches!(child, JsonNode::Scalar(_)) {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(label);
        out.insert(child_path.clone());
        collect_container_paths_json(child, &child_path, out);
    }
}

pub(super) fn collect_container_paths_xml(
    node: &XmlNode,
    path: &[String],
    out: &mut HashSet<Vec<String>>,
) {
    for child in &node.children {
        if child.children.is_empty() {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(child.name.clone());
        out.insert(child_path.clone());
        collect_container_paths_xml(child, &child_path, out);
    }
}

pub(super) fn search_text(path: &[String], value: Option<&str>) -> String {
    let joined = path.join(".");
    match value {
        Some(v) => format!("{joined}: {v}"),
        None => joined,
    }
}

pub(super) fn collect_all_paths_json(
    node: &JsonNode,
    path: &[String],
    out: &mut Vec<(Vec<String>, String)>,
) {
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), v))
            .collect(),
        JsonNode::Scalar(_) => return,
    };
    for (label, child) in entries {
        let mut child_path = path.to_vec();
        child_path.push(label);
        let value = match child {
            JsonNode::Scalar(s) => Some(s.display()),
            _ => None,
        };
        out.push((
            child_path.clone(),
            search_text(&child_path, value.as_deref()),
        ));
        collect_all_paths_json(child, &child_path, out);
    }
}

pub(super) fn collect_all_paths_xml(
    node: &XmlNode,
    path: &[String],
    out: &mut Vec<(Vec<String>, String)>,
) {
    for child in &node.children {
        let mut child_path = path.to_vec();
        child_path.push(child.name.clone());
        out.push((
            child_path.clone(),
            search_text(&child_path, child.text.as_deref()),
        ));
        collect_all_paths_xml(child, &child_path, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flattens_nested_json_when_expanded() {
        let value = json!({"user": {"name": "Alice", "tags": ["admin", "user"]}});
        let node = JsonNode::from_value(&value);
        let mut out = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &HashSet::new(), &mut out);

        // "user" + "name" + "tags" + the array's own two elements, since
        // flattening recurses into an expanded array just like an object.
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].key, "user");
        assert_eq!(out[0].path, path(&["user"]));
        assert!(out[0].has_children);
        assert!(out[0].value.is_none());

        assert_eq!(out[1].key, "name");
        assert_eq!(out[1].path, path(&["user", "name"]));
        assert!(!out[1].has_children);
        assert_eq!(out[1].value.as_ref().unwrap().0, "\"Alice\"");

        assert_eq!(out[2].key, "tags");
        assert_eq!(out[2].path, path(&["user", "tags"]));

        assert_eq!(out[3].key, "[0]");
        assert_eq!(out[3].path, path(&["user", "tags", "[0]"]));
        assert_eq!(out[3].value.as_ref().unwrap().0, "\"admin\"");

        assert_eq!(out[4].key, "[1]");
        assert_eq!(out[4].path, path(&["user", "tags", "[1]"]));
        assert_eq!(out[4].value.as_ref().unwrap().0, "\"user\"");
    }

    #[test]
    fn does_not_recurse_into_collapsed_json_containers() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let mut collapsed = HashSet::new();
        collapsed.insert(path(&["user"]));
        let mut out = Vec::new();
        flatten_json(&node, &[], 0, &collapsed, &HashSet::new(), &mut out);

        // The collapsed container's own line is still shown, just not its children.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "user");
        assert!(out[0].has_children);
    }

    #[test]
    fn truncates_a_large_array_with_a_summary_line() {
        let items: Vec<serde_json::Value> = (0..ARRAY_PREVIEW_LIMIT + 5)
            .map(|i| serde_json::json!(i))
            .collect();
        let value = json!({ "tags": items });
        let node = JsonNode::from_value(&value);
        let mut out = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &HashSet::new(), &mut out);

        // "tags" + ARRAY_PREVIEW_LIMIT visible elements + 1 summary line.
        assert_eq!(out.len(), 1 + ARRAY_PREVIEW_LIMIT + 1);
        let summary = out.last().unwrap();
        assert_eq!(summary.key, ARRAY_TRUNCATION_LABEL);
        assert!(!summary.has_children);
        assert!(summary.is_array_summary);
        assert_eq!(
            summary.value.as_ref().unwrap().0,
            "5 more (Tab to show all)"
        );
        assert_eq!(summary.path, path(&["tags", ARRAY_TRUNCATION_LABEL]));
    }

    #[test]
    fn array_override_shows_every_element_and_no_summary_line() {
        let items: Vec<serde_json::Value> = (0..ARRAY_PREVIEW_LIMIT + 5)
            .map(|i| serde_json::json!(i))
            .collect();
        let value = json!({ "tags": items });
        let node = JsonNode::from_value(&value);
        let mut array_overrides = HashSet::new();
        array_overrides.insert(path(&["tags"]));
        let mut out = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &array_overrides, &mut out);

        // "tags" + every element, no summary line.
        assert_eq!(out.len(), 1 + ARRAY_PREVIEW_LIMIT + 5);
        assert!(out.iter().all(|l| !l.is_array_summary));
    }

    #[test]
    fn small_arrays_are_never_truncated() {
        let value = json!({"tags": ["a", "b"]});
        let node = JsonNode::from_value(&value);
        let mut out = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &HashSet::new(), &mut out);

        assert_eq!(out.len(), 3); // "tags" + 2 elements, no summary line
        assert!(out.iter().all(|l| !l.is_array_summary));
    }

    #[test]
    fn flattens_xml_elements_with_text_children() {
        let xml = r#"<root><user><name>Alice</name></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let mut out = Vec::new();
        flatten_xml(&node, &[], 0, &HashSet::new(), &mut out);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].key, "user");
        assert!(out[0].has_children);
        assert_eq!(out[1].key, "name");
        assert_eq!(out[1].value.as_ref().unwrap().0, "Alice");
        assert_eq!(out[1].path, path(&["user", "name"]));
    }

    #[test]
    fn xml_attributes_are_not_yet_surfaced_as_a_line_value() {
        // Line has no field for attributes today, so an element with only
        // attributes (no text) renders with no value segment at all.
        let xml = r#"<root><user id="1"></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let mut out = Vec::new();
        flatten_xml(&node, &[], 0, &HashSet::new(), &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "user");
        assert!(out[0].value.is_none());
    }

    #[test]
    fn does_not_recurse_into_collapsed_xml_containers() {
        let xml = r#"<root><user><name>Alice</name></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let mut collapsed = HashSet::new();
        collapsed.insert(path(&["user"]));
        let mut out = Vec::new();
        flatten_xml(&node, &[], 0, &collapsed, &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "user");
    }

    #[test]
    fn collects_all_json_container_paths_regardless_of_collapse_state() {
        let value = json!({"user": {"name": "Alice", "tags": ["admin"]}, "flag": true});
        let node = JsonNode::from_value(&value);
        let mut out = HashSet::new();
        collect_container_paths_json(&node, &[], &mut out);

        assert_eq!(out.len(), 2);
        assert!(out.contains(&path(&["user"])));
        assert!(out.contains(&path(&["user", "tags"])));
        assert!(!out.contains(&path(&["user", "name"])));
        assert!(!out.contains(&path(&["flag"])));
    }

    #[test]
    fn collects_all_xml_container_paths() {
        let xml = r#"<root><user><name>Alice</name></user><flag>true</flag></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let mut out = HashSet::new();
        collect_container_paths_xml(&node, &[], &mut out);

        assert_eq!(out.len(), 1);
        assert!(out.contains(&path(&["user"])));
        assert!(!out.contains(&path(&["user", "name"])));
        assert!(!out.contains(&path(&["flag"])));
    }

    #[test]
    fn collects_every_json_node_path_including_leaves() {
        let value = json!({"user": {"name": "Alice", "tags": ["admin"]}, "flag": true});
        let node = JsonNode::from_value(&value);
        let mut out = Vec::new();
        collect_all_paths_json(&node, &[], &mut out);
        let paths: Vec<_> = out.iter().map(|(p, _)| p.clone()).collect();

        assert!(paths.contains(&path(&["user"])));
        assert!(paths.contains(&path(&["user", "name"])));
        assert!(paths.contains(&path(&["user", "tags"])));
        assert!(paths.contains(&path(&["user", "tags", "[0]"])));
        assert!(paths.contains(&path(&["flag"])));
    }

    #[test]
    fn collects_every_json_leafs_search_text_includes_its_value() {
        let value = json!({"author": "user0"});
        let node = JsonNode::from_value(&value);
        let mut out = Vec::new();
        collect_all_paths_json(&node, &[], &mut out);

        let (_, text) = out
            .iter()
            .find(|(p, _)| p == &path(&["author"]))
            .expect("author path must be collected");
        assert_eq!(text, "author: \"user0\"");
    }

    #[test]
    fn collects_every_xml_node_path_including_leaves() {
        let xml = r#"<root><user><name>Alice</name></user><flag>true</flag></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let mut out = Vec::new();
        collect_all_paths_xml(&node, &[], &mut out);
        let paths: Vec<_> = out.iter().map(|(p, _)| p.clone()).collect();

        assert!(paths.contains(&path(&["user"])));
        assert!(paths.contains(&path(&["user", "name"])));
        assert!(paths.contains(&path(&["flag"])));
    }

    #[test]
    fn collects_every_xml_leafs_search_text_includes_its_text() {
        let xml = r#"<root><author>user0</author></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let mut out = Vec::new();
        collect_all_paths_xml(&node, &[], &mut out);

        let (_, text) = out
            .iter()
            .find(|(p, _)| p == &path(&["author"]))
            .expect("author path must be collected");
        assert_eq!(text, "author: user0");
    }

    #[test]
    fn json_type_label_describes_each_kind_of_node() {
        assert_eq!(
            json_type_label(&JsonNode::from_value(&json!({"a": 1, "b": 2}))),
            "object (2 fields)"
        );
        assert_eq!(
            json_type_label(&JsonNode::from_value(&json!({"a": 1}))),
            "object (1 field)"
        );
        assert_eq!(
            json_type_label(&JsonNode::from_value(&json!([1, 2, 3]))),
            "array (3 items)"
        );
        assert_eq!(
            json_type_label(&JsonNode::from_value(&json!("hello"))),
            "string (5 chars)"
        );
        assert_eq!(json_type_label(&JsonNode::from_value(&json!(42))), "number");
        assert_eq!(
            json_type_label(&JsonNode::from_value(&json!(true))),
            "boolean"
        );
        assert_eq!(json_type_label(&JsonNode::from_value(&json!(null))), "null");
    }

    #[test]
    fn xml_type_label_describes_each_kind_of_node() {
        let with_children = roxmltree::Document::parse("<a><b/><c/></a>").unwrap();
        assert_eq!(
            xml_type_label(&XmlNode::from_document(&with_children)),
            "element (2 children)"
        );

        let with_text = roxmltree::Document::parse("<a>hello</a>").unwrap();
        assert_eq!(
            xml_type_label(&XmlNode::from_document(&with_text)),
            "string (5 chars)"
        );

        let empty = roxmltree::Document::parse("<a/>").unwrap();
        assert_eq!(
            xml_type_label(&XmlNode::from_document(&empty)),
            "empty element"
        );
    }
}
