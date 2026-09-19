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
