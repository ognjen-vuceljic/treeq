use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Xml,
    Yaml,
    Ndjson,
}

/// File extension wins; otherwise sniff the content. Only ever picks YAML or
/// NDJSON when the input is plainly not JSON, so JSON error messages for
/// broken JSON are unchanged.
pub fn detect_format(input: &str, file: Option<&Path>) -> Option<Format> {
    let trimmed = input.trim_start();
    let first = trimmed.chars().next()?;
    let ext = file
        .and_then(|p| p.extension())
        .map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        Some("yaml" | "yml") => return Some(Format::Yaml),
        Some("ndjson" | "jsonl") => return Some(Format::Ndjson),
        _ => {}
    }
    if first == '<' {
        return Some(Format::Xml);
    }
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Some(Format::Json);
    }
    if matches!(first, '{' | '[') && looks_like_ndjson(trimmed) {
        return Some(Format::Ndjson);
    }
    if !matches!(first, '{' | '[' | '"') && looks_like_yaml(trimmed) {
        return Some(Format::Yaml);
    }
    Some(Format::Json)
}

fn looks_like_ndjson(s: &str) -> bool {
    let mut lines = s.lines().filter(|l| !l.trim().is_empty());
    let first_ok = lines
        .next()
        .is_some_and(|l| serde_json::from_str::<serde_json::Value>(l).is_ok());
    first_ok && lines.next().is_some()
}

fn looks_like_yaml(s: &str) -> bool {
    matches!(
        serde_yaml::from_str::<serde_yaml::Value>(s),
        Ok(serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_))
    ) || s.starts_with("---")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_xml_from_leading_angle_bracket() {
        assert_eq!(detect_format("  <root/>", None), Some(Format::Xml));
    }

    #[test]
    fn detects_json_otherwise() {
        assert_eq!(detect_format(r#"{"a": 1}"#, None), Some(Format::Json));
    }

    #[test]
    fn returns_none_for_empty_input() {
        assert_eq!(detect_format("   ", None), None);
    }

    #[test]
    fn sniffs_yaml_and_ndjson_and_honours_extensions() {
        assert_eq!(detect_format("a: 1\nb: [2]\n", None), Some(Format::Yaml));
        assert_eq!(detect_format("- 1\n- 2\n", None), Some(Format::Yaml));
        assert_eq!(
            detect_format("{\"a\":1}\n{\"a\":2}\n", None),
            Some(Format::Ndjson)
        );
        assert_eq!(detect_format("{\"a\":", None), Some(Format::Json));
        assert_eq!(
            detect_format("{}", Some(Path::new("x.yml"))),
            Some(Format::Yaml)
        );
        assert_eq!(
            detect_format("{}", Some(Path::new("x.jsonl"))),
            Some(Format::Ndjson)
        );
    }
}
