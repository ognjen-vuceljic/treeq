#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Xml,
    Yaml,
}

pub fn detect_format(input: &str) -> Option<Format> {
    let trimmed = input.trim_start();
    match trimmed.chars().next() {
        Some('<') => Some(Format::Xml),
        Some(_) => Some(Format::Json),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_xml_from_leading_angle_bracket() {
        assert_eq!(detect_format("  <root/>"), Some(Format::Xml));
    }

    #[test]
    fn detects_json_otherwise() {
        assert_eq!(detect_format(r#"{"a": 1}"#), Some(Format::Json));
    }

    #[test]
    fn returns_none_for_empty_input() {
        assert_eq!(detect_format("   "), None);
    }
}
