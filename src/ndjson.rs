//! NDJSON / JSON Lines parsing.
//!
//! Line numbers reported in errors are the physical 1-indexed line number in
//! the original input, so they match what a user sees in their editor. Blank
//! lines (empty after trimming) are skipped when parsing but still count
//! toward the line number.

/// Parses NDJSON input: one JSON value per non-empty line. Blank lines
/// (empty after trimming) are skipped. All parsed values are collected into
/// a single `serde_json::Value::Array`.
///
/// On a parse failure, returns `Err` with the 1-indexed physical line number
/// and the underlying parse error.
pub fn parse_ndjson(input: &str) -> Result<serde_json::Value, String> {
    let mut values = Vec::new();
    for (idx, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let line_no = idx + 1;
        let value: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("line {line_no}: {e}"))?;
        values.push(value);
    }
    Ok(serde_json::Value::Array(values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_multiline_input() {
        let input = "{\"a\": 1}\n{\"a\": 2}\n{\"a\": 3}";
        let result = parse_ndjson(input).unwrap();
        assert_eq!(result, json!([{"a": 1}, {"a": 2}, {"a": 3}]));
    }

    #[test]
    fn reports_line_number_on_failure() {
        let input = "{\"a\": 1}\nnot json\n{\"a\": 3}";
        let err = parse_ndjson(input).unwrap_err();
        assert!(err.starts_with("line 2:"), "unexpected error: {err}");
    }

    #[test]
    fn reports_physical_line_number_across_blank_lines() {
        let input = "\n{\"a\": 1}\nnot json\n";
        let err = parse_ndjson(input).unwrap_err();
        assert!(err.starts_with("line 3:"), "unexpected error: {err}");
    }

    #[test]
    fn skips_blank_lines() {
        let input = "{\"a\": 1}\n\n\n{\"a\": 2}\n";
        let result = parse_ndjson(input).unwrap();
        assert_eq!(result, json!([{"a": 1}, {"a": 2}]));
    }

    #[test]
    fn empty_input_yields_empty_array() {
        let result = parse_ndjson("").unwrap();
        assert_eq!(result, json!([]));
    }
}
