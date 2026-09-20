use super::state::AppState;
use crate::clipboard::copy_to_clipboard;
use crossterm::event::KeyCode;

/// The array-truncation summary line's own path always ends with its own
/// synthetic label segment; strip it to get the array's real path (used to
/// key `array_overrides`, and to yank a real, pastable path with 'y').
fn array_path_for_summary_line(path: &[String]) -> &[String] {
    path.split_last().map_or(path, |(_, rest)| rest)
}

/// Fuzzy subsequence match, case-insensitive: every character of `pattern`
/// must appear in `text` in order, though not necessarily contiguously
/// (e.g. "nme" matches "name"). A plain substring match is a special
/// case of this, so this is a strict superset of the old `contains` check.
pub(super) fn fuzzy_matches(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    pattern
        .to_lowercase()
        .chars()
        .all(|p| chars.any(|c| c == p))
}

/// Matches against each currently-visible line's full dotted path, cycling
/// forward from the cursor. If nothing visible matches, falls back to
/// searching every node in the document — including collapsed subtrees —
/// and, on a match there, expands just enough ancestors to bring it into
/// view (see issue #30). The actual cursor move for that fallback case
/// happens once `lines` is rebuilt after this returns (see
/// `state::apply_pending_cursor_path`), since expanding a collapsed
/// ancestor doesn't take effect until then.
pub(super) fn jump_to_next_match(state: &mut AppState) {
    if state.search.is_empty() {
        return;
    }
    let n = state.lines.len();
    for offset in 1..=n {
        let idx = (state.cursor + offset) % n;
        if fuzzy_matches(&state.lines[idx].path.join("."), &state.search) {
            state.cursor = idx;
            return;
        }
    }
    if let Some(path) = state
        .all_paths
        .iter()
        .find(|path| fuzzy_matches(&path.join("."), &state.search))
        .cloned()
    {
        for i in 1..path.len() {
            let ancestor = path[..i].to_vec();
            state.collapsed.remove(&ancestor);
            // Also lift any array-preview truncation an ancestor might be
            // under (see issue #5): harmless to set on a non-array ancestor,
            // since `flatten_json` only consults it for arrays.
            state.array_overrides.insert(ancestor);
        }
        state.pending_cursor_path = Some(path);
    }
}

