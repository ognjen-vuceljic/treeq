use super::state::Line;
use crate::color::Color as TqColor;
use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use std::collections::HashSet;

pub(super) fn flatten_json(
    node: &JsonNode,
    path: &[String],
    depth: usize,
    collapsed: &HashSet<Vec<String>>,
    out: &mut Vec<Line>,
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
        });
        if has_children && !collapsed.contains(&child_path) {
            flatten_json(child, &child_path, depth + 1, collapsed, out);
        }
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
        flatten_json(&node, &[], 0, &HashSet::new(), &mut out);

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
        assert_eq!(out[1].value.as_ref().unwrap().0, "Alice");

        assert_eq!(out[2].key, "tags");
        assert_eq!(out[2].path, path(&["user", "tags"]));

        assert_eq!(out[3].key, "[0]");
        assert_eq!(out[3].path, path(&["user", "tags", "[0]"]));
        assert_eq!(out[3].value.as_ref().unwrap().0, "admin");

        assert_eq!(out[4].key, "[1]");
        assert_eq!(out[4].path, path(&["user", "tags", "[1]"]));
        assert_eq!(out[4].value.as_ref().unwrap().0, "user");
    }

    #[test]
    fn does_not_recurse_into_collapsed_json_containers() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let mut collapsed = HashSet::new();
        collapsed.insert(path(&["user"]));
        let mut out = Vec::new();
        flatten_json(&node, &[], 0, &collapsed, &mut out);

        // The collapsed container's own line is still shown, just not its children.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "user");
        assert!(out[0].has_children);
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
}
