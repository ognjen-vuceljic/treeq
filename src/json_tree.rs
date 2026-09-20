use crate::color::Color;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonScalar {
    Str(String),
    Number(String),
    Bool(bool),
    Null,
}

impl JsonScalar {
    pub fn display(&self) -> String {
        match self {
            JsonScalar::Str(s) => s.clone(),
            JsonScalar::Number(s) => s.clone(),
            JsonScalar::Bool(b) => b.to_string(),
            JsonScalar::Null => "null".to_string(),
        }
    }

    pub fn color(&self) -> Color {
        match self {
            JsonScalar::Str(_) => Color::Str,
            JsonScalar::Number(_) => Color::Number,
            JsonScalar::Bool(_) => Color::Bool,
            JsonScalar::Null => Color::Null,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsonNode {
    Object(Vec<(String, JsonNode)>),
    Array(Vec<JsonNode>),
    Scalar(JsonScalar),
}

impl JsonNode {
    pub fn from_value(value: &serde_json::Value) -> JsonNode {
        match value {
            serde_json::Value::Object(map) => JsonNode::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), JsonNode::from_value(v)))
                    .collect(),
            ),
            serde_json::Value::Array(items) => {
                JsonNode::Array(items.iter().map(JsonNode::from_value).collect())
            }
            serde_json::Value::String(s) => JsonNode::Scalar(JsonScalar::Str(s.clone())),
            serde_json::Value::Null => JsonNode::Scalar(JsonScalar::Null),
            serde_json::Value::Bool(b) => JsonNode::Scalar(JsonScalar::Bool(*b)),
            serde_json::Value::Number(n) => JsonNode::Scalar(JsonScalar::Number(n.to_string())),
        }
    }

    pub fn from_yaml_value(value: &serde_yaml::Value) -> Result<JsonNode, String> {
        match value {
            serde_yaml::Value::Null => Ok(JsonNode::Scalar(JsonScalar::Null)),
            serde_yaml::Value::Bool(b) => Ok(JsonNode::Scalar(JsonScalar::Bool(*b))),
            serde_yaml::Value::Number(n) => Ok(JsonNode::Scalar(JsonScalar::Number(n.to_string()))),
            serde_yaml::Value::String(s) => Ok(JsonNode::Scalar(JsonScalar::Str(s.clone()))),
            serde_yaml::Value::Sequence(items) => {
                let converted = items
                    .iter()
                    .map(JsonNode::from_yaml_value)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(JsonNode::Array(converted))
            }
            serde_yaml::Value::Mapping(map) => JsonNode::from_yaml_mapping(map),
            serde_yaml::Value::Tagged(_) => Err("YAML tags are not supported".to_string()),
        }
    }

    /// Note: YAML's `<<: *anchor` merge-key idiom is not merged — a `<<` key
    /// is kept as a literal object field pointing at the aliased mapping,
    /// same as any other key. Implementing real merge semantics (including
    /// `<<: [*a, *b]` and merge-vs-explicit-key precedence) is out of scope
    /// for this viewer; treeq shows the document's raw structure.
    fn from_yaml_mapping(map: &serde_yaml::Mapping) -> Result<JsonNode, String> {
        let mut fields = Vec::with_capacity(map.len());
        for (k, v) in map {
            let key = k
                .as_str()
                .ok_or_else(|| "YAML mapping keys must be strings".to_string())?;
            fields.push((key.to_string(), JsonNode::from_yaml_value(v)?));
        }
        Ok(JsonNode::Object(fields))
    }
}

pub fn find_json_path<'a>(node: &'a JsonNode, path: &str) -> Result<&'a JsonNode, String> {
    let mut current = node;
    for segment in path.split('.') {
        current = match current {
            JsonNode::Object(fields) => fields
                .iter()
                .find(|(k, _)| k == segment)
                .map(|(_, v)| v)
                .ok_or_else(|| segment.to_string())?,
            JsonNode::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|i| items.get(i))
                .ok_or_else(|| segment.to_string())?,
            JsonNode::Scalar(_) => return Err(segment.to_string()),
        };
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_nested_object_and_array() {
        let value = json!({"name": "Alice", "tags": ["admin", "user"]});
        let node = JsonNode::from_value(&value);
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "name".to_string(),
                    JsonNode::Scalar(JsonScalar::Str("Alice".to_string()))
                ),
                (
                    "tags".to_string(),
                    JsonNode::Array(vec![
                        JsonNode::Scalar(JsonScalar::Str("admin".to_string())),
                        JsonNode::Scalar(JsonScalar::Str("user".to_string())),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn converts_number_bool_and_null_scalars() {
        let value = json!({"age": 30, "active": true, "middle_name": null});
        let node = JsonNode::from_value(&value);
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "age".to_string(),
                    JsonNode::Scalar(JsonScalar::Number("30".to_string()))
                ),
                (
                    "active".to_string(),
                    JsonNode::Scalar(JsonScalar::Bool(true))
                ),
                (
                    "middle_name".to_string(),
                    JsonNode::Scalar(JsonScalar::Null)
                ),
            ])
        );
    }

    #[test]
    fn finds_nested_path() {
        let value = json!({"user": {"tags": ["admin", "user"]}});
        let node = JsonNode::from_value(&value);
        let found = find_json_path(&node, "user.tags.1").unwrap();
        assert_eq!(
            found,
            &JsonNode::Scalar(JsonScalar::Str("user".to_string()))
        );
    }

    #[test]
    fn reports_first_unmatched_segment() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let err = find_json_path(&node, "user.missing.deeper").unwrap_err();
        assert_eq!(err, "missing");
    }

    #[test]
    fn converts_yaml_mapping_and_sequence() {
        let value: serde_yaml::Value =
            serde_yaml::from_str("name: Alice\ntags:\n  - admin\n  - user\n").unwrap();
        let node = JsonNode::from_yaml_value(&value).unwrap();
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "name".to_string(),
                    JsonNode::Scalar(JsonScalar::Str("Alice".to_string()))
                ),
                (
                    "tags".to_string(),
                    JsonNode::Array(vec![
                        JsonNode::Scalar(JsonScalar::Str("admin".to_string())),
                        JsonNode::Scalar(JsonScalar::Str("user".to_string())),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn converts_yaml_scalars() {
        let value: serde_yaml::Value =
            serde_yaml::from_str("age: 30\nactive: true\nmiddle_name: null\n").unwrap();
        let node = JsonNode::from_yaml_value(&value).unwrap();
        assert_eq!(
            node,
            JsonNode::Object(vec![
                (
                    "age".to_string(),
                    JsonNode::Scalar(JsonScalar::Number("30".to_string()))
                ),
                (
                    "active".to_string(),
                    JsonNode::Scalar(JsonScalar::Bool(true))
                ),
                (
                    "middle_name".to_string(),
                    JsonNode::Scalar(JsonScalar::Null)
                ),
            ])
        );
    }

    #[test]
    fn errors_on_non_string_mapping_key() {
        let value: serde_yaml::Value = serde_yaml::from_str("1: one\n2: two\n").unwrap();
        let err = JsonNode::from_yaml_value(&value).unwrap_err();
        assert_eq!(err, "YAML mapping keys must be strings");
    }

    #[test]
    fn errors_on_yaml_tagged_value() {
        let value: serde_yaml::Value = serde_yaml::from_str("!Tag value").unwrap();
        let err = JsonNode::from_yaml_value(&value).unwrap_err();
        assert_eq!(err, "YAML tags are not supported");
    }
}
