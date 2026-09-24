use crate::color::{Color, ColorMode, paint, paint_depth};
use crate::json_tree::{JsonNode, JsonScalar, escape_display_str};
use crate::xml_tree::XmlNode;

pub fn render_json(
    node: &JsonNode,
    root_label: &str,
    max_depth: Option<usize>,
    array_limit: Option<usize>,
    mode: ColorMode,
) -> String {
    let mut out = String::new();
    match node {
        JsonNode::Scalar(s) => out.push_str(&scalar_line(root_label, s, mode)),
        _ => {
            out.push_str(root_label);
            out.push('\n');
            render_json_children(node, "", 0, max_depth, array_limit, mode, &mut out);
        }
    }
    out
}

fn scalar_line(label: &str, s: &JsonScalar, mode: ColorMode) -> String {
    let value_str = paint(&s.display(), s.color(), mode);
    format!("{label}: {value_str}\n")
}

#[allow(clippy::too_many_arguments)]
fn render_json_children(
    node: &JsonNode,
    prefix: &str,
    depth: usize,
    max_depth: Option<usize>,
    array_limit: Option<usize>,
    mode: ColorMode,
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
        let branch_str = paint_depth(&format!("{prefix}{branch}"), depth, mode.enabled());
        let label_str = paint(&escape_display_str(&label), Color::Key, mode);
        match child {
            JsonNode::Scalar(s) => {
                out.push_str(&scalar_line(&format!("{branch_str}{label_str}"), s, mode));
            }
            // An empty object/array has no children to descend into, so
            // without this it prints as a bare key indistinguishable from a
            // collapsed container -- show its (empty) shape instead, same
            // as a scalar leaf (issue #122).
            JsonNode::Object(f) if f.is_empty() => {
                let empty = paint("{}", Color::Structural, mode);
                out.push_str(&format!("{branch_str}{label_str}: {empty}\n"));
            }
            JsonNode::Array(items) if items.is_empty() => {
                let empty = paint("[]", Color::Structural, mode);
                out.push_str(&format!("{branch_str}{label_str}: {empty}\n"));
            }
            _ if max_depth.is_some_and(|d| depth + 1 >= d) => {
                let ellipsis = paint("…", Color::Structural, mode);
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
                    mode,
                    out,
                );
            }
        }
    }
    if truncated > 0 {
        let branch_str = paint_depth(&format!("{prefix}└── "), depth, mode.enabled());
        let msg = paint(&format!("… ({truncated} more)"), Color::Structural, mode);
        out.push_str(&format!("{branch_str}{msg}\n"));
    }
}

pub fn render_xml(node: &XmlNode, max_depth: Option<usize>, mode: ColorMode) -> String {
    let mut out = String::new();
    out.push_str(&xml_label(node, mode));
    out.push('\n');
    render_xml_children(node, "", 0, max_depth, mode, &mut out);
    out
}

fn xml_label(node: &XmlNode, mode: ColorMode) -> String {
    let mut label = paint(&escape_display_str(&node.name), Color::Key, mode);
    if !node.attributes.is_empty() {
        let attrs: Vec<String> = node
            .attributes
            .iter()
            .map(|(k, v)| {
                format!(
                    "{}=\"{}\"",
                    paint(&escape_display_str(k), Color::Key, mode),
                    paint(&escape_display_str(v), Color::Str, mode)
                )
            })
            .collect();
        label.push_str(&format!(" [{}]", attrs.join(" ")));
    }
    if let Some(text) = &node.text {
        label.push_str(&format!(
            ": {}",
            paint(&escape_display_str(text), Color::Str, mode)
        ));
    }
    label
}

