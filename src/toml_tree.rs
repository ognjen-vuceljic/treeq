use crate::error_context::toml_error_context;
use crate::json_tree::{JsonNode, JsonScalar};

pub fn parse_toml_tree(input: &str) -> Result<JsonNode, String> {
    toml::from_str::<toml::Value>(input)
        .map(|v| to_node(&v))
        .map_err(|e| toml_error_context(input, &e))
}

/// True when the input is a non-empty TOML table: the sniff used to pick TOML
/// for extension-less input that is plainly not JSON.
pub fn looks_like_toml(input: &str) -> bool {
    matches!(
        toml::from_str::<toml::Table>(input),
        Ok(table) if !table.is_empty()
    )
}

/// Datetimes have no JSON type, so they show as their TOML text (the colour
/// layer already highlights ISO dates inside strings).
fn to_node(value: &toml::Value) -> JsonNode {
    use toml::Value as T;
    let scalar = JsonNode::Scalar;
    match value {
        T::String(s) => scalar(JsonScalar::Str(s.clone())),
        T::Integer(i) => scalar(JsonScalar::Number(i.to_string())),
        T::Float(f) => scalar(JsonScalar::Number(f.to_string())),
        T::Boolean(b) => scalar(JsonScalar::Bool(*b)),
        T::Datetime(d) => scalar(JsonScalar::Str(d.to_string())),
        T::Array(items) => JsonNode::Array(items.iter().map(to_node).collect()),
        T::Table(t) => JsonNode::Object(t.iter().map(|(k, v)| (k.clone(), to_node(v))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json_tree::to_compact_json;

    fn json(src: &str) -> String {
        to_compact_json(&parse_toml_tree(src).unwrap())
    }

    #[test]
    fn converts_scalars_tables_and_arrays_keeping_key_order() {
        assert_eq!(
            json("z = 1\na = true\ns = \"x\"\n[t]\nk = [1, 2]\n"),
            r#"{"z":1,"a":true,"s":"x","t":{"k":[1,2]}}"#
        );
    }

    #[test]
    fn datetimes_show_as_their_toml_text() {
        assert_eq!(
            json("a = 1979-05-27T07:32:00Z\nb = 1979-05-27\nc = 07:32:00\n"),
            r#"{"a":"1979-05-27T07:32:00Z","b":"1979-05-27","c":"07:32:00"}"#
        );
    }

    #[test]
    fn arrays_of_tables_and_inline_tables() {
        assert_eq!(
            json("[[p]]\nn = 1\n[[p]]\nn = 2\npt = { x = 1 }\n"),
            r#"{"p":[{"n":1},{"n":2,"pt":{"x":1}}]}"#
        );
    }

    #[test]
    fn integers_keep_full_precision_and_floats_special_values_survive() {
        assert_eq!(
            json("i = 9223372036854775807\nf = 1.5\nn = inf\n"),
            r#"{"i":9223372036854775807,"f":1.5,"n":inf}"#
        );
    }

    #[test]
    fn dotted_keys_build_nested_tables() {
        assert_eq!(json("a.b.c = 1\n"), r#"{"a":{"b":{"c":1}}}"#);
    }

    #[test]
    fn error_names_line_and_column_with_a_caret() {
        let err = parse_toml_tree("a = 1\nb = \n").unwrap_err();
        assert!(
            err.starts_with("error: invalid TOML at line 2"),
            "got: {err}"
        );
        assert!(err.contains('^'), "got: {err}");
    }

    #[test]
    fn error_column_counts_bytes_after_multibyte_text() {
        let err = parse_toml_tree("\"é\" = 1\nb = @\n").unwrap_err();
        assert!(err.contains("line 2, column 5"), "got: {err}");
    }

    #[test]
    fn duplicate_keys_are_an_error() {
        assert!(parse_toml_tree("a = 1\na = 2\n").is_err());
    }

    #[test]
    fn sniff_accepts_only_non_empty_tables() {
        assert!(looks_like_toml("a = 1"));
        assert!(!looks_like_toml(""));
        assert!(!looks_like_toml("# just a comment\n"));
        assert!(!looks_like_toml("hello"));
        assert!(!looks_like_toml("a: 1"));
    }

    #[test]
    fn large_document_converts() {
        let src: String = (0..20_000).map(|i| format!("k{i} = {i}\n")).collect();
        let JsonNode::Object(fields) = parse_toml_tree(&src).unwrap() else {
            panic!("not an object")
        };
        assert_eq!(fields.len(), 20_000);
    }
}
