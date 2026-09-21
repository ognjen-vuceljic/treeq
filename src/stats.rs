use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonStats {
    pub max_depth: usize,
    pub objects: usize,
    pub arrays: usize,
    pub scalars: usize,
}

pub fn json_stats(node: &JsonNode) -> JsonStats {
    let mut stats = JsonStats {
        max_depth: 0,
        objects: 0,
        arrays: 0,
        scalars: 0,
    };
    walk_json(node, 0, &mut stats);
    stats
}

fn walk_json(node: &JsonNode, depth: usize, stats: &mut JsonStats) {
    stats.max_depth = stats.max_depth.max(depth);
    match node {
        JsonNode::Object(fields) => {
            stats.objects += 1;
            for (_, child) in fields {
                walk_json(child, depth + 1, stats);
            }
        }
        JsonNode::Array(items) => {
            stats.arrays += 1;
            for child in items {
                walk_json(child, depth + 1, stats);
            }
        }
        JsonNode::Scalar(_) => stats.scalars += 1,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlStats {
    pub max_depth: usize,
    pub elements: usize,
    pub attributes: usize,
    pub text_nodes: usize,
}

pub fn xml_stats(node: &XmlNode) -> XmlStats {
    let mut stats = XmlStats {
        max_depth: 0,
        elements: 0,
        attributes: 0,
        text_nodes: 0,
    };
    walk_xml(node, 0, &mut stats);
    stats
}

fn walk_xml(node: &XmlNode, depth: usize, stats: &mut XmlStats) {
    stats.max_depth = stats.max_depth.max(depth);
    stats.elements += 1;
    stats.attributes += node.attributes.len();
    if node.text.is_some() {
        stats.text_nodes += 1;
    }
    for child in &node.children {
        walk_xml(child, depth + 1, stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn computes_json_stats() {
        let value = json!({"a": {"b": [1, 2, 3]}, "c": "x"});
        let node = JsonNode::from_value(&value);
        let stats = json_stats(&node);
        assert_eq!(
            stats,
            JsonStats {
                max_depth: 3,
                objects: 2,
                arrays: 1,
                scalars: 4,
            }
        );
    }

    #[test]
    fn computes_xml_stats() {
        let xml = r#"<root a="1"><child>text</child><child2/></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let stats = xml_stats(&node);
        assert_eq!(
            stats,
            XmlStats {
                max_depth: 1,
                elements: 3,
                attributes: 1,
                text_nodes: 1,
            }
        );
    }
}
