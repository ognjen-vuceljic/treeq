use super::flatten::display_path;
use super::state::AppState;
use crate::clipboard::copy_to_clipboard;
use crossterm::event::KeyCode;

/// Strips the array-truncation summary line's own synthetic segment to get
/// the array's real path.
pub(super) fn array_path_for_summary_line(path: &[String]) -> &[String] {
    path.split_last().map_or(path, |(_, rest)| rest)
}

/// Subsequence match, case-insensitive (e.g. "nme" matches "name").
pub(super) fn fuzzy_matches(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    pattern
        .to_lowercase()
        .chars()
        .all(|p| chars.any(|c| c == p))
}

/// Cycles forward through visible matches; falls back to the whole document
/// (including collapsed subtrees), expanding ancestors as needed. The
/// actual cursor move for that fallback happens once `lines` rebuilds (see
/// `state::apply_pending_cursor_path`).
pub(super) fn jump_to_next_match(state: &mut AppState) {
    if state.search.is_empty() {
        return;
    }
    let n = state.lines.len();
    for offset in 1..=n {
        let idx = (state.cursor + offset) % n;
        if fuzzy_matches(&line_search_text(&state.lines[idx]), &state.search) {
            state.cursor = idx;
            return;
        }
    }
    if let Some(path) = state
        .all_paths
        .iter()
        .find(|(_, text)| fuzzy_matches(text, &state.search))
        .map(|(path, _)| path.clone())
    {
        expand_path_into_view(state, &path);
        state.pending_cursor_path = Some(path);
    }
}

/// Delegates to `flatten::search_text` so visible-line and whole-document
/// search stay in the same format. The array-summary line's value is UI
/// chrome, not document data, so it's excluded from matching.
pub(super) fn line_search_text(line: &super::state::Line) -> String {
    let value = if line.is_array_summary {
        None
    } else {
        line.value.as_ref().map(|(v, _)| v.as_str())
    };
    super::flatten::search_text(&line.path, value)
}

/// Doesn't move the cursor itself; the caller sets `pending_cursor_path`,
/// since a collapsed ancestor doesn't take effect until `lines` rebuilds.
pub(super) fn expand_path_into_view(state: &mut AppState, path: &[String]) {
    for i in 1..path.len() {
        let ancestor = path[..i].to_vec();
        state.collapsed.remove(&ancestor);
        // Harmless on a non-array ancestor: flatten_json only reads this for arrays.
        state.array_overrides.insert(ancestor);
    }
}

pub(super) fn move_cursor_to_path(state: &mut AppState, path: &[String]) {
    if let Some(idx) = state.lines.iter().position(|l| l.path == path) {
        state.cursor = idx;
    }
}

/// Empty query intentionally yields no matches rather than dumping the
/// entire document.
pub(super) fn popup_matches(state: &AppState) -> Vec<Vec<String>> {
    popup_match_entries(state)
        .into_iter()
        .map(|(path, _)| path.clone())
        .collect()
}

/// Same matches as `popup_matches`, paired with their search text.
pub(super) fn popup_match_entries(state: &AppState) -> Vec<&(Vec<String>, String)> {
    if state.popup_query.is_empty() {
        return Vec::new();
    }
    state
        .all_paths
        .iter()
        .filter(|(_, text)| fuzzy_matches(text, &state.popup_query))
        .collect()
}

const MAX_COUNT_DIGITS: usize = 6;

fn apply_count_jump(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Char(c) if c.is_ascii_digit() => {
            if let Some(buf) = &mut state.count_buffer
                && buf.len() < MAX_COUNT_DIGITS
            {
                buf.push(c);
            }
        }
        KeyCode::Down => {
            let n = take_count(state);
            state.cursor = (state.cursor + n).min(state.lines.len().saturating_sub(1));
        }
        KeyCode::Up => {
            let n = take_count(state);
            state.cursor = state.cursor.saturating_sub(n);
        }
        _ => state.count_buffer = None,
    }
}

/// Defaults to 1 when the buffer is empty or unparseable.
fn take_count(state: &mut AppState) -> usize {
    state
        .count_buffer
        .take()
        .and_then(|buf| buf.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
}

fn apply_popup_key(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            if let Some(path) = popup_matches(state).get(state.popup_selected).cloned() {
                expand_path_into_view(state, &path);
                state.pending_cursor_path = Some(path);
            }
            state.popup_visible = false;
        }
        KeyCode::Backspace => {
            state.popup_query.pop();
            state.popup_selected = 0;
        }
        KeyCode::Down => {
            let n = popup_matches(state).len();
            if n > 0 {
                state.popup_selected = (state.popup_selected + 1).min(n - 1);
            }
        }
        KeyCode::Up => {
            state.popup_selected = state.popup_selected.saturating_sub(1);
        }
        KeyCode::Tab => cycle_popup_selection(state, 1),
        KeyCode::BackTab => cycle_popup_selection(state, -1),
        KeyCode::Char(c) => {
            state.popup_query.push(c);
            state.popup_selected = 0;
        }
        KeyCode::Esc => state.popup_visible = false,
        _ => {}
    }
}

fn cycle_popup_selection(state: &mut AppState, delta: i64) {
    let n = popup_matches(state).len();
    if n == 0 {
        return;
    }
    let n = n as i64;
    let cur = state.popup_selected as i64;
    state.popup_selected = (((cur + delta) % n + n) % n) as usize;
}

fn collapse_nearest_parent(state: &mut AppState) {
    let Some(line) = state.lines.get(state.cursor) else {
        return;
    };
    let path = line.path.clone();
    if path.len() < 2 {
        return;
    }
    let parent = path[..path.len() - 1].to_vec();
    state.collapsed.insert(parent.clone());
    move_cursor_to_path(state, &parent);
}

