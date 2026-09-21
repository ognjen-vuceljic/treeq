use super::flatten::{flatten_json, flatten_xml};
use crate::color::Color as TqColor;
use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use ratatui::style::Color;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

/// Dim/dark tints rather than raw ANSI hues: a full-saturation background
/// washes out the foreground text layered on top of it. Kept at roughly the
/// same luminance so no tag stands out as brighter or dimmer than another.
const TAG_PALETTE: [Color; 8] = [
    Color::Rgb(90, 30, 30), // red
    Color::Rgb(30, 45, 90), // blue
    Color::Rgb(85, 30, 85), // magenta
    Color::Rgb(30, 80, 45), // green
    Color::Rgb(90, 80, 25), // yellow
    Color::Rgb(25, 80, 85), // cyan
    Color::Rgb(85, 85, 85), // white/light gray
    Color::Rgb(60, 60, 60), // gray
];

/// `tag` is 1-based; out-of-range values wrap rather than panic.
pub(super) fn tag_color(tag: u8) -> Color {
    let idx = (tag.saturating_sub(1) as usize) % TAG_PALETTE.len();
    TAG_PALETTE[idx]
}

/// Cycled by nesting depth for a line's indent+marker span, so the eye can
/// track "which level am I at" the way editor indent-guides do -- the same
/// structural cue as `render.rs`'s `paint_depth`, just as ratatui `Color`s
/// instead of raw ANSI codes. Depth 0 matches `ratatui_color(Structural)`'s
/// existing `DarkGray` exactly, so a flat/shallow document looks unchanged.
const DEPTH_TINT_PALETTE: [Color; 6] = [
    Color::DarkGray,
    Color::Rgb(70, 90, 130),  // dim blue
    Color::Rgb(60, 110, 110), // dim cyan
    Color::Rgb(70, 105, 70),  // dim green
    Color::Rgb(110, 70, 110), // dim magenta
    Color::Rgb(115, 100, 60), // dim yellow
];

pub(super) fn depth_tint_color(depth: usize) -> Color {
    DEPTH_TINT_PALETTE[depth % DEPTH_TINT_PALETTE.len()]
}

pub(super) struct Line {
    pub(super) depth: usize,
    pub(super) key: String,
    pub(super) value: Option<(String, TqColor)>,
    pub(super) path: Vec<String>,
    pub(super) has_children: bool,
    /// True only for the synthetic "N more (Tab to show all)" line, so a
    /// real key spelled the same way is never mistaken for it.
    pub(super) is_array_summary: bool,
    /// Human-readable type/size, e.g. "object (3 fields)", "string (5 chars)".
    pub(super) type_label: String,
}

pub(super) struct AppState {
    pub(super) lines: Vec<Line>,
    pub(super) collapsed: HashSet<Vec<String>>,
    pub(super) all_container_paths: HashSet<Vec<String>>,
    /// Arrays expanded past the default preview limit. Collapsing an
    /// array's own line clears its entry, so re-expanding it starts
    /// truncated again.
    pub(super) array_overrides: HashSet<Vec<String>>,
    /// Tagged nodes, keyed by path; in-memory only.
    pub(super) tags: HashMap<Vec<String>, u8>,
    pub(super) cursor: usize,
    pub(super) search: String,
    pub(super) searching: bool,
    pub(super) use_color: bool,
    pub(super) status_message: Option<String>,
    pub(super) help_visible: bool,
    /// False for XML. Gates the `Y` (yank as jq path) binding.
    pub(super) is_json: bool,
    /// `Cell` so `render()` can update it without needing `&mut AppState`.
    pub(super) scroll_offset: Cell<usize>,
    /// Every node's path and search text (dotted path plus `: value` for a
    /// leaf), independent of collapse state, so search can reach collapsed
    /// subtrees and match key/value combos.
    pub(super) all_paths: Vec<(Vec<String>, String)>,
    /// A search match found outside the visible lines: consumed by
    /// `rebuild_json_lines`/`rebuild_xml_lines` once `lines` is rebuilt.
    pub(super) pending_cursor_path: Option<Vec<String>>,
    /// Which occurrence (0-based) of `pending_cursor_path` to land on, for
    /// the rare case multiple lines share the exact same path -- XML
    /// sibling elements with the same tag name have no per-instance
    /// disambiguation the way a JSON array's `[N]` index segment does, so
    /// `["book"]` can identify several distinct lines at once. Ignored
    /// (i.e. always the first occurrence) by every caller except
    /// `cycle_search_match`, which is the one place cycling through
    /// several same-path matches actually needs to land on a *specific*
    /// one rather than always the first (see issue #75's regression: a
    /// naive first-match resolution got Tab-cycling permanently stuck on
    /// one line whenever matches shared a path).
    pub(super) pending_cursor_occurrence: usize,
    /// Digits typed so far for a pending count-prefixed jump (`g` + digits
    /// + arrow). Capped at 6 digits to stay a safely parseable `usize`.
    pub(super) count_buffer: Option<String>,
    /// True while the whole-document search popup (`F`) is open.
    pub(super) popup_visible: bool,
    /// Separate from `search` so opening the popup doesn't clobber it.
    pub(super) popup_query: String,
    pub(super) popup_selected: usize,
    pub(super) inspect_visible: bool,
    pub(super) popup_scroll_offset: Cell<usize>,
    /// `--pick`: Enter prints the current path and exits, instead of doing
    /// nothing.
    pub(super) pick_mode: bool,
    /// Set by `handle_key` when `pick_mode` and Enter is pressed;
    /// `tui.rs`'s `run_loop` reads and prints it after leaving the
    /// alternate screen.
    pub(super) pick_result: Option<String>,
}

