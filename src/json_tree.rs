#[derive(Debug, Clone, PartialEq)]
pub enum JsonNode {
    Object(Vec<(String, JsonNode)>),
    Array(Vec<JsonNode>),
    Scalar(String),
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
            serde_json::Value::String(s) => JsonNode::Scalar(s.clone()),
            serde_json::Value::Null => JsonNode::Scalar("null".to_string()),
            other => JsonNode::Scalar(other.to_string()),
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
                ("name".to_string(), JsonNode::Scalar("Alice".to_string())),
                (
                    "tags".to_string(),
                    JsonNode::Array(vec![
                        JsonNode::Scalar("admin".to_string()),
                        JsonNode::Scalar("user".to_string()),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn converts_number_and_null_scalars() {
        let value = json!({"age": 30, "middle_name": null});
        let node = JsonNode::from_value(&value);
        assert_eq!(
            node,
            JsonNode::Object(vec![
                ("age".to_string(), JsonNode::Scalar("30".to_string())),
                (
                    "middle_name".to_string(),
                    JsonNode::Scalar("null".to_string())
                ),
            ])
        );
    }

    #[test]
    fn finds_nested_path() {
        let value = json!({"user": {"tags": ["admin", "user"]}});
        let node = JsonNode::from_value(&value);
        let found = find_json_path(&node, "user.tags.1").unwrap();
        assert_eq!(found, &JsonNode::Scalar("user".to_string()));
    }

    #[test]
    fn reports_first_unmatched_segment() {
        let value = json!({"user": {"name": "Alice"}});
        let node = JsonNode::from_value(&value);
        let err = find_json_path(&node, "user.missing.deeper").unwrap_err();
        assert_eq!(err, "missing");
    }
}
