use crate::json_tree::{JsonNode, escape_path_segment};
use crate::xml_tree::XmlNode;

/// Lists every path in the tree, one entry per node (containers and
/// leaves), in the same dotted format `find_json_path` expects -- so output
/// round-trips straight into `--path`. Each segment is escaped (see
/// `escape_path_segment`), both for control bytes (a raw one was never
/// meaningful to round-trip through a shell command line anyway) and for a
/// literal `.` in the key itself, which would otherwise be indistinguishable
/// from the `.` segment separator (issue #61).
pub fn json_paths(node: &JsonNode) -> Vec<String> {
    let mut out = Vec::new();
    walk_json(node, &[], &mut out);
    out
}

fn walk_json(node: &JsonNode, path: &[String], out: &mut Vec<String>) {
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        JsonNode::Scalar(_) => return,
    };
    for (label, child) in entries {
        let mut child_path = path.to_vec();
        child_path.push(label);
        out.push(display_path(&child_path));
        walk_json(child, &child_path, out);
    }
}

fn display_path(path: &[String]) -> String {
    path.iter()
        .map(|s| escape_path_segment(s))
        .collect::<Vec<_>>()
        .join(".")
}

pub fn xml_paths(node: &XmlNode) -> Vec<String> {
    let mut out = Vec::new();
    walk_xml(node, &[], &mut out);
    out
}

fn walk_xml(node: &XmlNode, path: &[String], out: &mut Vec<String>) {
    for child in &node.children {
        let mut child_path = path.to_vec();
        child_path.push(child.name.clone());
        out.push(display_path(&child_path));
        walk_xml(child, &child_path, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json_tree::{JsonScalar, find_json_path};
    use serde_json::json;

    #[test]
    fn lists_all_json_paths() {
        let value = json!({"user": {"name": "Alice", "tags": ["admin", "user"]}});
        let node = JsonNode::from_value(&value);
        let paths = json_paths(&node);
        assert_eq!(
            paths,
            vec![
                "user".to_string(),
                "user.name".to_string(),
                "user.tags".to_string(),
                "user.tags.0".to_string(),
                "user.tags.1".to_string(),
            ]
        );
    }

    #[test]
    fn escapes_control_bytes_in_a_json_key_within_a_listed_path() {
        let value = json!({"before\u{1b}after": 1});
        let node = JsonNode::from_value(&value);
        let paths = json_paths(&node);
        assert_eq!(paths, vec!["before\\u001bafter".to_string()]);
    }

    #[test]
    fn escapes_a_literal_dot_in_a_json_key_so_it_is_distinguishable_from_the_path_separator() {
        let value = json!({"a.b": {"c": 1}});
        let node = JsonNode::from_value(&value);
        let paths = json_paths(&node);
        assert_eq!(paths, vec!["a\\.b".to_string(), "a\\.b.c".to_string()]);
        // Round-trips back through the same segment split `--path` uses.
        assert_eq!(
            find_json_path(&node, "a\\.b.c").unwrap(),
            &JsonNode::Scalar(JsonScalar::Number("1".to_string()))
        );
    }

    #[test]
    fn lists_all_xml_paths() {
        let xml = r#"<root><user><name>Alice</name></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let paths = xml_paths(&node);
        assert_eq!(paths, vec!["user".to_string(), "user.name".to_string()]);
    }
}
