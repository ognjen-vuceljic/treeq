use super::flatten::{flatten_json, flatten_xml};
use crate::color::Color as TqColor;
use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use ratatui::style::Color;
use std::collections::HashSet;

pub(super) struct Line {
    pub(super) depth: usize,
    pub(super) key: String,
    pub(super) value: Option<(String, TqColor)>,
    pub(super) path: Vec<String>,
    pub(super) has_children: bool,
}

pub(super) struct AppState {
    pub(super) lines: Vec<Line>,
    pub(super) collapsed: HashSet<Vec<String>>,
    pub(super) all_container_paths: HashSet<Vec<String>>,
    pub(super) cursor: usize,
    pub(super) search: String,
    pub(super) searching: bool,
    pub(super) use_color: bool,
    pub(super) status_message: Option<String>,
    pub(super) help_visible: bool,
}

/// The keybinding legend shown when help is toggled on, as
/// (key, description) pairs, in display order. This is the source of truth
/// for the in-app overlay; a test in `tui::render` cross-checks that every
/// description here also appears in README.md's keybindings table so the
/// two can't silently drift apart.
pub(super) const HELP_LEGEND: &[(&str, &str)] = &[
    ("↑ / ↓", "move cursor"),
    ("Enter / Space", "collapse / expand"),
    ("/", "search"),
    ("y", "yank current path"),
    ("c", "collapse all"),
    ("e", "expand all"),
    ("?", "toggle this help"),
    ("q / Esc", "quit"),
];

pub(super) fn rebuild_json_lines(state: &mut AppState, node: &JsonNode) {
    let mut lines = Vec::new();
    flatten_json(node, &[], 0, &state.collapsed, &mut lines);
    state.lines = lines;
    state.cursor = state.cursor.min(state.lines.len().saturating_sub(1));
}

pub(super) fn rebuild_xml_lines(state: &mut AppState, node: &XmlNode) {
    let mut lines = Vec::new();
    flatten_xml(node, &[], 0, &state.collapsed, &mut lines);
    state.lines = lines;
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

    fn state_with_cursor(cursor: usize) -> AppState {
        AppState {
            lines: Vec::new(),
            collapsed: HashSet::new(),
            all_container_paths: HashSet::new(),
            cursor,
            search: String::new(),
            searching: false,
            use_color: false,
            status_message: None,
            help_visible: false,
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
}