/// True if `s` is a valid unquoted jq object-key identifier: starts with a
/// letter or underscore, and contains only letters, digits, or underscores.
fn is_jq_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A real object key spelled like `"[0]"` is indistinguishable from a
/// synthesized array index here — a pre-existing ambiguity shared with 'y'
/// yank and `--path`.
fn is_array_index_segment(s: &str) -> bool {
    s.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn escape_jq_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// e.g. `["user", "tags", "[0]"]` -> `.user.tags[0]`.
fn to_jq_path(path: &[String]) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let mut out = String::new();
    for segment in path {
        if is_array_index_segment(segment) {
            out.push_str(segment);
        } else if is_jq_identifier(segment) {
            out.push('.');
            out.push_str(segment);
        } else {
            out.push_str(".[\"");
            out.push_str(&escape_jq_string(segment));
            out.push_str("\"]");
        }
    }
    out
}

fn toggle_tag(state: &mut AppState, tag: u8) {
    let Some(line) = state.lines.get(state.cursor) else {
        return;
    };
    let path = if line.is_array_summary {
        array_path_for_summary_line(&line.path).to_vec()
    } else {
        line.path.clone()
    };
    // The tag background is suppressed on the cursor's own line, so this
    // status message is the only feedback for the common case of tagging
    // the node you're looking at.
    if state.tags.get(&path) == Some(&tag) {
        state.tags.remove(&path);
        state.status_message = Some(format!("untagged: {}", display_path(&path)));
    } else {
        state.tags.insert(path.clone(), tag);
        state.status_message = Some(format!("tagged {tag}: {}", display_path(&path)));
    }
}

fn collapse_all_ancestors(state: &mut AppState) {
    let Some(line) = state.lines.get(state.cursor) else {
        return;
    };
    let path = line.path.clone();
    if path.len() < 2 {
        return;
    }
    for depth in 1..path.len() {
        state.collapsed.insert(path[..depth].to_vec());
    }
    move_cursor_to_path(state, &path[..1]);
    state.status_message = Some("collapsed ancestors".to_string());
}

