use crate::json_tree::{JsonNode, JsonScalar};
use crate::xml_tree::XmlNode;
use std::collections::BTreeSet;

/// A merged shape inferred from one or more `JsonNode`s occupying the same
/// "slot" (the root, or a given field across every element of an array).
enum Shape {
    Object(Vec<(String, Shape)>),
    Array(Box<Shape>),
    Scalar(BTreeSet<String>),
    /// Nodes in this slot are not all the same kind (e.g. one is an object,
    /// another a string): fall back to a plain label union.
    Mixed(BTreeSet<String>),
}

fn scalar_type_name(scalar: &JsonScalar) -> &'static str {
    match scalar {
        JsonScalar::Str(_) => "string",
        JsonScalar::Number(_) => "number",
        JsonScalar::Bool(_) => "boolean",
        JsonScalar::Null => "null",
    }
}

fn kind_label(node: &JsonNode) -> String {
    match node {
        JsonNode::Object(_) => "object".to_string(),
        JsonNode::Array(_) => "array".to_string(),
        JsonNode::Scalar(s) => scalar_type_name(s).to_string(),
    }
}

fn infer_shape(nodes: &[&JsonNode]) -> Shape {
    if nodes.iter().all(|n| matches!(n, JsonNode::Object(_))) {
        return infer_object_shape(nodes);
    }
    if nodes.iter().all(|n| matches!(n, JsonNode::Array(_))) {
        return infer_array_shape(nodes);
    }
    if nodes.iter().all(|n| matches!(n, JsonNode::Scalar(_))) {
        let types = nodes
            .iter()
            .map(|n| match n {
                JsonNode::Scalar(s) => scalar_type_name(s).to_string(),
                _ => unreachable!(),
            })
            .collect();
        return Shape::Scalar(types);
    }
    Shape::Mixed(nodes.iter().map(|n| kind_label(n)).collect())
}

/// Field order follows first appearance across `nodes`, matching how JSON
/// objects are otherwise rendered.
fn object_field_order(nodes: &[&JsonNode]) -> Vec<String> {
    let mut order = Vec::new();
    for node in nodes {
        if let JsonNode::Object(fields) = node {
            for (name, _) in fields {
                if !order.contains(name) {
                    order.push(name.clone());
                }
            }
        }
    }
    order
}

fn field_values<'a>(nodes: &[&'a JsonNode], field: &str) -> Vec<&'a JsonNode> {
    nodes
        .iter()
        .filter_map(|node| match node {
            JsonNode::Object(fields) => fields.iter().find(|(k, _)| k == field).map(|(_, v)| v),
            _ => None,
        })
        .collect()
}

fn infer_object_shape(nodes: &[&JsonNode]) -> Shape {
    let fields = object_field_order(nodes)
        .into_iter()
        .map(|name| {
            let values = field_values(nodes, &name);
            let shape = infer_shape(&values);
            (name, shape)
        })
        .collect();
    Shape::Object(fields)
}

fn infer_array_shape(nodes: &[&JsonNode]) -> Shape {
    let elements: Vec<&JsonNode> = nodes
        .iter()
        .flat_map(|node| match node {
            JsonNode::Array(items) => items.iter(),
            _ => [].iter(),
        })
        .collect();
    if elements.is_empty() {
        return Shape::Array(Box::new(Shape::Scalar(BTreeSet::new())));
    }
    Shape::Array(Box::new(infer_shape(&elements)))
}

fn scalar_type_str(types: &BTreeSet<String>) -> String {
    if types.is_empty() {
        "empty".to_string()
    } else {
        types.iter().cloned().collect::<Vec<_>>().join("|")
    }
}

fn array_type_str(shape: &Shape) -> String {
    match shape {
        Shape::Scalar(types) | Shape::Mixed(types) => scalar_type_str(types),
        Shape::Object(_) => "object".to_string(),
        Shape::Array(inner) => format!("array<{}>", array_type_str(inner)),
    }
}

/// Peels through nested `Array` wrappers to find the element shape that
/// ultimately determines whether a sub-schema should be expanded.
fn innermost(shape: &Shape) -> &Shape {
    match shape {
        Shape::Array(inner) => innermost(inner),
        other => other,
    }
}

fn format_object_fields(fields: &[(String, Shape)], depth: usize, out: &mut String) {
    for (name, shape) in fields {
        format_field(name, shape, depth, out);
    }
}

