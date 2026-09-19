use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;

pub fn render_json(node: &JsonNode, root_label: &str, max_depth: Option<usize>) -> String {
    let mut out = String::new();
    out.push_str(root_label);
    out.push('\n');
    render_json_children(node, "", 0, max_depth, &mut out);
    out
}

fn render_json_children(
    node: &JsonNode,
    prefix: &str,
    depth: usize,
    max_depth: Option<usize>,
    out: &mut String,
) {
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), v))
            .collect(),
        JsonNode::Scalar(_) => Vec::new(),
    };
    let len = entries.len();
    for (i, (label, child)) in entries.into_iter().enumerate() {
        let is_last = i + 1 == len;
        let branch = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        match child {
            JsonNode::Scalar(s) => out.push_str(&format!("{prefix}{branch}{label}: {s}\n")),
            _ if max_depth.is_some_and(|d| depth + 1 >= d) => {
                out.push_str(&format!("{prefix}{branch}{label}: \u{2026}\n"));
            }
            _ => {
                out.push_str(&format!("{prefix}{branch}{label}\n"));
                let next_prefix = format!("{prefix}{child_prefix}");
                render_json_children(child, &next_prefix, depth + 1, max_depth, out);
            }
        }
    }
}

pub fn render_xml(node: &XmlNode, max_depth: Option<usize>) -> String {
    let mut out = String::new();
    out.push_str(&xml_label(node));
    out.push('\n');
    render_xml_children(node, "", 0, max_depth, &mut out);
    out
}

fn xml_label(node: &XmlNode) -> String {
    let mut label = node.name.clone();
    if !node.attributes.is_empty() {
        let attrs: Vec<String> = node
            .attributes
            .iter()
            .map(|(k, v)| format!("{k}=\"{v}\""))
            .collect();
        label.push_str(&format!(" [{}]", attrs.join(" ")));
    }
    if let Some(text) = &node.text {
        label.push_str(&format!(": {text}"));
    }
    label
}

fn render_xml_children(
    node: &XmlNode,
    prefix: &str,
    depth: usize,
    max_depth: Option<usize>,
    out: &mut String,
) {
    let len = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last = i + 1 == len;
        let branch = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        if !child.children.is_empty() && max_depth.is_some_and(|d| depth + 1 >= d) {
            out.push_str(&format!("{prefix}{branch}{}: \u{2026}\n", child.name));
            continue;
        }
        out.push_str(&format!("{prefix}{branch}{}\n", xml_label(child)));
        let next_prefix = format!("{prefix}{child_prefix}");
        render_xml_children(child, &next_prefix, depth + 1, max_depth, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_nested_json_tree() {
        let value = json!({"name": "Alice", "tags": ["admin", "user"]});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None);
        assert_eq!(
            output,
            "root\n├── name: Alice\n└── tags\n    ├── [0]: admin\n    └── [1]: user\n"
        );
    }

    #[test]
    fn truncates_json_tree_at_depth() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", Some(1));
        assert_eq!(output, "root\n└── user: …\n");
    }

    #[test]
    fn renders_xml_tree_with_attributes_and_text() {
        let xml = r#"<person id="1"><name>Alice</name></person>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let output = render_xml(&node, None);
        assert_eq!(output, "person [id=\"1\"]\n└── name: Alice\n");
    }
}
