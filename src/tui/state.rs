use super::flatten::{flatten_json, flatten_xml};
use crate::color::Color as TqColor;
use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use ratatui::style::Color;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

/// The tag color palette, indexed by `tag - 1`. Bound to keys `1`-`8` (see
/// issue #40); an array here (rather than a fixed-arity match) is what
/// makes the tag count a one-line change instead of a restructuring.
///
/// Dim/dark RGB tints rather than raw ANSI hues (see issue #43): a
/// full-saturation background like `Color::Green` reads as a harsh solid
/// block and washes out the foreground text it's layered under. Each tint
/// here targets roughly the same dark luminance so foreground text (key
/// cyan, string green, number yellow, etc.) stays readable against every
/// one of them, while still being clearly distinct hue-to-hue.
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

/// One of `TAG_PALETTE`'s colors a node can be marked with via the `1`-`8`
/// keys (see issues #6, #40); additive to the type-based palette, applied
/// as a background rather than overriding a scalar's foreground color.
/// `tag` is 1-based; out-of-range values wrap rather than panic.
pub(super) fn tag_color(tag: u8) -> Color {
    let idx = (tag.saturating_sub(1) as usize) % TAG_PALETTE.len();
    TAG_PALETTE[idx]
}

pub(super) struct Line {
    pub(super) depth: usize,
    pub(super) key: String,
    pub(super) value: Option<(String, TqColor)>,
    pub(super) path: Vec<String>,
    pub(super) has_children: bool,
    /// True only for the synthetic "N more (Tab to show all)" line an
    /// oversized array is truncated to; distinguishes it from a real node
    /// so a real key that happens to be spelled like the sentinel marker
    /// text is never mistaken for one (see issue #5's array truncation).
    pub(super) is_array_summary: bool,
    /// A human-readable type/size description (e.g. "object (3 fields)",
    /// "string (5 chars)"), computed once at flatten time for the inspect
    /// popup (`i`, see issue #42) rather than re-walking the source tree.
    pub(super) type_label: String,
}

pub(super) struct AppState {
    pub(super) lines: Vec<Line>,
    pub(super) collapsed: HashSet<Vec<String>>,
    pub(super) all_container_paths: HashSet<Vec<String>>,
    /// Paths of arrays the user has chosen to fully expand past the
    /// default preview limit (see issue #5). Collapsing an array's own
    /// line (Tab/Space) also clears its entry here, so re-expanding it
    /// later starts truncated again — the way back to the fast preview.
    pub(super) array_overrides: HashSet<Vec<String>>,
    /// Nodes explicitly tagged (`1`-`8`, see issue #40) with a highlight
    /// color, keyed by path; in-memory only, reset each session like
    /// everything else here.
    pub(super) tags: HashMap<Vec<String>, u8>,
    pub(super) cursor: usize,
    pub(super) search: String,
    pub(super) searching: bool,
    pub(super) use_color: bool,
    pub(super) status_message: Option<String>,
    pub(super) help_visible: bool,
    /// True for a JSON (or YAML, which is rendered as JSON) document, false
    /// for XML. Gates the `Y` (yank as jq path) binding, since jq has no
    /// XML equivalent (see issue #8).
    pub(super) is_json: bool,
    /// The list viewport's scroll offset, carried over between frames so
    /// the cursor can move within an already-scrolled view instead of
    /// re-pinning to the window's edge every render (see issue #36's
    /// fix-review). `Cell` avoids threading `&mut AppState` through
    /// `render()` just for this.
    pub(super) scroll_offset: Cell<usize>,
    /// Every node's path in the whole document (containers and leaves),
    /// paired with its search text (dotted path, plus `: value` for a leaf
    /// — see issue #50), computed once at startup and unaffected by
    /// collapse state. Lets search reach into collapsed subtrees instead of
    /// being limited to `lines` (see issue #30) and match key/value combos.
    pub(super) all_paths: Vec<(Vec<String>, String)>,
    /// Set by a search match found outside the currently-visible lines: the
    /// path to select once `lines` has been rebuilt after expanding its
    /// ancestors (see issue #30). Consumed and cleared by
    /// `rebuild_json_lines` / `rebuild_xml_lines`.
    pub(super) pending_cursor_path: Option<Vec<String>>,
    /// While `Some`, a count-prefixed jump is being entered (`g`, then
    /// digits, then an arrow key — see issue #39): the digits typed so far,
    /// shown in the status bar. `None` means normal key handling applies.
    /// Capped at 6 digits while accumulating to keep it representable as a
    /// `usize` without needing overflow-checked parsing.
    pub(super) count_buffer: Option<String>,
    /// True while the interactive search-results popup is open (`F`, see
    /// issue #41): an fzf-like list of every match across the whole
    /// document (not just visible lines), as opposed to `/`'s one-at-a-time
    /// incremental jump.
    pub(super) popup_visible: bool,
    /// The popup's own query text, separate from `search` so opening the
    /// popup doesn't clobber (or get clobbered by) an in-progress `/`
    /// search.
    pub(super) popup_query: String,
    /// Index into the popup's current match list (recomputed from
    /// `popup_query` each frame, not stored). Clamped whenever the query or
    /// match count changes.
    pub(super) popup_selected: usize,
    /// True while the inspect popup is open (`i`, see issue #42): shows the
    /// current node's type/size, full path, and any tag, all otherwise not
    /// visible in the tree view at a glance.
    pub(super) inspect_visible: bool,
}

