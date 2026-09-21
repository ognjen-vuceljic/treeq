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

fn code_ansi16(color: Color) -> &'static str {
    match color {
        Color::Key => "\x1b[36m",
        Color::Str => "\x1b[32m",
        Color::Number => "\x1b[33m",
        Color::Bool => "\x1b[35m",
        Color::Null => "\x1b[90m",
        Color::Structural => "\x1b[2m",
    }
}

/// Distinct 24-bit tones per semantic bucket, used when the terminal
/// advertises truecolor support. `Structural` stays a plain dim modifier in
/// both tiers -- it's de-emphasized chrome (ellipses, "N more"), not a
/// value that benefits from its own hue (depth-tinted guide lines already
/// get their own palette, see `paint_depth`).
fn code_truecolor(color: Color) -> &'static str {
    match color {
        Color::Key => "\x1b[38;2;86;182;194m",
        Color::Str => "\x1b[38;2;152;195;121m",
        Color::Number => "\x1b[38;2;229;192;123m",
        Color::Bool => "\x1b[38;2;198;120;221m",
        Color::Null => "\x1b[38;2;128;128;128m",
        Color::Structural => "\x1b[2m",
    }
}

/// `COLORTERM=truecolor|24bit` is the de facto standard a terminal uses to
/// advertise 24-bit color support (checked by e.g. neovim, tmux). Anything
/// else falls back to the original 16-color codes -- this only chooses a
/// tier *within* "color enabled"; `NO_COLOR`/non-TTY detection already
/// happens upstream in `main.rs`'s `use_color()` and is unaffected.
fn supports_truecolor() -> bool {
    matches!(
        std::env::var("COLORTERM").as_deref(),
        Ok("truecolor") | Ok("24bit")
    )
}

/// The color tier a renderer should use for a given run. Kept as an
/// explicit, caller-provided value (computed once, from `is_terminal()`,
/// `NO_COLOR`, and `COLORTERM`) rather than read ambiently inside `paint()`
/// itself -- an ambient env read here would make color output (and any
/// test asserting exact escape codes) depend on whatever `COLORTERM`
/// happens to be set to in the calling environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Off,
    Ansi16,
    Truecolor,
}

impl ColorMode {
    pub fn enabled(self) -> bool {
        self != ColorMode::Off
    }

    /// `enabled` is the caller's own on/off decision (TTY + `NO_COLOR`);
    /// this only picks a *tier* within "on" by checking `COLORTERM`.
    pub fn detect(enabled: bool) -> ColorMode {
        if !enabled {
            ColorMode::Off
        } else if supports_truecolor() {
            ColorMode::Truecolor
        } else {
            ColorMode::Ansi16
        }
    }
}

fn code_for(color: Color, mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Off => "",
        ColorMode::Ansi16 => code_ansi16(color),
        ColorMode::Truecolor => code_truecolor(color),
    }
}

pub fn paint(text: &str, color: Color, mode: ColorMode) -> String {
    if !mode.enabled() {
        return text.to_string();
    }
    format!("{}{text}{RESET}", code_for(color, mode))
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
        assert_eq!(
            paint("foo", Color::Key, ColorMode::Ansi16),
            "\x1b[36mfoo\x1b[0m"
        );
    }

    #[test]
    fn returns_plain_text_when_disabled() {
        assert_eq!(paint("foo", Color::Key, ColorMode::Off), "foo");
    }

    #[test]
    fn wraps_text_in_truecolor_codes_in_the_truecolor_tier() {
        assert_eq!(
            paint("foo", Color::Key, ColorMode::Truecolor),
            "\x1b[38;2;86;182;194mfoo\x1b[0m"
        );
    }

    #[test]
    fn ansi16_tier_uses_the_original_16_color_codes() {
        assert_eq!(code_for(Color::Key, ColorMode::Ansi16), "\x1b[36m");
        assert_eq!(code_for(Color::Str, ColorMode::Ansi16), "\x1b[32m");
        assert_eq!(code_for(Color::Number, ColorMode::Ansi16), "\x1b[33m");
        assert_eq!(code_for(Color::Bool, ColorMode::Ansi16), "\x1b[35m");
        assert_eq!(code_for(Color::Null, ColorMode::Ansi16), "\x1b[90m");
        assert_eq!(code_for(Color::Structural, ColorMode::Ansi16), "\x1b[2m");
    }

    #[test]
    fn truecolor_tier_gives_every_semantic_bucket_a_distinct_24_bit_code() {
        let codes: std::collections::HashSet<&str> = [
            Color::Key,
            Color::Str,
            Color::Number,
            Color::Bool,
            Color::Null,
        ]
        .map(|c| code_for(c, ColorMode::Truecolor))
        .into_iter()
        .collect();
        assert_eq!(
            codes.len(),
            5,
            "every non-structural bucket must be distinct"
        );
        for c in codes {
            assert!(
                c.starts_with("\x1b[38;2;"),
                "expected a 24-bit truecolor escape, got {c:?}"
            );
        }
    }

    #[test]
    fn structural_stays_a_plain_dim_modifier_in_both_tiers() {
        assert_eq!(code_for(Color::Structural, ColorMode::Ansi16), "\x1b[2m");
        assert_eq!(code_for(Color::Structural, ColorMode::Truecolor), "\x1b[2m");
    }

    #[test]
    fn detect_is_off_when_the_caller_says_color_is_disabled() {
        assert_eq!(ColorMode::detect(false), ColorMode::Off);
    }

    #[test]
    fn detect_picks_a_real_tier_when_the_caller_says_color_is_enabled() {
        // Whichever tier `COLORTERM` in this actual process picks, `detect`
        // must not report `Off` when the caller says color is on.
        assert_ne!(ColorMode::detect(true), ColorMode::Off);
    }

    #[test]
    fn color_mode_enabled_matches_whether_it_is_off() {
        assert!(!ColorMode::Off.enabled());
        assert!(ColorMode::Ansi16.enabled());
        assert!(ColorMode::Truecolor.enabled());
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