fn format_field(name: &str, shape: &Shape, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    match shape {
        Shape::Object(fields) => {
            out.push_str(&format!("{indent}{name}: object\n"));
            format_object_fields(fields, depth + 1, out);
        }
        Shape::Array(inner) => {
            out.push_str(&format!(
                "{indent}{name}: array<{}>\n",
                array_type_str(inner)
            ));
            if let Shape::Object(fields) = innermost(inner) {
                format_object_fields(fields, depth + 1, out);
            }
        }
        Shape::Scalar(types) | Shape::Mixed(types) => {
            out.push_str(&format!("{indent}{name}: {}\n", scalar_type_str(types)));
        }
    }
}

/// Infers a lightweight shape summary of a JSON document and formats it as
/// an indented string (2 spaces per depth), similar in spirit to
/// `render::render_json`.
pub fn json_schema(node: &JsonNode) -> String {
    let shape = infer_shape(&[node]);
    let mut out = String::new();
    match &shape {
        Shape::Object(fields) => format_object_fields(fields, 0, &mut out),
        Shape::Array(inner) => {
            out.push_str(&format!("array<{}>\n", array_type_str(inner)));
            if let Shape::Object(fields) = innermost(inner) {
                format_object_fields(fields, 1, &mut out);
            }
        }
        Shape::Scalar(types) | Shape::Mixed(types) => {
            out.push_str(&scalar_type_str(types));
            out.push('\n');
        }
    }
    out
}

fn distinct_child_names(node: &XmlNode) -> Vec<String> {
    let mut names = Vec::new();
    for child in &node.children {
        if !names.contains(&child.name) {
            names.push(child.name.clone());
        }
    }
    names
}

fn format_xml_child(node: &XmlNode, name: &str, depth: usize, out: &mut String) {
    let siblings: Vec<&XmlNode> = node.children.iter().filter(|c| c.name == name).collect();
    let indent = "  ".repeat(depth);
    let mut line = format!("{indent}{name}");
    if let Some(first) = siblings.first()
        && !first.attributes.is_empty()
    {
        let attrs: Vec<&str> = first.attributes.iter().map(|(k, _)| k.as_str()).collect();
        line.push_str(&format!(" [{}]", attrs.join(", ")));
    }
    if siblings.len() > 1 {
        line.push_str(" (repeated)");
    }
    out.push_str(&line);
    out.push('\n');
    if let Some(first) = siblings.first() {
        format_xml_children(first, depth + 1, out);
    }
}

fn format_xml_children(node: &XmlNode, depth: usize, out: &mut String) {
    for name in distinct_child_names(node) {
        format_xml_child(node, &name, depth, out);
    }
}

/// Infers a lightweight shape summary of an XML document: for each node,
/// lists distinct child element names, their attribute names, and marks
/// `(repeated)` when a name appears more than once among its siblings.
pub fn xml_schema(node: &XmlNode) -> String {
    let mut out = String::new();
    format_xml_children(node, 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema_of(value: serde_json::Value) -> String {
        json_schema(&JsonNode::from_value(&value))
    }

    #[test]
    fn infers_object_with_scalar_and_array_fields() {
        let out = schema_of(json!({"user": {"name": "Alice", "tags": ["admin", "user"]}}));
        assert_eq!(out, "user: object\n  name: string\n  tags: array<string>\n");
    }

    #[test]
    fn dedupes_and_sorts_mixed_scalar_array_types() {
        let out = schema_of(json!({"vals": [1, "x", true, 2]}));
        assert_eq!(out, "vals: array<boolean|number|string>\n");
    }

    #[test]
    fn unions_fields_across_array_of_objects() {
        let out = schema_of(json!({"items": [{"a": 1}, {"b": "x"}]}));
        assert_eq!(out, "items: array<object>\n  a: number\n  b: string\n");
    }

    #[test]
    fn unions_types_for_a_field_that_varies_across_elements() {
        let out = schema_of(json!({"items": [{"a": 1}, {"a": "x"}]}));
        assert_eq!(out, "items: array<object>\n  a: number|string\n");
    }

    #[test]
    fn root_array_prints_type_with_no_leading_label() {
        let out = schema_of(json!(["a", "b", 1]));
        assert_eq!(out, "array<number|string>\n");
    }

    #[test]
    fn root_scalar_prints_just_the_type_name() {
        let out = schema_of(json!(42));
        assert_eq!(out, "number\n");
    }

    #[test]
    fn empty_array_has_no_element_type() {
        let out = schema_of(json!({"items": []}));
        assert_eq!(out, "items: array<empty>\n");
    }

    #[test]
    fn xml_schema_lists_children_attributes_and_repeats() {
        let xml = r#"<root><user id="1"><tag/><tag/></user><user id="2"/></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        let out = xml_schema(&node);
        assert_eq!(out, "user [id] (repeated)\n  tag (repeated)\n");
    }

    #[test]
    fn xml_schema_handles_no_children() {
        let xml = r#"<root/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc);
        assert_eq!(xml_schema(&node), "");
    }
}
