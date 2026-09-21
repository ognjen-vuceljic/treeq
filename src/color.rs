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

/// Dim, desaturated tones cycled by nesting depth for tree guide lines (the
/// `├──`/`│`/`└──` prefix) -- a structural cue for tracking "which level am
/// I at", not a semantic color, so it's kept separate from `Color`'s fixed
/// palette rather than added as another `Color` variant.
const DEPTH_TINTS: [&str; 6] = [
    "\x1b[2m",    // depth 0: today's plain dim gray, unchanged
    "\x1b[2;34m", // dim blue
    "\x1b[2;36m", // dim cyan
    "\x1b[2;32m", // dim green
    "\x1b[2;35m", // dim magenta
    "\x1b[2;33m", // dim yellow
];

/// Same shape as `paint`, but keyed on `depth % N` instead of a fixed
/// `Color`. Only ever applied to guide characters, never key/value text.
pub fn paint_depth(text: &str, depth: usize, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    let tint = DEPTH_TINTS[depth % DEPTH_TINTS.len()];
    format!("{tint}{text}{RESET}")
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

    #[test]
    fn depth_tints_cycle_through_distinct_codes() {
        let tints: std::collections::HashSet<String> = (0..DEPTH_TINTS.len())
            .map(|d| paint_depth("x", d, true))
            .collect();
        assert_eq!(
            tints.len(),
            DEPTH_TINTS.len(),
            "every depth in one full cycle must get a distinct tint"
        );
    }

    #[test]
    fn depth_tint_wraps_around_past_the_palette_length() {
        assert_eq!(
            paint_depth("x", 0, true),
            paint_depth("x", DEPTH_TINTS.len(), true)
        );
    }

    #[test]
    fn depth_zero_matches_the_original_plain_structural_dim() {
        assert_eq!(paint_depth("x", 0, true), "\x1b[2mx\x1b[0m");
    }

    #[test]
    fn depth_tint_returns_plain_text_when_disabled() {
        assert_eq!(paint_depth("x", 3, false), "x");
    }
}