fn render_xml_children(
    node: &XmlNode,
    prefix: &str,
    depth: usize,
    max_depth: Option<usize>,
    mode: ColorMode,
    out: &mut String,
) {
    let len = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last = i + 1 == len;
        let branch = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };
        let branch_str = paint_depth(&format!("{prefix}{branch}"), depth, mode.enabled());
        if !child.children.is_empty() && max_depth.is_some_and(|d| depth + 1 >= d) {
            // `xml_label` (not just the bare name) so a truncated element's
            // attributes and text stay visible -- only its children are cut
            // off (issue #129).
            let label = xml_label(child, mode);
            let ellipsis = paint("…", Color::Structural, mode);
            out.push_str(&format!("{branch_str}{label} {ellipsis}\n"));
            continue;
        }
        out.push_str(&format!("{branch_str}{}\n", xml_label(child, mode)));
        let next_prefix = format!("{prefix}{child_prefix}");
        render_xml_children(child, &next_prefix, depth + 1, max_depth, mode, out);
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
        let output = render_json(&node, "root", None, None, ColorMode::Off);
        assert_eq!(
            output,
            "root\n├── name: \"Alice\"\n└── tags\n    ├── [0]: \"admin\"\n    └── [1]: \"user\"\n"
        );
    }

    #[test]
    fn renders_json_tree_with_color() {
        let value = json!({"name": "Alice"});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, None, ColorMode::Ansi16);
        assert!(output.contains("\x1b[36mname\x1b[0m"));
        assert!(output.contains("\x1b[32m\"Alice\"\x1b[0m"));
    }

    #[test]
    fn renders_json_tree_with_truecolor() {
        let value = json!({"name": "Alice"});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, None, ColorMode::Truecolor);
        assert!(output.contains("\x1b[38;2;86;182;194mname\x1b[0m"));
        assert!(output.contains("\x1b[38;2;152;195;121m\"Alice\"\x1b[0m"));
    }

    #[test]
    fn json_guide_lines_are_tinted_by_depth_not_all_the_same_dim_gray() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, None, ColorMode::Ansi16);
        // depth 0's guide line ("user"'s own branch) keeps the original
        // plain dim; depth 1's guide line ("name"'s branch) gets a
        // distinct tint -- verifying the two don't collapse to one color.
        assert!(output.contains("\x1b[2m└── \x1b[0m"));
        assert!(output.contains("\x1b[2;34m    └── \x1b[0m"));
    }

    #[test]
    fn xml_guide_lines_are_tinted_by_depth() {
        let doc = roxmltree::Document::parse("<root><a><b/></a></root>").unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let output = render_xml(&node, None, ColorMode::Ansi16);
        assert!(output.contains("\x1b[2m└── \x1b[0m"));
        assert!(output.contains("\x1b[2;34m    └── \x1b[0m"));
    }

    #[test]
    fn escapes_control_bytes_in_a_json_object_key_not_just_scalar_values() {
        // JsonScalar::display() already escapes values; a malicious key
        // needs the same treatment or it reaches the terminal raw.
        let value = json!({"before\u{1b}after": 1});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, None, ColorMode::Off);
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("before\\u001bafter"));
    }

    #[test]
    fn renders_a_scalar_root_as_its_own_key_value_line_instead_of_nothing() {
        let value = json!(42);
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "count", None, None, ColorMode::Off);
        assert_eq!(output, "count: 42\n");
    }

    #[test]
    fn renders_a_colored_scalar_root() {
        let value = json!("Alice");
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "name", None, None, ColorMode::Ansi16);
        assert!(output.contains("\x1b[32m\"Alice\"\x1b[0m"));
        assert!(output.starts_with("name: "));
    }

    #[test]
    fn truncates_json_tree_at_depth() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", Some(1), None, ColorMode::Off);
        assert_eq!(output, "root\n└── user: …\n");
    }

    #[test]
    fn array_limit_truncates_a_large_array_with_a_summary_line() {
        let value = json!({"tags": ["a", "b", "c", "d", "e"]});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, Some(2), ColorMode::Off);
        assert_eq!(
            output,
            "root\n└── tags\n    ├── [0]: \"a\"\n    ├── [1]: \"b\"\n    └── … (3 more)\n"
        );
    }

    #[test]
    fn array_limit_does_not_affect_object_field_counts() {
        let value = json!({"a": 1, "b": 2, "c": 3});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, Some(2), ColorMode::Off);
        assert_eq!(output, "root\n├── a: 1\n├── b: 2\n└── c: 3\n");
    }

    #[test]
    fn array_limit_is_a_no_op_when_the_array_is_already_within_the_limit() {
        let value = json!({"tags": ["a", "b"]});
        let node = JsonNode::from_value(&value);
        let output = render_json(&node, "root", None, Some(5), ColorMode::Off);
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
        let output = render_xml(&node, None, ColorMode::Off);
        assert_eq!(output, "person [id=\"1\"]\n└── name: Alice\n");
    }

    #[test]
    fn escapes_control_bytes_in_xml_attribute_values_and_text() {
        // ESC (0x1b) isn't a legal XML character even via a numeric
        // reference, but DEL (0x7f) is -- this is the exact byte the
        // adversarial review reproduced getting through unescaped.
        let xml = "<root a=\"before&#x7f;after\">text&#x7f;here</root>";
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let output = render_xml(&node, None, ColorMode::Off);
        assert!(!output.contains('\u{7f}'));
        assert!(output.contains("before\\u007fafter"));
        assert!(output.contains("text\\u007fhere"));
    }

    #[test]
    fn escapes_control_bytes_in_an_xml_element_name() {
        // Element/attribute *names* can't contain a raw or referenced
        // control character per the XML Name grammar, but this guards the
        // escaping path itself regardless of whether real XML can trigger
        // it, matching how JSON keys are covered even though JSON's own
        // grammar is more permissive there.
        let xml = "<root><ok>x</ok></root>";
        let doc = roxmltree::Document::parse(xml).unwrap();
        let mut node = XmlNode::from_document(&doc).unwrap();
        node.children[0].name = "before\u{1b}after".to_string();
        let output = render_xml(&node, None, ColorMode::Off);
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("before\\u001bafter"));
    }

    #[test]
    fn escapes_control_bytes_in_an_xml_element_name_truncated_by_max_depth() {
        // The ellipsis branch (`--depth` cutting off a node with children)
        // is a separate render path from `xml_label` and was missed on the
        // first pass of this fix.
        let xml = "<root><ok><inner>x</inner></ok></root>";
        let doc = roxmltree::Document::parse(xml).unwrap();
        let mut node = XmlNode::from_document(&doc).unwrap();
        node.children[0].name = "before\u{1b}after".to_string();
        let output = render_xml(&node, Some(1), ColorMode::Off);
        assert!(!output.contains('\u{1b}'));
        // No colon: `xml_label` already appends its own `: text` when the
        // truncated element has text, so `label` + `: …` would double up
        // (issue #129's depth-truncation fix).
        assert!(output.contains("before\\u001bafter …"));
    }

    #[test]
    fn renders_xml_tree_with_color() {
        let xml = r#"<person id="1"><name>Alice</name></person>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let output = render_xml(&node, None, ColorMode::Ansi16);
        assert!(output.contains("\x1b[36mperson\x1b[0m"));
        assert!(output.contains("\x1b[32mAlice\x1b[0m"));
    }
}
