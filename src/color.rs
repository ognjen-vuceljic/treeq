#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Key,
    Str,
    Number,
    Bool,
    Null,
    Structural,
}

const RESET: &str = "\x1b[0m";

fn code(color: Color) -> &'static str {
    match color {
        Color::Key => "\x1b[36m",
        Color::Str => "\x1b[32m",
        Color::Number => "\x1b[33m",
        Color::Bool => "\x1b[35m",
        Color::Null => "\x1b[90m",
        Color::Structural => "\x1b[2m",
    }
}

pub fn paint(text: &str, color: Color, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    format!("{}{text}{RESET}", code(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_text_in_ansi_codes_when_enabled() {
        assert_eq!(paint("foo", Color::Key, true), "\x1b[36mfoo\x1b[0m");
    }

    #[test]
    fn returns_plain_text_when_disabled() {
        assert_eq!(paint("foo", Color::Key, false), "foo");
    }
}
