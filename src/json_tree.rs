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
}