/// Moves the cursor to the line at `path`, if one is currently visible.
pub(super) fn move_cursor_to_path(state: &mut AppState, path: &[String]) {
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

/// True only for a segment shaped exactly like a synthesized array-index
/// segment (`"[0]"`, `"[12]"`, ...), as `flatten_json` produces via
/// `format!("[{i}]", ...)`. Note: a real object key that happens to be
/// spelled identically (e.g. a JSON document with a literal `"[0]"` key)
/// is indistinguishable from an array index in `Line.path` today — this is
/// a pre-existing ambiguity shared with the internal dotted-path 'y' yank
/// and `--path`, not something this jq conversion can resolve on its own.
fn is_array_index_segment(s: &str) -> bool {
    s.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Escapes a string for use inside a double-quoted jq string literal:
/// backslash, double-quote, and the common single-character escapes for
/// control characters that would otherwise break a "ready-to-run",
/// single-line filter when pasted (a literal newline/tab byte survives
/// otherwise, since `str::replace` only touches the two characters it's
/// given).
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

/// Converts a `Line`'s dotted-path segments into a jq filter expression,
/// e.g. `["user", "tags", "[0]"]` -> `.user.tags[0]`. Array-index segments
/// are already bracketed (`"[0]"`) by the flattener and pass through
/// as-is; object keys that aren't valid unquoted jq identifiers use
/// bracket-and-quote form instead (e.g. `.["odd key"]`).
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

/// Tags the current node with `tag` (1-8, see issue #40), or untags it if it
/// already carries that exact tag. Tagging the array-truncation summary
/// line tags the array's real path instead, consistent with 'y' yank (see
/// issue #5).
fn toggle_tag(state: &mut AppState, tag: u8) {
    let Some(line) = state.lines.get(state.cursor) else {
        return;
    };
    let path = if line.is_array_summary {
        array_path_for_summary_line(&line.path).to_vec()
    } else {
        line.path.clone()
    };
    // A tag's own background is suppressed on the cursor's line (it would
    // clash with the reversed-video cursor highlight), so this status
    // message is the only feedback when tagging the node you're on — the
    // common case, since you tag what you're looking at.
    if state.tags.get(&path) == Some(&tag) {
        state.tags.remove(&path);
        state.status_message = Some(format!("untagged: {}", path.join(".")));
    } else {
        state.tags.insert(path.clone(), tag);
        state.status_message = Some(format!("tagged {tag}: {}", path.join(".")));
    }
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
            if let Some(line) = state.lines.get(state.cursor) {
                if line.is_array_summary {
                    let array_path = array_path_for_summary_line(&line.path).to_vec();
                    state.array_overrides.insert(array_path);
                } else if line.has_children {
                    if state.collapsed.remove(&line.path) {
                        // Nothing further: re-expanding a container doesn't
                        // touch array_overrides.
                    } else {
                        state.collapsed.insert(line.path.clone());
                        // Re-collapsing an array also resets its preview:
                        // expanding it again later starts truncated, so a
                        // fully-expanded huge array always has a way back.
                        state.array_overrides.remove(&line.path);
                    }
                }
            }
        }
        KeyCode::Char('/') => {
            state.searching = true;
            state.search.clear();
        }
        KeyCode::Char('y') => {
            if let Some(line) = state.lines.get(state.cursor) {
                let path = if line.is_array_summary {
                    array_path_for_summary_line(&line.path).join(".")
                } else {
                    line.path.join(".")
                };
                state.status_message = Some(match copy_to_clipboard(&path) {
                    Ok(()) => format!("copied: {path}"),
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
                    Ok(()) => format!("copied: {jq}"),
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
                vec!["user".to_string()],
                vec!["user".to_string(), "name".to_string()],
                vec!["user".to_string(), "age".to_string()],
            ],
            pending_cursor_path: None,
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
        // A real container whose key happens to be spelled "…more" must
        // still behave as an ordinary collapsible node, not be mistaken
        // for the synthetic array-truncation summary line.
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
                vec!["root".to_string()],
                vec!["root".to_string(), "user".to_string()],
                vec![
                    "root".to_string(),
                    "user".to_string(),
                    "address".to_string(),
                ],
                vec![
                    "root".to_string(),
                    "user".to_string(),
                    "address".to_string(),
                    "city".to_string(),
                ],
            ],
            pending_cursor_path: None,
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
    fn jump_to_next_match_matches_against_the_full_dotted_path_not_just_the_own_key() {
        let mut state = fixture();
        // "usag" is not a subsequence of "age" alone, only of the full path
        // "user.age" — confirms matching now looks at the whole path.
        state.search = "usag".to_string();
        jump_to_next_match(&mut state);
        assert_eq!(state.cursor, 2, "\"user.age\" line must be reached");
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

        // 300 items is well past the 200-item array preview limit (issue
        // #5), so item [250] isn't in `lines` until the array is expanded.
        let items: Vec<serde_json::Value> = (0..300)
            .map(|i| serde_json::json!(format!("event-{i}")))
            .collect();
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
            // Search matches keys/paths, not scalar values, so this targets
            // the array index "[250]" itself rather than its "event-250"
            // string content.
            search: "250".to_string(),
            searching: true,
            use_color: false,
            status_message: None,
            help_visible: false,
            is_json: true,
            scroll_offset: std::cell::Cell::new(0),
            all_paths,
            pending_cursor_path: None,
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
        // 9 is deliberately left unbound (see issue #40): the palette tops
        // out at 8 to avoid `Light*`/base color pairs that look identical
        // on common terminal themes.
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('9'));
        assert!(state.tags.is_empty());
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
        // Only digits-in-brackets ("[0]") are treated as array indices;
        // anything else bracketed is a real (if unusual) object key.
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
}
