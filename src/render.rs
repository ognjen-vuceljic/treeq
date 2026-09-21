use crate::color::{Color, paint};
use crate::json_tree::{JsonNode, JsonScalar};
use crate::xml_tree::XmlNode;

pub fn render_json(
    node: &JsonNode,
    root_label: &str,
    max_depth: Option<usize>,
    array_limit: Option<usize>,
    use_color: bool,
) -> String {
    let mut out = String::new();
    match node {
        JsonNode::Scalar(s) => out.push_str(&scalar_line(root_label, s, use_color)),
        _ => {
            out.push_str(root_label);
            out.push('\n');
            render_json_children(node, "", 0, max_depth, array_limit, use_color, &mut out);
        }
    }
    out
}

fn scalar_line(label: &str, s: &JsonScalar, use_color: bool) -> String {
    let value_str = paint(&s.display(), s.color(), use_color);
    format!("{label}: {value_str}\n")
}

#[allow(clippy::too_many_arguments)]
fn render_json_children(
    node: &JsonNode,
    prefix: &str,
    depth: usize,
    max_depth: Option<usize>,
    array_limit: Option<usize>,
    use_color: bool,
    out: &mut String,
) {
    let is_array = matches!(node, JsonNode::Array(_));
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), v))
            .collect(),
        JsonNode::Scalar(_) => Vec::new(),
    };
    let total = entries.len();
    let visible = if is_array {
        array_limit.map_or(total, |limit| limit.min(total))
    } else {
        total
    };
    let truncated = total - visible;
    for (i, (label, child)) in entries.into_iter().take(visible).enumerate() {
        let is_last = i + 1 == visible && truncated == 0;
        let branch = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        let branch_str = paint(&format!("{prefix}{branch}"), Color::Structural, use_color);
        let label_str = paint(&label, Color::Key, use_color);
        match child {
            JsonNode::Scalar(s) => {
                out.push_str(&scalar_line(
                    &format!("{branch_str}{label_str}"),
                    s,
                    use_color,
                ));
            }
            _ if max_depth.is_some_and(|d| depth + 1 >= d) => {
                let ellipsis = paint("…", Color::Structural, use_color);
                out.push_str(&format!("{branch_str}{label_str}: {ellipsis}\n"));
            }
            _ => {
                out.push_str(&format!("{branch_str}{label_str}\n"));
                let next_prefix = format!("{prefix}{child_prefix}");
                render_json_children(
                    child,
                    &next_prefix,
                    depth + 1,
                    max_depth,
                    array_limit,
                    use_color,
                    out,
                );
            }
        }
    }
    if truncated > 0 {
        let branch_str = paint(&format!("{prefix}└── "), Color::Structural, use_color);
        let msg = paint(
            &format!("… ({truncated} more)"),
            Color::Structural,
            use_color,
        );
        out.push_str(&format!("{branch_str}{msg}\n"));
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
        let output = render_json(&node, "root", None, None, false);
        assert_eq!(
            output,
            "root\n├── name: \"Alice\"\n└── tags\n    ├── [0]: \"admin\"\n    └── [1]: \"user\"\n"
        );
    }

    #[test]
    fn renders_json_tree_with_color() {
        let value = json!({"name": "Alice"});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, None, true);
        assert!(output.contains("\x1b[36mname\x1b[0m"));
        assert!(output.contains("\x1b[32m\"Alice\"\x1b[0m"));
    }

    #[test]
    fn renders_a_scalar_root_as_its_own_key_value_line_instead_of_nothing() {
        let value = json!(42);
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "count", None, None, false);
        assert_eq!(output, "count: 42\n");
    }

    #[test]
    fn renders_a_colored_scalar_root() {
        let value = json!("Alice");
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "name", None, None, true);
        assert!(output.contains("\x1b[32m\"Alice\"\x1b[0m"));
        assert!(output.starts_with("name: "));
    }

    #[test]
    fn truncates_json_tree_at_depth() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", Some(1), None, false);
        assert_eq!(output, "root\n└── user: …\n");
    }

    #[test]
    fn array_limit_truncates_a_large_array_with_a_summary_line() {
        let value = json!({"tags": ["a", "b", "c", "d", "e"]});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, Some(2), false);
        assert_eq!(
            output,
            "root\n└── tags\n    ├── [0]: \"a\"\n    ├── [1]: \"b\"\n    └── … (3 more)\n"
        );
    }

    #[test]
    fn array_limit_does_not_affect_object_field_counts() {
        let value = json!({"a": 1, "b": 2, "c": 3});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, Some(2), false);
        assert_eq!(output, "root\n├── a: 1\n├── b: 2\n└── c: 3\n");
    }

    #[test]
    fn array_limit_is_a_no_op_when_the_array_is_already_within_the_limit() {
        let value = json!({"tags": ["a", "b"]});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, Some(5), false);
        assert_eq!(
            output,
            "root\n└── tags\n    ├── [0]: \"a\"\n    └── [1]: \"b\"\n"
        );
    }

    #[test]
    fn renders_xml_tree_with_attributes_and_text_without_color() {
        let xml = r#"<person id="1"><name>Alice</name></person>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let output = render_xml(&node, None, false);
        assert_eq!(output, "person [id=\"1\"]\n└── name: Alice\n");
    }

    #[test]
    fn renders_xml_tree_with_color() {
        let xml = r#"<person id="1"><name>Alice</name></person>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let output = render_xml(&node, None, true);
        assert!(output.contains("\x1b[36mperson\x1b[0m"));
        assert!(output.contains("\x1b[32mAlice\x1b[0m"));
    }
}
