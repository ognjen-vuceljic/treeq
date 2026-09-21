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
    /// Every non-null node in this slot shared one shape, but at least one
    /// node was `null` (e.g. a nullable nested object/array field).
    Nullable(Box<Shape>),
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

fn is_null(node: &JsonNode) -> bool {
    matches!(node, JsonNode::Scalar(JsonScalar::Null))
}

/// Infers the shape shared by `nodes`, treating `null` as a modifier rather
/// than a distinct kind: a mix of objects (or arrays) and nulls still infers
/// the objects'/arrays' shape, wrapped as `Nullable`, instead of falling
/// back to a structure-losing `Mixed` label union.
fn infer_shape(nodes: &[&JsonNode]) -> Shape {
    let non_null: Vec<&JsonNode> = nodes.iter().copied().filter(|n| !is_null(n)).collect();
    let has_null = non_null.len() != nodes.len();

    if non_null.is_empty() {
        let mut types = BTreeSet::new();
        if has_null {
            types.insert("null".to_string());
        }
        return Shape::Scalar(types);
    }

    let shape = infer_non_null_shape(&non_null, has_null);
    match shape {
        Shape::Object(_) | Shape::Array(_) if has_null => Shape::Nullable(Box::new(shape)),
        Shape::Mixed(mut types) if has_null => {
            types.insert("null".to_string());
            Shape::Mixed(types)
        }
        other => other,
    }
}

fn infer_non_null_shape(non_null: &[&JsonNode], has_null: bool) -> Shape {
    if non_null.iter().all(|n| matches!(n, JsonNode::Object(_))) {
        return infer_object_shape(non_null);
    }
    if non_null.iter().all(|n| matches!(n, JsonNode::Array(_))) {
        return infer_array_shape(non_null);
    }
    if non_null.iter().all(|n| matches!(n, JsonNode::Scalar(_))) {
        let mut types: BTreeSet<String> = non_null
            .iter()
            .map(|n| match n {
                JsonNode::Scalar(s) => scalar_type_name(s).to_string(),
                _ => unreachable!(),
            })
            .collect();
        if has_null {
            types.insert("null".to_string());
        }
        return Shape::Scalar(types);
    }
    Shape::Mixed(non_null.iter().map(|n| kind_label(n)).collect())
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
        Shape::Nullable(inner) => format!("{}|null", array_type_str(inner)),
    }
}

/// Peels through nested `Array` and `Nullable` wrappers to find the element
/// shape that ultimately determines whether a sub-schema should be expanded.
fn innermost(shape: &Shape) -> &Shape {
    match shape {
        Shape::Array(inner) | Shape::Nullable(inner) => innermost(inner),
        other => other,
    }
}

fn format_object_fields(fields: &[(String, Shape)], depth: usize, out: &mut String) {
    for (name, shape) in fields {
        format_field(name, shape, depth, out);
    }
}

/// Returns this shape's one-line type label and, if it's (or wraps) an
/// object, the fields that should be expanded beneath it.
fn shape_label_and_fields(shape: &Shape) -> (String, Option<&[(String, Shape)]>) {
    match shape {
        Shape::Object(fields) => ("object".to_string(), Some(fields.as_slice())),
        Shape::Array(inner) => {
            let label = format!("array<{}>", array_type_str(inner));
            let fields = match innermost(inner) {
                Shape::Object(fields) => Some(fields.as_slice()),
                _ => None,
            };
            (label, fields)
        }
        Shape::Scalar(types) | Shape::Mixed(types) => (scalar_type_str(types), None),
        Shape::Nullable(inner) => {
            let (label, fields) = shape_label_and_fields(inner);
            (format!("{label}|null"), fields)
        }
    }
}

fn format_field(name: &str, shape: &Shape, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    let (label, fields) = shape_label_and_fields(shape);
    out.push_str(&format!("{indent}{name}: {label}\n"));
    if let Some(fields) = fields {
        format_object_fields(fields, depth + 1, out);
    }
}

/// Infers a lightweight shape summary of a JSON document and formats it as
/// an indented string (2 spaces per depth), similar in spirit to
/// `render::render_json`.
pub fn json_schema(node: &JsonNode) -> String {
    // A single root node is never null-mixed with anything else, so
    // infer_shape(&[node]) never produces Shape::Nullable here.
    let shape = infer_shape(&[node]);
    let mut out = String::new();
    match &shape {
        Shape::Object(fields) if fields.is_empty() => out.push_str("object<empty>\n"),
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
        Shape::Nullable(_) => {
            let (label, fields) = shape_label_and_fields(&shape);
            out.push_str(&label);
            out.push('\n');
            if let Some(fields) = fields {
                format_object_fields(fields, 1, &mut out);
            }
        }
    }
    out
}

