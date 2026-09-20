use super::state::AppState;
use crate::clipboard::copy_to_clipboard;
use crossterm::event::KeyCode;

/// Fuzzy subsequence match, case-insensitive: every character of `pattern`
/// must appear in `text` in order, though not necessarily contiguously
/// (e.g. "nme" matches "name"). A plain substring match is a special
/// case of this, so this is a strict superset of the old `contains` check.
fn fuzzy_matches(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    pattern
        .to_lowercase()
        .chars()
        .all(|p| chars.any(|c| c == p))
}

/// Matches against each visible line's own key, not the full dotted path,
/// and only among currently-expanded lines (`state.lines` excludes anything
/// under a collapsed ancestor) — searching into collapsed subtrees, or by
/// full path, is out of scope here (see issue #3).
pub(super) fn jump_to_next_match(state: &mut AppState) {
    if state.search.is_empty() {
        return;
    }
    let n = state.lines.len();
    for offset in 1..=n {
        let idx = (state.cursor + offset) % n;
        if fuzzy_matches(&state.lines[idx].key, &state.search) {
            state.cursor = idx;
            return;
        }
    }
}

/// Moves the cursor to the line at `path`, if one is currently visible.
fn move_cursor_to_path(state: &mut AppState, path: &[String]) {
    if let Some(idx) = state.lines.iter().position(|l| l.path == path) {
        state.cursor = idx;
    }
}

/// Collapses the immediate parent container of the current node (never the
/// node itself, even if it is a container) and moves the cursor there.
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

/// Collapses every ancestor of the current node up to the root in one
/// action, and moves the cursor to the outermost (root) ancestor.
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
    state.status_message = None;
    match key {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('?') => state.help_visible = true,
        KeyCode::Down => state.cursor = (state.cursor + 1).min(state.lines.len().saturating_sub(1)),
        KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Tab | KeyCode::Char(' ') => {
            if let Some(line) = state.lines.get(state.cursor)
                && line.has_children
                && !state.collapsed.remove(&line.path)
            {
                state.collapsed.insert(line.path.clone());
            }
        }
        KeyCode::Char('/') => {
            state.searching = true;
            state.search.clear();
        }
        KeyCode::Char('y') => {
            if let Some(line) = state.lines.get(state.cursor) {
                let path = line.path.join(".");
                state.status_message = Some(match copy_to_clipboard(&path) {
                    Ok(()) => format!("copied: {path}"),
                    Err(e) => format!("copy failed: {e}"),
                });
            }
        }
        KeyCode::Backspace => collapse_nearest_parent(state),
        KeyCode::Char('C') => collapse_all_ancestors(state),
        KeyCode::Char('c') => {
            state.collapsed = state.all_container_paths.clone();
            state.status_message = Some("collapsed all".to_string());
        }
        KeyCode::Char('e') => {
            state.collapsed.clear();
            state.status_message = Some("expanded all".to_string());
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
    use std::collections::HashSet;

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
            cursor: 0,
            search: String::new(),
            searching: false,
            use_color: false,
            status_message: None,
            help_visible: false,
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
            cursor: 3, // "city"
            search: String::new(),
            searching: false,
            use_color: false,
            status_message: None,
            help_visible: false,
        }
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
}