/// Source of truth for the in-app help overlay; cross-checked by a test
/// against README.md's keybindings table.
pub(super) const HELP_LEGEND: &[(&str, &str)] = &[
    ("↑ / ↓", "move cursor (or j/k)"),
    ("g", "count-jump: digits+↑/↓; gg/G: first/last"),
    ("Tab / Space", "collapse / expand (or h/l)"),
    ("Backspace", "collapse parent"),
    ("Shift+C", "collapse ancestors"),
    ("/", "fuzzy search (Tab: cycle; n/N: repeat)"),
    ("F", "search-results popup (whole document)"),
    ("i", "inspect node (type, size, path, tag)"),
    ("1-8", "tag / untag node"),
    ("x", "clear all tags"),
    ("y", "yank current path"),
    ("Y", "yank as jq path (JSON only)"),
    ("c", "collapse all"),
    ("e", "expand all"),
    ("?", "toggle this help"),
    ("q / Esc", "quit"),
];

fn apply_pending_cursor_path(state: &mut AppState) {
    if let Some(path) = state.pending_cursor_path.take() {
        let occurrence = std::mem::take(&mut state.pending_cursor_occurrence);
        super::keys::move_cursor_to_nth_path(state, &path, occurrence);
    }
}

pub(super) fn rebuild_json_lines(state: &mut AppState, node: &JsonNode) {
    let mut lines = Vec::new();
    flatten_json(
        node,
        &[],
        0,
        &state.collapsed,
        &state.array_overrides,
        &mut lines,
    );
    state.lines = lines;
    apply_pending_cursor_path(state);
    state.cursor = state.cursor.min(state.lines.len().saturating_sub(1));
}

pub(super) fn rebuild_xml_lines(state: &mut AppState, node: &XmlNode) {
    let mut lines = Vec::new();
    flatten_xml(node, &[], 0, &state.collapsed, &mut lines);
    state.lines = lines;
    apply_pending_cursor_path(state);
    state.cursor = state.cursor.min(state.lines.len().saturating_sub(1));
}