pub(super) fn handle_key(state: &mut AppState, key: KeyCode) -> bool {
    if state.searching {
        match key {
            KeyCode::Enter | KeyCode::Esc => state.searching = false,
            KeyCode::Backspace => {
                state.search.pop();
            }
            KeyCode::Char(c) => {
                state.search.push(c);
                jump_to_next_match(state);
            }
            _ => {}
        }
        return false;
    }
    if state.help_visible {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') | KeyCode::Esc => state.help_visible = false,
            _ => {}
        }
        return false;
    }
    if state.popup_visible {
        apply_popup_key(state, key);
        return false;
    }
    if state.inspect_visible {
        match key {
            KeyCode::Char('q') => return true,
            KeyCode::Char('i') | KeyCode::Esc => state.inspect_visible = false,
            _ => {}
        }
        return false;
    }
    if state.count_buffer.is_some() {
        apply_count_jump(state, key);
        return false;
    }
    state.status_message = None;
    match key {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('?') => state.help_visible = true,
        KeyCode::Char('g') => state.count_buffer = Some(String::new()),
        KeyCode::Down => state.cursor = (state.cursor + 1).min(state.lines.len().saturating_sub(1)),
        KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Tab | KeyCode::Char(' ') => {
            if let Some(line) = state.lines.get(state.cursor) {
                if line.is_array_summary {
                    let array_path = array_path_for_summary_line(&line.path).to_vec();
                    state.array_overrides.insert(array_path);
                } else if line.has_children {
                    if state.collapsed.remove(&line.path) {
                        // no-op: re-expanding doesn't touch array_overrides
                    } else {
                        state.collapsed.insert(line.path.clone());
                        // Resets the preview so re-expanding starts truncated again.
                        state.array_overrides.remove(&line.path);
                    }
                }
            }
        }
        KeyCode::Char('/') => {
            state.searching = true;
            state.search.clear();
        }
        KeyCode::Char('F') => {
            state.popup_visible = true;
            // Pre-fill from an in-progress `/` search, if any.
            state.popup_query = state.search.clone();
            state.popup_selected = 0;
        }
        KeyCode::Char('i') => state.inspect_visible = true,
        KeyCode::Char('y') => {
            if let Some(line) = state.lines.get(state.cursor) {
                let path = if line.is_array_summary {
                    array_path_for_summary_line(&line.path)
                } else {
                    line.path.as_slice()
                };
                let raw = path.join(".");
                state.status_message = Some(match copy_to_clipboard(&raw) {
                    Ok(()) => format!("copied: {}", display_path(path)),
                    Err(e) => format!("copy failed: {e}"),
                });
            }
        }
        KeyCode::Char('Y') => {
            if !state.is_json {
                state.status_message =
                    Some("jq path is only available for JSON documents".to_string());
            } else if let Some(line) = state.lines.get(state.cursor) {
                let path = if line.is_array_summary {
                    array_path_for_summary_line(&line.path)
                } else {
                    &line.path
                };
                let jq = to_jq_path(path);
                state.status_message = Some(match copy_to_clipboard(&jq) {
                    // The copied jq filter is already escaped for jq syntax
                    // (`to_jq_path`); this is a *further*, separate escape
                    // for terminal safety on the status line only, not on
                    // what actually gets copied.
                    Ok(()) => format!("copied: {}", crate::json_tree::escape_display_str(&jq)),
                    Err(e) => format!("copy failed: {e}"),
                });
            }
        }
        KeyCode::Backspace => collapse_nearest_parent(state),
        KeyCode::Char('C') => collapse_all_ancestors(state),
        KeyCode::Char(c @ '1'..='8') => toggle_tag(state, c as u8 - b'0'),
        KeyCode::Char('c') => {
            state.collapsed = state.all_container_paths.clone();
            state.status_message = Some("collapsed all".to_string());
        }
        KeyCode::Char('e') => {
            state.collapsed.clear();
            state.status_message = Some("expanded all".to_string());
        }
        KeyCode::Char('x') => {
            let n = state.tags.len();
            state.tags.clear();
            state.status_message = Some(format!("cleared {n} tags"));
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::state::Line;
    use super::*;
    use crate::color::Color as TqColor;
    use std::collections::{HashMap, HashSet};

    fn line(key: &str, has_children: bool, path: &[&str]) -> Line {
        Line {
            depth: 0,
            key: key.to_string(),
            value: if has_children {
                None
            } else {
                Some((key.to_string(), TqColor::Str))
            },
            path: path.iter().map(|s| s.to_string()).collect(),
            has_children,
            is_array_summary: false,
            type_label: if has_children {
                "object (0 fields)".to_string()
            } else {
                "string (0 chars)".to_string()
            },
        }
    }

    fn array_summary_line(path: &[&str]) -> Line {
        Line {
            depth: 0,
            key: "…more".to_string(),
            value: Some(("3 more (Tab to show all)".to_string(), TqColor::Structural)),
            path: path.iter().map(|s| s.to_string()).collect(),
            has_children: false,
            is_array_summary: true,
            type_label: "array preview marker".to_string(),
        }
    }

    fn fixture() -> AppState {
        AppState {
            lines: vec![
                line("user", true, &["user"]),
                line("name", false, &["user", "name"]),
                line("age", false, &["user", "age"]),
            ],
            collapsed: HashSet::new(),
            all_container_paths: HashSet::from([vec!["user".to_string()]]),
            array_overrides: HashSet::new(),
            tags: HashMap::new(),
            cursor: 0,
            search: String::new(),
            searching: false,
            use_color: false,
            status_message: None,
            help_visible: false,
            is_json: true,
            scroll_offset: std::cell::Cell::new(0),
            all_paths: vec![
                (vec!["user".to_string()], "user".to_string()),
                (
                    vec!["user".to_string(), "name".to_string()],
                    "user.name".to_string(),
                ),
                (
                    vec!["user".to_string(), "age".to_string()],
                    "user.age".to_string(),
                ),
            ],
            pending_cursor_path: None,
            count_buffer: None,
            popup_visible: false,
            popup_query: String::new(),
            popup_selected: 0,
            inspect_visible: false,
            popup_scroll_offset: std::cell::Cell::new(0),
        }
    }

    #[test]
    fn q_and_esc_quit_when_not_searching() {
        assert!(handle_key(&mut fixture(), KeyCode::Char('q')));
        assert!(handle_key(&mut fixture(), KeyCode::Esc));
    }

    #[test]
    fn down_and_up_move_cursor_within_bounds() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.cursor, 1);
        handle_key(&mut state, KeyCode::Down);
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.cursor, 2, "cursor must not go past the last line");
        handle_key(&mut state, KeyCode::Up);
        handle_key(&mut state, KeyCode::Up);
        handle_key(&mut state, KeyCode::Up);
        assert_eq!(state.cursor, 0, "cursor must not go below zero");
    }

    #[test]
    fn tab_collapses_and_expands_a_container_line() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Tab);
        assert!(state.collapsed.contains(&vec!["user".to_string()]));
        handle_key(&mut state, KeyCode::Tab);
        assert!(!state.collapsed.contains(&vec!["user".to_string()]));
    }

    #[test]
    fn space_also_collapses_and_expands_a_container_line() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char(' '));
        assert!(state.collapsed.contains(&vec!["user".to_string()]));
        handle_key(&mut state, KeyCode::Char(' '));
        assert!(!state.collapsed.contains(&vec!["user".to_string()]));
    }

    #[test]
    fn tab_on_a_leaf_line_does_nothing() {
        let mut state = fixture();
        state.cursor = 1; // "name" has no children
        handle_key(&mut state, KeyCode::Tab);
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn tab_on_an_array_truncation_marker_expands_the_array_via_override() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        handle_key(&mut state, KeyCode::Tab);
        assert!(
            state
                .array_overrides
                .contains(&vec!["user".to_string(), "age".to_string()])
        );
        assert!(
            state.collapsed.is_empty(),
            "the marker line must not be treated as a collapsible container"
        );
    }

    #[test]
    fn a_real_key_that_looks_like_the_truncation_label_still_collapses_normally() {
        let mut state = fixture();
        state.lines.push(line("…more", true, &["user", "…more"]));
        state.cursor = 3;
        handle_key(&mut state, KeyCode::Tab);
        assert!(
            state
                .collapsed
                .contains(&vec!["user".to_string(), "…more".to_string()])
        );
        assert!(state.array_overrides.is_empty());
    }

    #[test]
    fn yanking_the_array_summary_line_copies_the_arrays_real_path() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        handle_key(&mut state, KeyCode::Char('y'));
        assert_eq!(
            state.status_message.as_deref(),
            Some("copied: user.age"),
            "yank must strip the synthetic marker segment, not include it"
        );
    }

    #[test]
    fn yank_status_message_escapes_control_bytes_in_the_path() {
        let mut state = fixture();
        state.lines[0] = line("before\u{1b}after", true, &["before\u{1b}after"]);
        handle_key(&mut state, KeyCode::Char('y'));
        let msg = state.status_message.as_deref().unwrap();
        assert!(!msg.contains('\u{1b}'));
        assert_eq!(msg, "copied: before\\u001bafter");
    }

    #[test]
    fn recollapsing_an_array_clears_its_expand_override() {
        let mut state = fixture();
        state.array_overrides.insert(vec!["user".to_string()]);
        handle_key(&mut state, KeyCode::Tab); // collapse "user"
        assert!(
            !state.array_overrides.contains(&vec!["user".to_string()]),
            "re-collapsing an array must reset it back to the truncated preview"
        );
    }

    fn nested_fixture() -> AppState {
        AppState {
            lines: vec![
                line("root", true, &["root"]),
                line("user", true, &["root", "user"]),
                line("address", true, &["root", "user", "address"]),
                line("city", false, &["root", "user", "address", "city"]),
            ],
            collapsed: HashSet::new(),
            all_container_paths: HashSet::from([
                vec!["root".to_string()],
                vec!["root".to_string(), "user".to_string()],
                vec![
                    "root".to_string(),
                    "user".to_string(),
                    "address".to_string(),
                ],
            ]),
            array_overrides: HashSet::new(),
            tags: HashMap::new(),
            cursor: 3, // "city"
            search: String::new(),
            searching: false,
            use_color: false,
            status_message: None,
            help_visible: false,
            is_json: true,
            scroll_offset: std::cell::Cell::new(0),
            all_paths: vec![
                (vec!["root".to_string()], "root".to_string()),
                (
                    vec!["root".to_string(), "user".to_string()],
                    "root.user".to_string(),
                ),
                (
                    vec![
                        "root".to_string(),
                        "user".to_string(),
                        "address".to_string(),
                    ],
                    "root.user.address".to_string(),
                ),
                (
                    vec![
                        "root".to_string(),
                        "user".to_string(),
                        "address".to_string(),
                        "city".to_string(),
                    ],
                    "root.user.address.city".to_string(),
                ),
            ],
            pending_cursor_path: None,
            count_buffer: None,
            popup_visible: false,
            popup_query: String::new(),
            popup_selected: 0,
            inspect_visible: false,
            popup_scroll_offset: std::cell::Cell::new(0),
        }
    }

    fn tall_fixture(n: usize, cursor: usize) -> AppState {
        AppState {
            lines: (0..n)
                .map(|i| line(&format!("item{i}"), false, &["item"]))
                .collect(),
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
            popup_scroll_offset: std::cell::Cell::new(0),
        }
    }

    #[test]
    fn g_then_digits_then_down_moves_the_cursor_that_many_lines() {
        let mut state = tall_fixture(20, 0);
        handle_key(&mut state, KeyCode::Char('g'));
        assert_eq!(state.count_buffer.as_deref(), Some(""));
        handle_key(&mut state, KeyCode::Char('5'));
        assert_eq!(state.count_buffer.as_deref(), Some("5"));
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.cursor, 5);
        assert!(
            state.count_buffer.is_none(),
            "the count is consumed after the jump"
        );
    }

    #[test]
    fn g_then_multi_digit_count_then_up_moves_the_cursor_that_many_lines() {
        let mut state = tall_fixture(20, 15);
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('1'));
        handle_key(&mut state, KeyCode::Char('2'));
        handle_key(&mut state, KeyCode::Up);
        assert_eq!(state.cursor, 3);
    }

    #[test]
    fn g_then_down_with_no_digits_moves_one_line_like_a_plain_arrow() {
        let mut state = tall_fixture(20, 0);
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn count_jump_clamps_to_the_last_line_instead_of_panicking() {
        let mut state = tall_fixture(5, 0);
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('9'));
        handle_key(&mut state, KeyCode::Char('9'));
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.cursor, 4, "must clamp to the last line, not panic");
    }

    #[test]
    fn count_jump_clamps_to_the_first_line_instead_of_underflowing() {
        let mut state = tall_fixture(20, 2);
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('9'));
        handle_key(&mut state, KeyCode::Up);
        assert_eq!(
            state.cursor, 0,
            "must clamp to the first line, not underflow"
        );
    }

    #[test]
    fn any_non_digit_non_arrow_key_cancels_a_pending_count() {
        let mut state = tall_fixture(20, 10);
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('5'));
        handle_key(&mut state, KeyCode::Esc);
        assert!(state.count_buffer.is_none());
        assert_eq!(state.cursor, 10, "canceling must not move the cursor");
    }

    #[test]
    fn digit_beyond_the_max_buffer_length_is_ignored_not_appended() {
        let mut state = tall_fixture(20, 0);
        handle_key(&mut state, KeyCode::Char('g'));
        for _ in 0..8 {
            handle_key(&mut state, KeyCode::Char('9'));
        }
        assert_eq!(
            state.count_buffer.as_deref().map(str::len),
            Some(6),
            "the buffer must stop growing past its cap"
        );
    }

    #[test]
    fn g_key_does_not_tag_the_current_node() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('g'));
        assert!(state.tags.is_empty());
    }

    #[test]
    fn shift_f_opens_the_popup_with_an_empty_query_by_default() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        assert!(state.popup_visible);
        assert_eq!(state.popup_query, "");
        assert_eq!(state.popup_selected, 0);
    }

    #[test]
    fn shift_f_prefills_the_popup_from_an_in_progress_search() {
        let mut state = fixture();
        state.search = "nam".to_string();
        handle_key(&mut state, KeyCode::Char('F'));
        assert_eq!(state.popup_query, "nam");
    }

    #[test]
    fn empty_popup_query_matches_nothing() {
        let state = fixture();
        assert!(popup_matches(&state).is_empty());
    }

    #[test]
    fn typing_in_the_popup_filters_matches_and_resets_selection() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        state.popup_selected = 5; // pretend a prior query had selected far down
        handle_key(&mut state, KeyCode::Char('a'));
        handle_key(&mut state, KeyCode::Char('g'));
        assert_eq!(state.popup_query, "ag");
        assert_eq!(state.popup_selected, 0);
        let matches = popup_matches(&state);
        assert_eq!(matches, vec![vec!["user".to_string(), "age".to_string()]]);
    }

    #[test]
    fn popup_matches_a_key_value_combo_reaching_a_collapsed_or_off_screen_node() {
        let mut state = fixture();
        state
            .all_paths
            .push((vec!["author".to_string()], "author: \"user0\"".to_string()));
        state.popup_query = "author: \"user".to_string();
        assert_eq!(popup_matches(&state), vec![vec!["author".to_string()]]);
    }

    #[test]
    fn popup_backspace_shrinks_the_query() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('a'));
        handle_key(&mut state, KeyCode::Backspace);
        assert_eq!(state.popup_query, "");
    }

    #[test]
    fn popup_down_and_up_move_the_selection_clamped_to_the_match_list() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        // "user" matches all 3 paths (user, user.name, user.age).
        handle_key(&mut state, KeyCode::Char('u'));
        assert_eq!(popup_matches(&state).len(), 3);
        handle_key(&mut state, KeyCode::Down);
        handle_key(&mut state, KeyCode::Down);
        handle_key(&mut state, KeyCode::Down);
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.popup_selected, 2, "must clamp to the last match");
        handle_key(&mut state, KeyCode::Up);
        handle_key(&mut state, KeyCode::Up);
        handle_key(&mut state, KeyCode::Up);
        handle_key(&mut state, KeyCode::Up);
        assert_eq!(state.popup_selected, 0, "must clamp to the first match");
    }

    #[test]
    fn tab_cycles_forward_and_wraps_past_the_last_match() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('u'));
        assert_eq!(popup_matches(&state).len(), 3);
        handle_key(&mut state, KeyCode::Tab);
        handle_key(&mut state, KeyCode::Tab);
        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(state.popup_selected, 0, "must wrap back to the first match");
    }

    #[test]
    fn back_tab_cycles_backward_and_wraps_before_the_first_match() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('u'));
        handle_key(&mut state, KeyCode::BackTab);
        assert_eq!(state.popup_selected, 2, "must wrap to the last match");
    }

    #[test]
    fn tab_does_nothing_when_there_are_no_matches() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('z'));
        assert!(popup_matches(&state).is_empty());
        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(state.popup_selected, 0);
    }

    #[test]
    fn tab_stays_put_with_exactly_one_match() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('n'));
        handle_key(&mut state, KeyCode::Char('a'));
        handle_key(&mut state, KeyCode::Char('m'));
        handle_key(&mut state, KeyCode::Char('e'));
        assert_eq!(
            popup_matches(&state).len(),
            1,
            "\"name\" matches only one path"
        );
        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(state.popup_selected, 0);
        handle_key(&mut state, KeyCode::BackTab);
        assert_eq!(state.popup_selected, 0);
    }

    #[test]
    fn typing_after_cycling_resets_selection_even_if_still_in_range() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('u'));
        handle_key(&mut state, KeyCode::Tab);
        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(state.popup_selected, 2);
        handle_key(&mut state, KeyCode::Backspace);
        assert_eq!(
            state.popup_selected, 0,
            "narrowing the query must not leave a stale out-of-range-prone selection"
        );
    }

    #[test]
    fn tab_and_back_tab_are_inverses_over_a_full_cycle() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('u'));
        let n = popup_matches(&state).len();
        for _ in 0..n {
            handle_key(&mut state, KeyCode::Tab);
        }
        assert_eq!(
            state.popup_selected, 0,
            "cycling forward exactly n times must land back on the start"
        );
        for _ in 0..n {
            handle_key(&mut state, KeyCode::BackTab);
        }
        assert_eq!(
            state.popup_selected, 0,
            "cycling backward exactly n times must also land back on the start"
        );
    }

    #[test]
    fn cycling_to_the_last_match_then_narrowing_the_query_keeps_selection_in_range() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('u'));
        assert_eq!(popup_matches(&state).len(), 3);
        handle_key(&mut state, KeyCode::BackTab); // wraps to the last match
        assert_eq!(state.popup_selected, 2);
        handle_key(&mut state, KeyCode::Char('z')); // "uz" matches nothing
        assert!(popup_matches(&state).is_empty());
        assert_eq!(state.popup_selected, 0);
    }

    #[test]
    fn down_clamp_then_tab_still_wraps_correctly() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('u'));
        handle_key(&mut state, KeyCode::Down);
        handle_key(&mut state, KeyCode::Down);
        handle_key(&mut state, KeyCode::Down);
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.popup_selected, 2, "Down clamps at the last match");
        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(state.popup_selected, 0, "Tab still wraps past the clamp");
        handle_key(&mut state, KeyCode::BackTab);
        assert_eq!(
            state.popup_selected, 2,
            "BackTab wraps back to the last match"
        );
    }

    #[test]
    fn popup_esc_closes_without_moving_the_cursor_or_expanding_anything() {
        let mut state = nested_fixture();
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        state.lines = vec![
            line("root", true, &["root"]),
            line("user", true, &["root", "user"]),
        ];
        state.cursor = 0;
        handle_key(&mut state, KeyCode::Char('F'));
        handle_key(&mut state, KeyCode::Char('c'));
        handle_key(&mut state, KeyCode::Char('i'));
        handle_key(&mut state, KeyCode::Char('t'));
        handle_key(&mut state, KeyCode::Char('y'));
        handle_key(&mut state, KeyCode::Esc);
        assert!(!state.popup_visible);
        assert_eq!(state.cursor, 0);
        assert!(
            state
                .collapsed
                .contains(&vec!["root".to_string(), "user".to_string()]),
            "closing without Enter must not expand anything"
        );
        assert!(state.pending_cursor_path.is_none());
    }

    #[test]
    fn popup_enter_expands_the_selected_matchs_ancestors_and_queues_the_cursor() {
        let mut state = nested_fixture();
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        state.lines = vec![
            line("root", true, &["root"]),
            line("user", true, &["root", "user"]),
        ];
        state.cursor = 0;
        handle_key(&mut state, KeyCode::Char('F'));
        for c in "city".chars() {
            handle_key(&mut state, KeyCode::Char(c));
        }
        assert_eq!(
            popup_matches(&state),
            vec![vec![
                "root".to_string(),
                "user".to_string(),
                "address".to_string(),
                "city".to_string(),
            ]]
        );
        handle_key(&mut state, KeyCode::Enter);
        assert!(!state.popup_visible, "Enter must close the popup");
        assert!(
            !state
                .collapsed
                .contains(&vec!["root".to_string(), "user".to_string()]),
            "the collapsed ancestor must be expanded"
        );
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(
                vec![
                    "root".to_string(),
                    "user".to_string(),
                    "address".to_string(),
                    "city".to_string(),
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn popup_enter_with_no_matches_just_closes_the_popup() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        for c in "zzz".chars() {
            handle_key(&mut state, KeyCode::Char(c));
        }
        assert!(popup_matches(&state).is_empty());
        handle_key(&mut state, KeyCode::Enter);
        assert!(!state.popup_visible);
        assert!(state.pending_cursor_path.is_none());
    }

    #[test]
    fn typing_q_in_the_popup_appends_to_the_query_instead_of_quitting() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('F'));
        let quit = handle_key(&mut state, KeyCode::Char('q'));
        assert!(!quit);
        assert_eq!(state.popup_query, "q");
    }

    #[test]
    fn backspace_collapses_the_nearest_parent_and_moves_cursor_there() {
        let mut state = nested_fixture();
        handle_key(&mut state, KeyCode::Backspace);
        assert!(state.collapsed.contains(&vec![
            "root".to_string(),
            "user".to_string(),
            "address".to_string()
        ]));
        assert_eq!(state.cursor, 2, "cursor must move to the collapsed parent");
    }

    #[test]
    fn backspace_on_a_container_collapses_its_parent_not_itself() {
        let mut state = nested_fixture();
        state.cursor = 2; // "address", itself a container
        handle_key(&mut state, KeyCode::Backspace);
        assert!(
            !state.collapsed.contains(&vec![
                "root".to_string(),
                "user".to_string(),
                "address".to_string()
            ]),
            "must not collapse the current node itself"
        );
        assert!(
            state
                .collapsed
                .contains(&vec!["root".to_string(), "user".to_string()])
        );
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn backspace_at_the_root_does_nothing() {
        let mut state = nested_fixture();
        state.cursor = 0; // "root", no parent
        handle_key(&mut state, KeyCode::Backspace);
        assert!(state.collapsed.is_empty());
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn shift_c_collapses_every_ancestor_and_moves_cursor_to_the_root() {
        let mut state = nested_fixture();
        handle_key(&mut state, KeyCode::Char('C'));
        assert!(state.collapsed.contains(&vec!["root".to_string()]));
        assert!(
            state
                .collapsed
                .contains(&vec!["root".to_string(), "user".to_string()])
        );
        assert!(state.collapsed.contains(&vec![
            "root".to_string(),
            "user".to_string(),
            "address".to_string()
        ]));
        assert_eq!(state.cursor, 0);
        assert_eq!(state.status_message.as_deref(), Some("collapsed ancestors"));
    }

    #[test]
    fn shift_c_at_the_root_does_nothing() {
        let mut state = nested_fixture();
        state.cursor = 0; // "root", no ancestors
        handle_key(&mut state, KeyCode::Char('C'));
        assert!(state.collapsed.is_empty());
        assert_eq!(state.status_message, None);
    }

    #[test]
    fn enter_no_longer_collapses_a_container_line() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Enter);
        assert!(
            state.collapsed.is_empty(),
            "Enter is unbound outside search mode; only Tab/Space collapse"
        );
    }

    #[test]
    fn slash_enters_search_mode_and_clears_buffer() {
        let mut state = fixture();
        state.search = "stale".to_string();
        handle_key(&mut state, KeyCode::Char('/'));
        assert!(state.searching);
        assert_eq!(state.search, "");
    }

    #[test]
    fn typing_while_searching_appends_and_does_not_quit_on_q() {
        let mut state = fixture();
        state.searching = true;
        let quit = handle_key(&mut state, KeyCode::Char('q'));
        assert!(!quit, "'q' must type into the search buffer, not quit");
        assert_eq!(state.search, "q");
    }

    #[test]
    fn backspace_while_searching_pops_last_char() {
        let mut state = fixture();
        state.searching = true;
        state.search = "ab".to_string();
        handle_key(&mut state, KeyCode::Backspace);
        assert_eq!(state.search, "a");
    }

    #[test]
    fn enter_or_esc_exits_search_mode_without_collapsing() {
        let mut state = fixture();
        state.searching = true;
        handle_key(&mut state, KeyCode::Enter);
        assert!(!state.searching);
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn c_collapses_every_known_container_and_sets_status() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('c'));
        assert_eq!(state.collapsed, state.all_container_paths);
        assert_eq!(state.status_message.as_deref(), Some("collapsed all"));
    }

    #[test]
    fn e_expands_everything_and_sets_status() {
        let mut state = fixture();
        state.collapsed = state.all_container_paths.clone();
        handle_key(&mut state, KeyCode::Char('e'));
        assert!(state.collapsed.is_empty());
        assert_eq!(state.status_message.as_deref(), Some("expanded all"));
    }

    #[test]
    fn y_sets_a_copied_status_message_with_the_current_path() {
        let mut state = fixture();
        state.cursor = 1; // "user.name"
        handle_key(&mut state, KeyCode::Char('y'));
        assert_eq!(state.status_message.as_deref(), Some("copied: user.name"));
    }

    #[test]
    fn status_message_clears_on_the_next_non_search_keypress() {
        let mut state = fixture();
        state.status_message = Some("stale".to_string());
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.status_message, None);
    }

    #[test]
    fn jump_to_next_match_wraps_from_the_last_line_back_to_the_first() {
        let mut state = fixture();
        state.cursor = 2; // sitting on the last line ("age")
        state.search = "USER".to_string(); // only matches line 0, case-insensitively
        jump_to_next_match(&mut state);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn jump_to_next_match_leaves_cursor_when_nothing_matches() {
        let mut state = fixture();
        state.search = "zzz".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn jump_to_next_match_does_nothing_for_empty_search() {
        let mut state = fixture();
        state.cursor = 1;
        jump_to_next_match(&mut state);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn fuzzy_matches_matches_non_contiguous_characters_in_order() {
        assert!(fuzzy_matches("user.name", "usnm"));
        assert!(fuzzy_matches("user.name", "user.name"));
        assert!(fuzzy_matches("USER", "user"));
        assert!(!fuzzy_matches("user", "usnm"));
        assert!(!fuzzy_matches("abc", "cab"), "order must be preserved");
    }

    #[test]
    fn jump_to_next_match_finds_a_fuzzy_non_contiguous_match() {
        let mut state = fixture();
        // "nme" is not a substring of "name" but is a subsequence (n-a-m-e).
        state.search = "nme".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(
            state.cursor, 1,
            "must fuzzy-match \"name\" via non-contiguous chars"
        );
    }

    #[test]
    fn jump_to_next_match_matches_against_the_full_dotted_path_not_just_the_own_key() {
        let mut state = fixture();
        // "usag" is not a subsequence of "age" alone, only of the full path
        // "user.age" — confirms matching now looks at the whole path.
        state.search = "usag".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(state.cursor, 2, "\"user.age\" line must be reached");
    }

    #[test]
    fn jump_to_next_match_finds_a_visible_lines_key_value_combo() {
        let mut state = fixture();
        state.lines.push(Line {
            depth: 0,
            key: "author".to_string(),
            value: Some(("\"user0\"".to_string(), TqColor::Str)),
            path: vec!["author".to_string()],
            has_children: false,
            is_array_summary: false,
            type_label: "string (5 chars)".to_string(),
        });
        state.search = "author: \"user".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(state.cursor, 3, "the author line must be reached");
    }

    #[test]
    fn search_ignores_the_array_summary_lines_synthetic_boilerplate_value() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.search = "tab to show".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(
            state.cursor, 0,
            "the synthetic summary text must not be searchable"
        );
    }

    #[test]
    fn jump_to_next_match_reaches_into_a_collapsed_subtree() {
        let mut state = nested_fixture();
        // Only "root" and "user" are visible; "address" (and "city" beneath
        // it) is hidden behind a collapsed ancestor.
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        state.lines = vec![
            line("root", true, &["root"]),
            line("user", true, &["root", "user"]),
        ];
        state.cursor = 0;
        state.search = "city".to_string();

        jump_to_next_match(&mut state);

        assert!(
            !state
                .collapsed
                .contains(&vec!["root".to_string(), "user".to_string()]),
            "the collapsed ancestor must be expanded so the match becomes reachable"
        );
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(
                vec![
                    "root".to_string(),
                    "user".to_string(),
                    "address".to_string(),
                    "city".to_string(),
                ]
                .as_slice()
            ),
            "the matched path must be queued for cursor placement once lines rebuild"
        );
    }

    #[test]
    fn jump_to_next_match_prefers_a_visible_match_over_the_full_document_search() {
        let mut state = nested_fixture();
        state.search = "user".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(
            state.cursor, 1,
            "a match already visible must win without touching collapsed state"
        );
        assert!(state.pending_cursor_path.is_none());
    }

    #[test]
    fn jump_to_next_match_reaches_a_leaf_beyond_the_array_preview_truncation() {
        use super::super::flatten::{collect_all_paths_json, flatten_json};
        use super::super::state::rebuild_json_lines;
        use crate::json_tree::JsonNode;

        // Digit-free values so "250" can only match the array index, not
        // bleed across the key/value boundary into another item's value.
        let items: Vec<serde_json::Value> = (0..300).map(|_| serde_json::json!("x")).collect();
        let value = serde_json::json!({ "logs": items });
        let node = JsonNode::from_value(&value);

        let mut all_paths = Vec::new();
        collect_all_paths_json(&node, &[], &mut all_paths);
        let mut lines = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &HashSet::new(), &mut lines);

        let mut state = AppState {
            lines,
            collapsed: HashSet::new(),
            all_container_paths: HashSet::from([vec!["logs".to_string()]]),
            array_overrides: HashSet::new(),
            tags: HashMap::new(),
            cursor: 0,
            // Targets the array index "[250]" itself.
            search: "250".to_string(),
            searching: true,
            use_color: false,
            status_message: None,
            help_visible: false,
            is_json: true,
            scroll_offset: std::cell::Cell::new(0),
            all_paths,
            pending_cursor_path: None,
            count_buffer: None,
            popup_visible: false,
            popup_query: String::new(),
            popup_selected: 0,
            inspect_visible: false,
            popup_scroll_offset: std::cell::Cell::new(0),
        };

        jump_to_next_match(&mut state);
        rebuild_json_lines(&mut state, &node);

        assert!(
            state.array_overrides.contains(&vec!["logs".to_string()]),
            "the array must be lifted out of preview-truncation for the match to be reachable"
        );
        assert_eq!(
            state.lines[state.cursor].path,
            vec!["logs".to_string(), "[250]".to_string()]
        );
    }

    #[test]
    fn jump_to_next_match_leaves_state_untouched_when_nothing_matches_anywhere() {
        let mut state = nested_fixture();
        state.cursor = 0;
        state.search = "zzz".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(state.cursor, 0);
        assert!(state.pending_cursor_path.is_none());
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn question_mark_toggles_help_visibility() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('?'));
        assert!(state.help_visible);
        handle_key(&mut state, KeyCode::Char('?'));
        assert!(!state.help_visible);
    }

    #[test]
    fn while_help_is_visible_other_keys_are_swallowed_except_dismiss() {
        let mut state = fixture();
        state.help_visible = true;
        let quit = handle_key(&mut state, KeyCode::Down);
        assert!(!quit);
        assert_eq!(state.cursor, 0, "cursor must not move while help is shown");
        assert!(state.help_visible, "an unrelated key must not dismiss help");
    }

    #[test]
    fn esc_dismisses_help_without_quitting() {
        let mut state = fixture();
        state.help_visible = true;
        let quit = handle_key(&mut state, KeyCode::Esc);
        assert!(!quit);
        assert!(!state.help_visible);
    }

    #[test]
    fn q_quits_even_while_help_is_visible() {
        let mut state = fixture();
        state.help_visible = true;
        assert!(handle_key(&mut state, KeyCode::Char('q')));
    }

    #[test]
    fn digit_key_tags_the_current_node() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('2'));
        assert_eq!(state.tags.get(&vec!["user".to_string()]), Some(&2));
        assert_eq!(
            state.status_message.as_deref(),
            Some("tagged 2: user"),
            "tagging the cursor's own line has no visible background change, \
             so the status message is the only feedback"
        );
    }

    #[test]
    fn tag_status_message_escapes_control_bytes_in_the_path() {
        let mut state = fixture();
        state.lines[0] = line("before\u{1b}after", true, &["before\u{1b}after"]);
        handle_key(&mut state, KeyCode::Char('2'));
        let msg = state.status_message.as_deref().unwrap();
        assert!(!msg.contains('\u{1b}'));
        assert_eq!(msg, "tagged 2: before\\u001bafter");
        // The actual tags map key must stay the real, unescaped path.
        assert!(
            state
                .tags
                .contains_key(&vec!["before\u{1b}after".to_string()])
        );
    }

    #[test]
    fn pressing_the_same_digit_again_untags_the_node() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('3'));
        handle_key(&mut state, KeyCode::Char('3'));
        assert!(state.tags.is_empty());
        assert_eq!(state.status_message.as_deref(), Some("untagged: user"));
    }

    #[test]
    fn pressing_a_different_digit_replaces_the_existing_tag() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('1'));
        handle_key(&mut state, KeyCode::Char('4'));
        assert_eq!(state.tags.get(&vec!["user".to_string()]), Some(&4));
        assert_eq!(state.tags.len(), 1);
    }

    #[test]
    fn digit_8_tags_the_node_using_the_expanded_palette() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('8'));
        assert_eq!(state.tags.get(&vec!["user".to_string()]), Some(&8));
        assert_eq!(state.status_message.as_deref(), Some("tagged 8: user"));
    }

    #[test]
    fn digit_9_is_not_bound_to_tagging() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('9'));
        assert!(state.tags.is_empty());
    }

    #[test]
    fn x_key_clears_every_tag_at_once() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('1'));
        state.cursor = 1;
        handle_key(&mut state, KeyCode::Char('2'));
        assert_eq!(state.tags.len(), 2);
        handle_key(&mut state, KeyCode::Char('x'));
        assert!(state.tags.is_empty());
        assert_eq!(state.status_message.as_deref(), Some("cleared 2 tags"));
    }

    #[test]
    fn x_key_with_no_tags_reports_zero_cleared() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('x'));
        assert!(state.tags.is_empty());
        assert_eq!(state.status_message.as_deref(), Some("cleared 0 tags"));
    }

    #[test]
    fn tagging_the_array_summary_line_tags_the_arrays_real_path() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        handle_key(&mut state, KeyCode::Char('1'));
        assert_eq!(
            state.tags.get(&vec!["user".to_string(), "age".to_string()]),
            Some(&1)
        );
    }

    #[test]
    fn is_jq_identifier_accepts_plain_identifiers_and_rejects_the_rest() {
        assert!(is_jq_identifier("user"));
        assert!(is_jq_identifier("_private"));
        assert!(is_jq_identifier("user2"));
        assert!(!is_jq_identifier("2fast"), "must not start with a digit");
        assert!(!is_jq_identifier("odd key"), "must not contain a space");
        assert!(!is_jq_identifier(""), "must not be empty");
    }

    #[test]
    fn to_jq_path_converts_object_keys_and_array_indices() {
        assert_eq!(
            to_jq_path(&["user".to_string(), "tags".to_string(), "[0]".to_string()]),
            ".user.tags[0]"
        );
    }

    #[test]
    fn to_jq_path_quotes_a_key_that_is_not_a_valid_identifier() {
        assert_eq!(to_jq_path(&["odd key".to_string()]), ".[\"odd key\"]");
    }

    #[test]
    fn to_jq_path_escapes_embedded_quotes_and_backslashes_in_a_quoted_key() {
        assert_eq!(
            to_jq_path(&["say \"hi\"".to_string()]),
            ".[\"say \\\"hi\\\"\"]"
        );
    }

    #[test]
    fn to_jq_path_of_the_root_is_a_bare_dot() {
        assert_eq!(to_jq_path(&[]), ".");
    }

    #[test]
    fn to_jq_path_escapes_control_characters_in_a_quoted_key() {
        assert_eq!(
            to_jq_path(&["line\nbreak".to_string()]),
            ".[\"line\\nbreak\"]"
        );
        assert_eq!(to_jq_path(&["a\tb".to_string()]), ".[\"a\\tb\"]");
    }

    #[test]
    fn a_key_shaped_like_a_bracketed_word_is_still_quoted_not_treated_as_an_index() {
        assert_eq!(to_jq_path(&["[odd]".to_string()]), ".[\"[odd]\"]");
        assert_eq!(to_jq_path(&["[]".to_string()]), ".[\"[]\"]");
    }

    #[test]
    fn shift_y_yanks_a_jq_path_for_json_documents() {
        let mut state = fixture();
        state.cursor = 1; // "user.name"
        handle_key(&mut state, KeyCode::Char('Y'));
        assert_eq!(state.status_message.as_deref(), Some("copied: .user.name"));
    }

    #[test]
    fn shift_y_is_unavailable_for_xml_documents() {
        let mut state = fixture();
        state.is_json = false;
        state.cursor = 1;
        handle_key(&mut state, KeyCode::Char('Y'));
        assert_eq!(
            state.status_message.as_deref(),
            Some("jq path is only available for JSON documents")
        );
    }

    #[test]
    fn shift_y_on_the_array_summary_line_yanks_the_arrays_real_jq_path() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        handle_key(&mut state, KeyCode::Char('Y'));
        assert_eq!(state.status_message.as_deref(), Some("copied: .user.age"));
    }

    #[test]
    fn i_opens_the_inspect_popup() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('i'));
        assert!(state.inspect_visible);
    }

    #[test]
    fn i_closes_the_inspect_popup_when_already_open() {
        let mut state = fixture();
        state.inspect_visible = true;
        handle_key(&mut state, KeyCode::Char('i'));
        assert!(!state.inspect_visible);
    }

    #[test]
    fn esc_closes_the_inspect_popup_instead_of_quitting() {
        let mut state = fixture();
        state.inspect_visible = true;
        assert!(!handle_key(&mut state, KeyCode::Esc));
        assert!(!state.inspect_visible);
    }

    #[test]
    fn q_still_quits_while_the_inspect_popup_is_open() {
        let mut state = fixture();
        state.inspect_visible = true;
        assert!(handle_key(&mut state, KeyCode::Char('q')));
    }

    #[test]
    fn other_keys_are_ignored_while_the_inspect_popup_is_open() {
        let mut state = fixture();
        state.inspect_visible = true;
        handle_key(&mut state, KeyCode::Down);
        assert_eq!(state.cursor, 0, "cursor must not move while inspecting");
        assert!(state.inspect_visible);
    }
}