/// Distinct child element names across every node in `parents`, in first-
/// appearance order, so a repeated element's *union* of shapes across all
/// its instances is what gets inspected (see `format_xml_child`).
fn distinct_child_names(parents: &[&XmlNode]) -> Vec<String> {
    let mut names = Vec::new();
    for parent in parents {
        for child in &parent.children {
            if !names.contains(&child.name) {
                names.push(child.name.clone());
            }
        }
    }
    names
}

fn union_attribute_names(siblings: &[&XmlNode]) -> Vec<String> {
    let mut names = Vec::new();
    for sibling in siblings {
        for (key, _) in &sibling.attributes {
            if !names.contains(key) {
                names.push(key.clone());
            }
        }
    }
    names
}

fn format_xml_child(parents: &[&XmlNode], name: &str, depth: usize, out: &mut String) {
    let siblings: Vec<&XmlNode> = parents
        .iter()
        .flat_map(|p| p.children.iter())
        .filter(|c| c.name == name)
        .collect();
    let indent = "  ".repeat(depth);
    let mut line = format!("{indent}{name}");
    let attrs = union_attribute_names(&siblings);
    if !attrs.is_empty() {
        line.push_str(&format!(" [{}]", attrs.join(", ")));
    }
    if siblings.len() > 1 {
        line.push_str(" (repeated)");
    }
    out.push_str(&line);
    out.push('\n');
    format_xml_children(&siblings, depth + 1, out);
}

fn format_xml_children(parents: &[&XmlNode], depth: usize, out: &mut String) {
    for name in distinct_child_names(parents) {
        format_xml_child(parents, &name, depth, out);
    }
}

/// Infers a lightweight shape summary of an XML document: for each element
/// name, lists the *union* of attribute names and child element names seen
/// across every sibling with that name, and marks `(repeated)` when a name
/// appears more than once.
pub fn xml_schema(node: &XmlNode) -> String {
    let mut out = String::new();
    format_xml_children(&[node], 0, &mut out);
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
    fn empty_root_object_is_shown_explicitly() {
        let out = schema_of(json!({}));
        assert_eq!(out, "object<empty>\n");
    }

    #[test]
    fn a_null_element_among_objects_does_not_discard_field_info() {
        let out = schema_of(json!({"items": [{"a": 1, "b": 2}, {"a": 3, "b": 4}, null]}));
        assert_eq!(out, "items: array<object|null>\n  a: number\n  b: number\n");
    }

    #[test]
    fn a_nullable_nested_object_field_still_shows_its_fields() {
        let out = schema_of(json!({"items": [{"address": {"city": "x"}}, {"address": null}]}));
        assert_eq!(
            out,
            "items: array<object>\n  address: object|null\n    city: string\n"
        );
    }

    #[test]
    fn a_nullable_array_field_still_shows_its_element_type() {
        let out = schema_of(json!({"items": [{"tags": ["x"]}, {"tags": null}]}));
        assert_eq!(out, "items: array<object>\n  tags: array<string>|null\n");
    }

    #[test]
    fn a_genuinely_mixed_kind_field_still_falls_back_to_a_label_union_including_null() {
        let out = schema_of(json!({"items": [{"a": 1}, "x", null]}));
        assert_eq!(out, "items: array<null|object|string>\n");
    }

    #[test]
    fn xml_schema_lists_children_attributes_and_repeats() {
        let xml = r#"<root><user id="1"><tag/><tag/></user><user id="2"/></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let out = xml_schema(&node);
        assert_eq!(out, "user [id] (repeated)\n  tag (repeated)\n");
    }

    #[test]
    fn xml_schema_unions_attributes_and_children_across_all_siblings_not_just_the_first() {
        let xml = r#"<root>
            <user><tag/></user>
            <user><tag/><extra/></user>
        </root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let out = xml_schema(&node);
        assert_eq!(out, "user (repeated)\n  tag (repeated)\n  extra\n");
    }

    #[test]
    fn xml_schema_unions_attribute_names_across_siblings() {
        let xml = r#"<root><user id="1"/><user id="2" class="admin"/></root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let out = xml_schema(&node);
        assert_eq!(out, "user [id, class] (repeated)\n");
    }

    #[test]
    fn xml_schema_handles_no_children() {
        let xml = r#"<root/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        assert_eq!(xml_schema(&node), "");
    }
}