/// The keybinding legend shown when help is toggled on, as
/// (key, description) pairs, in display order. This is the source of truth
/// for the in-app overlay; a test in `tui::render` cross-checks that every
/// description here also appears in README.md's keybindings table so the
/// two can't silently drift apart.
pub(super) const HELP_LEGEND: &[(&str, &str)] = &[
    ("↑ / ↓", "move cursor"),
    ("g", "count-prefixed jump: type digits, then ↑/↓"),
    ("Tab / Space", "collapse / expand"),
    ("Backspace", "collapse parent"),
    ("Shift+C", "collapse ancestors"),
    ("/", "fuzzy search"),
    ("F", "search-results popup (whole document)"),
    ("i", "inspect node (type, size, path, tag)"),
    ("1-8", "tag / untag node"),
    ("y", "yank current path"),
    ("Y", "yank as jq path (JSON only)"),
    ("c", "collapse all"),
    ("e", "expand all"),
    ("?", "toggle this help"),
    ("q / Esc", "quit"),
];

/// If a search jump left a path pending selection, resolves it to the
/// freshly-rebuilt `lines`' index and clears it (see issue #30). Falls back
/// to leaving the cursor untouched if the path isn't visible after all
/// (shouldn't happen, since the caller expands its ancestors first).
fn apply_pending_cursor_path(state: &mut AppState) {
    if let Some(path) = state.pending_cursor_path.take() {
        super::keys::move_cursor_to_path(state, &path);
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

pub(super) fn ratatui_color(color: TqColor) -> Color {
    match color {
        TqColor::Key => Color::Cyan,
        TqColor::Str => Color::Green,
        TqColor::Number => Color::Yellow,
        TqColor::Bool => Color::Magenta,
        TqColor::Null | TqColor::Structural => Color::DarkGray,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_type_color_to_a_distinct_ratatui_color() {
        assert_eq!(ratatui_color(TqColor::Key), Color::Cyan);
        assert_eq!(ratatui_color(TqColor::Str), Color::Green);
        assert_eq!(ratatui_color(TqColor::Number), Color::Yellow);
        assert_eq!(ratatui_color(TqColor::Bool), Color::Magenta);
    }

    #[test]
    fn null_and_structural_share_the_same_dim_color() {
        assert_eq!(ratatui_color(TqColor::Null), Color::DarkGray);
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
        // Issue #43: a full-saturation ANSI background (e.g. Color::Green)
        // reads as a harsh solid block; every tag color must instead be a
        // dark/dim Rgb tint, all within a narrow luminance band, so no tag
        // reads noticeably brighter or dimmer than the rest (dim enough to
        // keep foreground text readable, bright enough to still show up
        // against a dark terminal theme's own background).
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
        // Defensive: `tag` is only ever produced by the `1`-`8` keybinding
        // today, but the function itself shouldn't panic on 0 or >8.
        assert_eq!(tag_color(0), tag_color(1));
        assert_eq!(tag_color(9), tag_color(1));
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
            count_buffer: None,
            popup_visible: false,
            popup_query: String::new(),
            popup_selected: 0,
            inspect_visible: false,
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
        let node = XmlNode::from_document(&doc);
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
