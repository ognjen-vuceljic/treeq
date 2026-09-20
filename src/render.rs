use crate::color::{Color, paint};
use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;

pub fn render_json(
    node: &JsonNode,
    root_label: &str,
    max_depth: Option<usize>,
    use_color: bool,
) -> String {
    let mut out = String::new();
    out.push_str(root_label);
    out.push('\n');
    render_json_children(node, "", 0, max_depth, use_color, &mut out);
    out
}

fn render_json_children(
    node: &JsonNode,
    prefix: &str,
    depth: usize,
    max_depth: Option<usize>,
    use_color: bool,
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
        let branch_str = paint(&format!("{prefix}{branch}"), Color::Structural, use_color);
        let label_str = paint(&label, Color::Key, use_color);
        match child {
            JsonNode::Scalar(s) => {
                let value_str = paint(&s.display(), s.color(), use_color);
                out.push_str(&format!("{branch_str}{label_str}: {value_str}\n"));
            }
            _ if max_depth.is_some_and(|d| depth + 1 >= d) => {
                let ellipsis = paint("…", Color::Structural, use_color);
                out.push_str(&format!("{branch_str}{label_str}: {ellipsis}\n"));
            }
            _ => {
                out.push_str(&format!("{branch_str}{label_str}\n"));
                let next_prefix = format!("{prefix}{child_prefix}");
                render_json_children(child, &next_prefix, depth + 1, max_depth, use_color, out);
            }
        }
    }
}

pub fn render_xml(node: &XmlNode, max_depth: Option<usize>, use_color: bool) -> String {
    let mut out = String::new();
    out.push_str(&xml_label(node, use_color));
    out.push('\n');
    render_xml_children(node, "", 0, max_depth, use_color, &mut out);
    out
}

fn xml_label(node: &XmlNode, use_color: bool) -> String {
    let mut label = paint(&node.name, Color::Key, use_color);
    if !node.attributes.is_empty() {
        let attrs: Vec<String> = node
            .attributes
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}=\"{}\"",
                    paint(k, Color::Key, use_color),
                    paint(v, Color::Str, use_color)
                )
            })
            .collect();
        label.push_str(&format!(" [{}]", attrs.join(" ")));
    }
    if let Some(text) = &node.text {
        label.push_str(&format!(": {}", paint(text, Color::Str, use_color)));
    }
    label
}

fn render_xml_children(
    node: &XmlNode,
    prefix: &str,
    depth: usize,
    max_depth: Option<usize>,
    use_color: bool,
    out: &mut String,
) {
    let len = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last = i + 1 == len;
        let branch = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        let branch_str = paint(&format!("{prefix}{branch}"), Color::Structural, use_color);
        if !child.children.is_empty() && max_depth.is_some_and(|d| depth + 1 >= d) {
            let name_str = paint(&child.name, Color::Key, use_color);
            let ellipsis = paint("…", Color::Structural, use_color);
            out.push_str(&format!("{branch_str}{name_str}: {ellipsis}\n"));
            continue;
        }
        out.push_str(&format!("{branch_str}{}\n", xml_label(child, use_color)));
        let next_prefix = format!("{prefix}{child_prefix}");
        render_xml_children(child, &next_prefix, depth + 1, max_depth, use_color, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_nested_json_tree_without_color() {
        let value = json!({"name": "Alice", "tags": ["admin", "user"]});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, false);
        assert_eq!(
            output,
            "root\n├── name: \"Alice\"\n└── tags\n    ├── [0]: \"admin\"\n    └── [1]: \"user\"\n"
        );
    }

    #[test]
    fn renders_json_tree_with_color() {
        let value = json!({"name": "Alice"});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, true);
        assert!(output.contains("\x1b[36mname\x1b[0m"));
        assert!(output.contains("\x1b[32m\"Alice\"\x1b[0m"));
    }

    #[test]
    fn truncates_json_tree_at_depth() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", Some(1), false);
        assert_eq!(output, "root\n└── user: …\n");
    }

    #[test]
    fn renders_xml_tree_with_attributes_and_text_without_color() {
        let xml = r#"<person id="1"><name>Alice</name></person>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let output = render_xml(&node, None, false);
        assert_eq!(output, "person [id=\"1\"]\n└── name: Alice\n");
    }

    #[test]
    fn renders_xml_tree_with_color() {
        let xml = r#"<person id="1"><name>Alice</name></person>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let output = render_xml(&node, None, true);
        assert!(output.contains("\x1b[36mperson\x1b[0m"));
        assert!(output.contains("\x1b[32mAlice\x1b[0m"));
    }
}
