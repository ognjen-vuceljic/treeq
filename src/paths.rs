use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;

/// Lists every path in the tree, one entry per node (containers and
/// leaves), in the same dotted, bracket-free format `find_json_path`
/// expects — so output can round-trip straight into `--path`.
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
        out.push(child_path.join("."));
        walk_json(child, &child_path, out);
    }
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
        out.push(child_path.join("."));
        walk_xml(child, &child_path, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn lists_all_xml_paths() {
        let xml = r#"<root><user><name>Alice</name></user></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let paths = xml_paths(&node);
        assert_eq!(paths, vec!["user".to_string(), "user.name".to_string()]);
    }
}
