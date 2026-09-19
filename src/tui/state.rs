use crate::color::Color as TqColor;
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