/// Same 24-bit tones as `color::code_truecolor` (kept in sync manually --
/// crossterm/ratatui negotiate the terminal's actual color depth and
/// degrade `Rgb` automatically, the same way `tag_color`/`depth_tint_color`
/// already rely on, so there's no separate ANSI16/truecolor tier to plumb
/// through the TUI the way the static renderer needs one). `Structural`
/// stays the original flat `DarkGray`, matching the static renderer's
/// choice to leave it a plain de-emphasized tone in every tier.
pub(super) fn ratatui_color(color: TqColor) -> Color {
    match color {
        TqColor::Key => Color::Rgb(86, 182, 194),
        TqColor::Str => Color::Rgb(152, 195, 121),
        TqColor::Number => Color::Rgb(229, 192, 123),
        TqColor::Bool => Color::Rgb(198, 120, 221),
        TqColor::Null => Color::Rgb(128, 128, 128),
        TqColor::Structural => Color::DarkGray,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_type_color_to_a_distinct_truecolor_ratatui_color() {
        let colors = [
            ratatui_color(TqColor::Key),
            ratatui_color(TqColor::Str),
            ratatui_color(TqColor::Number),
            ratatui_color(TqColor::Bool),
            ratatui_color(TqColor::Null),
        ];
        let unique: HashSet<Color> = colors.into_iter().collect();
        assert_eq!(unique.len(), 5, "every semantic color must be distinct");
        for c in colors {
            assert!(
                matches!(c, Color::Rgb(..)),
                "expected a truecolor Rgb value, got {c:?}"
            );
        }
    }

    #[test]
    fn structural_stays_the_original_flat_dark_gray() {
        assert_eq!(ratatui_color(TqColor::Structural), Color::DarkGray);
    }

    #[test]
    fn all_8_tags_get_distinct_colors() {
        let colors: HashSet<Color> = (1..=8).map(tag_color).collect();
        assert_eq!(
            colors.len(),
            8,
            "the expanded 1-8 tag palette must not repeat a color"
        );
    }

    #[test]
    fn tag_colors_are_dim_rgb_tints_not_harsh_ansi_blocks() {
        for tag in 1..=8 {
            match tag_color(tag) {
                Color::Rgb(r, g, b) => {
                    let max_channel = r.max(g).max(b);
                    assert!(
                        (50..=100).contains(&max_channel),
                        "tag {tag}'s peak channel must stay in a consistent dim band, got rgb({r}, {g}, {b})"
                    );
                }
                other => panic!("tag {tag}'s color must be Rgb, got {other:?}"),
            }
        }
    }

    #[test]
    fn tag_color_wraps_instead_of_panicking_outside_the_palette() {
        assert_eq!(tag_color(0), tag_color(1));
        assert_eq!(tag_color(9), tag_color(1));
    }

    #[test]
    fn depth_tint_colors_cycle_through_one_full_palette_without_repeats() {
        let colors: HashSet<Color> = (0..DEPTH_TINT_PALETTE.len())
            .map(depth_tint_color)
            .collect();
        assert_eq!(colors.len(), DEPTH_TINT_PALETTE.len());
    }

    #[test]
    fn depth_tint_wraps_around_past_the_palette_length() {
        assert_eq!(
            depth_tint_color(0),
            depth_tint_color(DEPTH_TINT_PALETTE.len())
        );
    }

    #[test]
    fn depth_zero_matches_the_original_structural_dark_gray() {
        assert_eq!(depth_tint_color(0), Color::DarkGray);
    }

    fn state_with_cursor(cursor: usize) -> AppState {
        AppState {
            lines: Vec::new(),
            collapsed: HashSet::new(),
            all_container_paths: HashSet::new(),
            array_overrides: HashSet::new(),
            tags: HashMap::new(),
            cursor,
            search: String::new(),
            searching: false,
            use_color: false,
            status_message: None,
            help_visible: false,
            is_json: true,
            scroll_offset: std::cell::Cell::new(0),
            all_paths: Vec::new(),
            pending_cursor_path: None,
            pending_cursor_occurrence: 0,
            count_buffer: None,
            popup_visible: false,
            popup_query: String::new(),
            popup_selected: 0,
            inspect_visible: false,
            popup_scroll_offset: std::cell::Cell::new(0),
            pick_mode: false,
            pick_result: None,
        }
    }

    #[test]
    fn rebuild_json_lines_repopulates_lines_from_the_tree() {
        let node = JsonNode::from_value(&serde_json::json!({"a": 1, "b": 2}));
        let mut state = state_with_cursor(0);
        rebuild_json_lines(&mut state, &node);
        assert_eq!(state.lines.len(), 2);
    }

    #[test]
    fn rebuild_json_lines_clamps_cursor_when_collapsing_shrinks_the_list() {
        let node = JsonNode::from_value(&serde_json::json!({"a": {"b": 1}}));
        let mut state = state_with_cursor(1);
        rebuild_json_lines(&mut state, &node);
        assert_eq!(state.lines.len(), 2);
        state.collapsed.insert(vec!["a".to_string()]);
        state.cursor = 1;
        rebuild_json_lines(&mut state, &node);
        assert_eq!(state.lines.len(), 1);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn rebuild_xml_lines_repopulates_lines_from_the_tree() {
        let doc = roxmltree::Document::parse("<root><a/><b/></root>").unwrap();
        let node = XmlNode::from_document(&doc).unwrap();
        let mut state = state_with_cursor(5);
        rebuild_xml_lines(&mut state, &node);
        assert_eq!(state.lines.len(), 2);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn rebuild_resolves_a_pending_cursor_path_to_its_new_index() {
        let node = JsonNode::from_value(&serde_json::json!({"a": {"b": 1}, "c": 2}));
        let mut state = state_with_cursor(0);
        state.pending_cursor_path = Some(vec!["c".to_string()]);
        rebuild_json_lines(&mut state, &node);
        assert_eq!(state.lines[state.cursor].path, vec!["c".to_string()]);
        assert!(
            state.pending_cursor_path.is_none(),
            "a resolved pending path must be cleared"
        );
    }

    #[test]
    fn rebuild_leaves_cursor_alone_when_the_pending_path_is_not_present() {
        let node = JsonNode::from_value(&serde_json::json!({"a": 1}));
        let mut state = state_with_cursor(0);
        state.pending_cursor_path = Some(vec!["missing".to_string()]);
        rebuild_json_lines(&mut state, &node);
        assert_eq!(state.cursor, 0);
        assert!(state.pending_cursor_path.is_none());
    }
}
