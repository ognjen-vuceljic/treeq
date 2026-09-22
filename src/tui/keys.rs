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
    fuzzy_match_char_indices(text, pattern).is_some()
}

/// Same matching rule as `fuzzy_matches`, but returns the char index (into
/// `text`, greedy left-to-right, one per matched pattern char) of each
/// match instead of a plain yes/no -- lets a renderer underline exactly the
/// characters that matched a fuzzy/non-contiguous query instead of the
/// whole line.
pub(super) fn fuzzy_match_char_indices(text: &str, pattern: &str) -> Option<Vec<usize>> {
    let lower_text: Vec<char> = text.to_lowercase().chars().collect();
    let mut indices = Vec::with_capacity(pattern.len());
    let mut cursor = 0;
    for p in pattern.to_lowercase().chars() {
        let found = lower_text[cursor..].iter().position(|&c| c == p)?;
        indices.push(cursor + found);
        cursor += found + 1;
    }
    Some(indices)
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

/// Steps to the next (`delta = 1`) or previous (`delta = -1`) match for the
/// in-progress `/` search, across the whole document (including collapsed
/// subtrees and truncated arrays) rather than just what's currently visible
/// -- the same match universe the `F` popup's own Tab/Shift+Tab cycling
/// already uses (`all_paths`), so behavior is consistent between the two
/// search modes. Wraps around at either end.
pub(super) fn cycle_search_match(state: &mut AppState, delta: i64) {
    if state.search.is_empty() {
        return;
    }
    // Each match also carries which occurrence (0-based) of its own path
    // it is among matches sharing that exact path -- distinct XML sibling
    // elements with the same tag name have no other way to tell them
    // apart, since (unlike a JSON array's `[N]` segment) their path is
    // identical. Without this, cycling would resolve every one of them
    // back to the same (first) line and get permanently stuck (issue #75
    // regression, caught by adversarial review before merging).
    let mut path_counts: std::collections::HashMap<Vec<String>, usize> =
        std::collections::HashMap::new();
    let matches: Vec<(Vec<String>, usize)> = state
        .all_paths
        .iter()
        .filter(|(_, text)| fuzzy_matches(text, &state.search))
        .map(|(path, _)| {
            let occurrence = path_counts.entry(path.clone()).or_insert(0);
            let this = *occurrence;
            *occurrence += 1;
            (path.clone(), this)
        })
        .collect();
    if matches.is_empty() {
        return;
    }
    let n = matches.len() as i64;
    // The cursor's own occurrence rank among *visible* lines sharing its
    // real path: visible order is a subsequence of document order
    // (collapsing never reorders siblings), so this rank lines up with the
    // rank computed above for `matches`.
    let current_idx = state.lines.get(state.cursor).and_then(|current| {
        let current_path = real_path(current);
        let rank = state.lines[..=state.cursor]
            .iter()
            .filter(|l| real_path(l) == current_path)
            .count()
            - 1;
        matches
            .iter()
            .position(|(p, occurrence)| p.as_slice() == current_path && *occurrence == rank)
    });
    let next_idx = match current_idx {
        Some(idx) => (((idx as i64 + delta) % n + n) % n) as usize,
        None if delta >= 0 => 0,
        None => (n - 1) as usize,
    };
    let (path, occurrence) = matches[next_idx].clone();
    expand_path_into_view(state, &path);
    state.pending_cursor_path = Some(path);
    state.pending_cursor_occurrence = occurrence;
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
    move_cursor_to_nth_path(state, path, 0);
}

/// Same as `move_cursor_to_path`, but lands on the `occurrence`-th (0-based)
/// line matching `path` rather than always the first -- needed when several
/// lines share the exact same path (see `AppState::pending_cursor_occurrence`).
pub(super) fn move_cursor_to_nth_path(state: &mut AppState, path: &[String], occurrence: usize) {
    if let Some(idx) = state
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.path == path)
        .nth(occurrence)
        .map(|(idx, _)| idx)
    {
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
        KeyCode::Down | KeyCode::Char('j') => {
            let n = take_count(state);
            state.cursor = (state.cursor + n).min(state.lines.len().saturating_sub(1));
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let n = take_count(state);
            state.cursor = state.cursor.saturating_sub(n);
        }
        // `gg` (empty buffer) jumps to line 1; `g5g`/`g5G` jump to line 5
        // (1-based) -- the app's own `g`-prefixed count syntax rather than
        // vim's literal `5gg`, since bare digits already mean "tag node"
        // outside count mode. `g` and `G` behave identically here; `G`
        // typed *without* first entering count mode (handled in
        // `handle_key`, not here) jumps straight to the last line instead.
        KeyCode::Char('g') | KeyCode::Char('G') => {
            let n = take_count(state);
            state.cursor = (n - 1).min(state.lines.len().saturating_sub(1));
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

/// After expanding reveals new rows below the cursor, ratatui's `List` only
/// guarantees the *selected* row (the cursor's own, unmoved by an expand)
/// stays visible -- if that row was already the last visible one, the newly
/// revealed children land entirely off-screen with nothing forcing a
/// scroll. If the cursor was already at or past the bottom edge of the
/// last-rendered viewport, pin the scroll offset to the cursor so it
/// becomes the top-most visible row, showing up to a full screen of new
/// children immediately (issue #112). A no-op before the first frame has
/// rendered (`viewport_height` still `0`) or when the cursor is safely
/// inside the viewport already.
fn reveal_cursor_in_viewport(state: &AppState) {
    let height = state.viewport_height.get();
    if height == 0 {
        return;
    }
    let bottom = state.scroll_offset.get() + height.saturating_sub(1);
    if state.cursor >= bottom {
        state.scroll_offset.set(state.cursor);
    }
}

/// `l` (nvim-tree convention): expands the current node if it's a
/// collapsed container, or reveals the rest of a truncated array's preview.
/// A no-op on a leaf or an already-expanded container -- unlike `Tab`/
/// `Space`, this never collapses anything, so it's safe to mash.
fn expand_current(state: &mut AppState) {
    let Some(line) = state.lines.get(state.cursor) else {
        return;
    };
    let revealed = if line.is_array_summary {
        let array_path = array_path_for_summary_line(&line.path).to_vec();
        state.array_overrides.insert(array_path)
    } else if line.has_children {
        state.collapsed.remove(&line.path)
    } else {
        false
    };
    if revealed {
        reveal_cursor_in_viewport(state);
    }
}

/// `h` (nvim-tree convention): collapses the current node if it's an
/// expanded container; otherwise (a leaf, or an already-collapsed
/// container) jumps to the parent instead, without collapsing it -- unlike
/// `Backspace`'s `collapse_nearest_parent`, which deliberately collapses
/// the parent too.
fn collapse_current_or_jump_to_parent(state: &mut AppState) {
    let Some(line) = state.lines.get(state.cursor) else {
        return;
    };
    if line.has_children && !line.is_array_summary && !state.collapsed.contains(&line.path) {
        state.collapsed.insert(line.path.clone());
        // Resets the preview so re-expanding starts truncated again, same
        // as Tab/Space's collapse branch.
        state.array_overrides.remove(&line.path);
        return;
    }
    // Deliberately `line.path` (not `real_path`): for an array-summary
    // marker line, `line.path` already carries the synthetic trailing
    // segment, so stripping one level lands on the array's own line --
    // matching `collapse_nearest_parent`'s convention. Using `real_path`
    // here would strip an extra level and overshoot past the array itself.
    let path = &line.path;
    if path.len() < 2 {
        return;
    }
    let parent = path[..path.len() - 1].to_vec();
    move_cursor_to_path(state, &parent);
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
    let path = real_path(line).to_vec();
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

/// The real path a line represents, resolving the array-summary line's
/// synthetic marker segment to its actual (array) path -- the same
/// resolution every other path-consuming key (tag, yank, ...) applies.
fn real_path(line: &super::state::Line) -> &[String] {
    if line.is_array_summary {
        array_path_for_summary_line(&line.path)
    } else {
        &line.path
    }
}

fn collapse_all(state: &mut AppState) {
    // Collapsing everything means every array's expand-past-limit preview
    // becomes irrelevant too -- without this, re-expanding an array Tab
    // shows it fully expanded instead of reset to the truncated preview,
    // contradicting the invariant `recollapsing_an_array_clears_its_expand_override`
    // enforces for the single-node Tab/Space path.
    state.array_overrides.clear();
    let root = state
        .lines
        .get(state.cursor)
        .and_then(|l| real_path(l).first().cloned());
    state.collapsed = state.all_container_paths.clone();
    state.status_message = Some("collapsed all".to_string());
    // Every node below the top level is now hidden, so the cursor can only
    // meaningfully land on the root ancestor of wherever it was. This is a
    // *whole-document* collapse, so earlier siblings can also lose rows and
    // shift everything after them -- unlike `collapse_all_ancestors`, which
    // only ever touches descendants of the target and so can reposition the
    // cursor immediately, this must go through `pending_cursor_path` and
    // get re-resolved by path against the rebuilt list, not a stale index.
    if let Some(root) = root {
        state.pending_cursor_path = Some(vec![root]);
    }
}

fn expand_all(state: &mut AppState) {
    // `expand_all` only clears `collapsed`, never `array_overrides`, so a
    // still-truncated array's summary line survives completely unchanged --
    // anchor on the line's own path (marker segment included), not
    // `real_path`'s resolved container path, or the cursor would jump off a
    // summary line that never moved onto the array's own line instead.
    let anchor = state.lines.get(state.cursor).map(|l| l.path.clone());
    state.collapsed.clear();
    state.status_message = Some("expanded all".to_string());
    // Expanding only reveals more lines, so the node the cursor was on is
    // still there -- put the cursor back on it instead of leaving the
    // numeric index pointing at whatever now occupies that same slot. Newly
    // revealed rows earlier in the document can shift everything after
    // them, so (as in `collapse_all`) this must defer to
    // `pending_cursor_path` rather than resolve against the stale list.
    if let Some(path) = anchor {
        state.pending_cursor_path = Some(path);
    }
}

/// Inclusive line-index range of the active visual-line selection, clamped
/// to the current line count (which can shrink between the anchor being set
/// and a later collapse/expand elsewhere rebuilding `lines`).
fn visual_range(state: &AppState) -> (usize, usize) {
    let last = state.lines.len().saturating_sub(1);
    let anchor = state.visual_anchor.unwrap_or(state.cursor).min(last);
    let cursor = state.cursor.min(last);
    if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    }
}

/// `(real_path, has_children, is_array_summary)` for every line in the
/// active selection, snapshotted before any mutation -- the selected lines
/// stay identified by path even though the mutations below don't reorder or
/// resize `state.lines` (they only ever touch `collapsed`/`array_overrides`).
fn selected_lines(state: &AppState) -> Vec<(Vec<String>, bool, bool)> {
    if state.lines.is_empty() {
        return Vec::new();
    }
    let (lo, hi) = visual_range(state);
    state.lines[lo..=hi]
        .iter()
        .map(|l| (l.path.clone(), l.has_children, l.is_array_summary))
        .collect()
}

fn path_starts_with(path: &[String], prefix: &[String]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}

fn exit_visual_mode(state: &mut AppState) {
    state.visual_anchor = None;
}

/// `l` across the selection: expands every selected container one level (or
/// reveals a truncated array's preview), same rule as single-line `l`.
fn visual_expand(state: &mut AppState) {
    for (path, has_children, is_array_summary) in selected_lines(state) {
        if is_array_summary {
            state
                .array_overrides
                .insert(array_path_for_summary_line(&path).to_vec());
        } else if has_children {
            state.collapsed.remove(&path);
        }
    }
    exit_visual_mode(state);
}

/// `h` across the selection: collapses every selected container one level.
/// Unlike single-line `h`, never jumps to a parent -- that has no sensible
/// meaning for a multi-line selection.
fn visual_collapse(state: &mut AppState) {
    for (path, has_children, is_array_summary) in selected_lines(state) {
        if has_children && !is_array_summary {
            state.collapsed.insert(path.clone());
            state.array_overrides.remove(&path);
        }
    }
    exit_visual_mode(state);
}

/// `c` across the selection: collapses every descendant (and the selected
/// node itself) of each selected line, the same as global `collapse_all`
/// but scoped to the selection's subtrees.
fn visual_collapse_all(state: &mut AppState) {
    let roots: Vec<Vec<String>> = selected_lines(state)
        .into_iter()
        .map(|(path, _, is_array_summary)| {
            if is_array_summary {
                array_path_for_summary_line(&path).to_vec()
            } else {
                path
            }
        })
        .collect();
    let to_collapse: Vec<Vec<String>> = state
        .all_container_paths
        .iter()
        .filter(|p| roots.iter().any(|r| path_starts_with(p, r)))
        .cloned()
        .collect();
    for path in &to_collapse {
        state.array_overrides.remove(path);
    }
    state.collapsed.extend(to_collapse);
    if let Some(root) = roots.into_iter().next() {
        state.pending_cursor_path = Some(root);
    }
    exit_visual_mode(state);
    state.status_message = Some("collapsed selection".to_string());
}

/// `e` across the selection: expands every descendant of each selected
/// line, the same as global `expand_all` but scoped to the selection.
fn visual_expand_all(state: &mut AppState) {
    let roots: Vec<Vec<String>> = selected_lines(state)
        .into_iter()
        .map(|(path, _, is_array_summary)| {
            if is_array_summary {
                array_path_for_summary_line(&path).to_vec()
            } else {
                path
            }
        })
        .collect();
    let to_expand: Vec<Vec<String>> = state
        .collapsed
        .iter()
        .filter(|p| roots.iter().any(|r| path_starts_with(p, r)))
        .cloned()
        .collect();
    for path in to_expand {
        state.collapsed.remove(&path);
    }
    if let Some(root) = roots.into_iter().next() {
        state.pending_cursor_path = Some(root);
    }
    exit_visual_mode(state);
    state.status_message = Some("expanded selection".to_string());
}

/// Key handling while `state.visual_anchor.is_some()`. `q` still quits (same
/// precedent as the help/inspect overlays); `Esc`/`V` cancel the selection.
fn handle_visual_key(state: &mut AppState, key: KeyCode) -> bool {
    match key {
        KeyCode::Char('q') => return true,
        KeyCode::Esc | KeyCode::Char('V') => exit_visual_mode(state),
        KeyCode::Down | KeyCode::Char('j') => {
            state.cursor = (state.cursor + 1).min(state.lines.len().saturating_sub(1));
        }
        KeyCode::Up | KeyCode::Char('k') => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Char('l') => visual_expand(state),
        KeyCode::Char('h') => visual_collapse(state),
        KeyCode::Char('c') => visual_collapse_all(state),
        KeyCode::Char('e') => visual_expand_all(state),
        _ => {}
    }
    false
}

/// Last visible line index of the current viewport, clamped to the
/// document's own last line (the viewport can be taller than the document,
/// e.g. a short file in a tall terminal).
fn viewport_bottom(state: &AppState) -> usize {
    let last = state.lines.len().saturating_sub(1);
    let height = state.viewport_height.get();
    (state.scroll_offset.get() + height.saturating_sub(1)).min(last)
}

/// `H`: jump to the first visible line of the viewport.
fn jump_to_viewport_top(state: &mut AppState) {
    state.cursor = state
        .scroll_offset
        .get()
        .min(state.lines.len().saturating_sub(1));
}

/// `M`: jump to the middle visible line of the viewport.
fn jump_to_viewport_middle(state: &mut AppState) {
    let top = state.scroll_offset.get();
    let bottom = viewport_bottom(state);
    state.cursor = top + (bottom - top) / 2;
}

/// `L`: jump to the last visible line of the viewport.
fn jump_to_viewport_bottom(state: &mut AppState) {
    state.cursor = viewport_bottom(state);
}

/// PgUp/PgDn: move by one viewport height. Falls back to a single line when
/// the viewport height isn't known yet (before the first frame renders).
fn page_up(state: &mut AppState) {
    let step = state.viewport_height.get().max(1);
    state.cursor = state.cursor.saturating_sub(step);
}

fn page_down(state: &mut AppState) {
    let step = state.viewport_height.get().max(1);
    state.cursor = (state.cursor + step).min(state.lines.len().saturating_sub(1));
}

pub(super) fn handle_key(state: &mut AppState, key: KeyCode) -> bool {
    if state.searching {
        match key {
            KeyCode::Enter | KeyCode::Esc => state.searching = false,
            KeyCode::Backspace => {
                state.search.pop();
            }
            KeyCode::Tab => cycle_search_match(state, 1),
            KeyCode::BackTab => cycle_search_match(state, -1),
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
    if state.visual_anchor.is_some() {
        return handle_visual_key(state, key);
    }
    state.status_message = None;
    match key {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('V') if !state.lines.is_empty() => state.visual_anchor = Some(state.cursor),
        KeyCode::Enter if state.pick_mode => {
            if let Some(line) = state.lines.get(state.cursor) {
                let path = real_path(line);
                state.pick_result = Some(if state.is_json {
                    to_jq_path(path)
                } else {
                    path.join(".")
                });
            }
            return true;
        }
        KeyCode::Char('?') => state.help_visible = true,
        KeyCode::Char('g') => state.count_buffer = Some(String::new()),
        // Bare `G` (no preceding `g`) jumps straight to the last line;
        // `g<digits>G` (via apply_count_jump) jumps to an absolute line.
        KeyCode::Char('G') => state.cursor = state.lines.len().saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            state.cursor = (state.cursor + 1).min(state.lines.len().saturating_sub(1));
        }
        KeyCode::Up | KeyCode::Char('k') => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Char('l') => expand_current(state),
        KeyCode::Char('h') => collapse_current_or_jump_to_parent(state),
        KeyCode::Char('H') => jump_to_viewport_top(state),
        KeyCode::Char('M') => jump_to_viewport_middle(state),
        KeyCode::Char('L') => jump_to_viewport_bottom(state),
        KeyCode::PageUp => page_up(state),
        KeyCode::PageDown => page_down(state),
        KeyCode::Char('n') => cycle_search_match(state, 1),
        KeyCode::Char('N') => cycle_search_match(state, -1),
        KeyCode::Tab | KeyCode::Char(' ') => {
            if let Some(line) = state.lines.get(state.cursor) {
                if line.is_array_summary {
                    let array_path = array_path_for_summary_line(&line.path).to_vec();
                    if state.array_overrides.insert(array_path) {
                        reveal_cursor_in_viewport(state);
                    }
                } else if line.has_children {
                    if state.collapsed.remove(&line.path) {
                        // no-op: re-expanding doesn't touch array_overrides
                        reveal_cursor_in_viewport(state);
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
                let path = real_path(line);
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
                let path = real_path(line);
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
        KeyCode::Char('c') => collapse_all(state),
        KeyCode::Char('e') => expand_all(state),
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
            viewport_height: std::cell::Cell::new(0),
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
            pending_cursor_occurrence: 0,
            count_buffer: None,
            popup_visible: false,
            popup_query: String::new(),
            popup_selected: 0,
            inspect_visible: false,
            popup_scroll_offset: std::cell::Cell::new(0),
            pick_mode: false,
            pick_result: None,
            visual_anchor: None,
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
    fn j_and_k_are_vim_aliases_for_down_and_up() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('j'));
        assert_eq!(state.cursor, 1);
        handle_key(&mut state, KeyCode::Char('j'));
        assert_eq!(state.cursor, 2);
        handle_key(&mut state, KeyCode::Char('k'));
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn expanding_a_container_at_the_bottom_edge_of_the_viewport_pins_it_to_the_top() {
        let mut state = nested_fixture();
        state.cursor = 1; // "user", a collapsed container
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        // Viewport shows rows 0..=1, so the cursor (row 1) sits exactly at
        // the bottom edge -- any children revealed below it would be
        // entirely off-screen without a scroll.
        state.scroll_offset.set(0);
        state.viewport_height.set(2);
        handle_key(&mut state, KeyCode::Char('l'));
        assert_eq!(
            state.scroll_offset.get(),
            1,
            "must pin the expanded row to the top of the viewport"
        );
    }

    #[test]
    fn expanding_a_container_comfortably_inside_the_viewport_does_not_move_the_scroll_offset() {
        let mut state = nested_fixture();
        state.cursor = 1; // "user"
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        state.scroll_offset.set(0);
        state.viewport_height.set(10); // cursor is nowhere near the bottom
        handle_key(&mut state, KeyCode::Char('l'));
        assert_eq!(state.scroll_offset.get(), 0);
    }

    #[test]
    fn re_expanding_an_already_expanded_container_does_not_move_the_scroll_offset() {
        let mut state = nested_fixture();
        state.cursor = 1; // "user", already expanded (not in `collapsed`)
        state.scroll_offset.set(0);
        state.viewport_height.set(2); // cursor is at the bottom edge
        handle_key(&mut state, KeyCode::Char('l'));
        assert_eq!(
            state.scroll_offset.get(),
            0,
            "a no-op expand must not move the viewport"
        );
    }

    #[test]
    fn expanding_via_tab_at_the_bottom_edge_also_reveals_the_children() {
        let mut state = nested_fixture();
        state.cursor = 1; // "user"
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        state.scroll_offset.set(0);
        state.viewport_height.set(2);
        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(state.scroll_offset.get(), 1);
    }

    #[test]
    fn collapsing_via_tab_does_not_trigger_a_reveal() {
        let mut state = nested_fixture();
        state.cursor = 1; // "user", expanded
        state.scroll_offset.set(0);
        state.viewport_height.set(2); // at the bottom edge
        handle_key(&mut state, KeyCode::Tab); // collapses it
        assert!(
            state
                .collapsed
                .contains(&vec!["root".to_string(), "user".to_string()])
        );
        assert_eq!(
            state.scroll_offset.get(),
            0,
            "collapsing removes rows, never needs a reveal"
        );
    }

    #[test]
    fn expanding_an_array_summary_marker_at_the_bottom_edge_also_reveals_it() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        state.scroll_offset.set(2);
        state.viewport_height.set(2); // rows 2..=3 visible; cursor at the edge
        handle_key(&mut state, KeyCode::Char('l'));
        assert_eq!(state.scroll_offset.get(), 3);
    }

    #[test]
    fn expand_reveal_is_a_no_op_before_the_first_frame_has_rendered() {
        let mut state = nested_fixture();
        state.cursor = 1; // "user"
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        // viewport_height defaults to 0 before render_tree ever runs.
        handle_key(&mut state, KeyCode::Char('l'));
        assert_eq!(state.scroll_offset.get(), 0);
    }

    #[test]
    fn l_expands_a_collapsed_container_but_never_collapses_it() {
        let mut state = fixture();
        state.collapsed.insert(vec!["user".to_string()]);
        handle_key(&mut state, KeyCode::Char('l'));
        assert!(!state.collapsed.contains(&vec!["user".to_string()]));
        // Already expanded: `l` must not toggle it back closed.
        handle_key(&mut state, KeyCode::Char('l'));
        assert!(!state.collapsed.contains(&vec!["user".to_string()]));
    }

    #[test]
    fn l_on_a_leaf_line_does_nothing() {
        let mut state = fixture();
        state.cursor = 1; // "name" has no children
        handle_key(&mut state, KeyCode::Char('l'));
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn l_on_an_array_truncation_marker_expands_the_array_via_override() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        handle_key(&mut state, KeyCode::Char('l'));
        assert!(
            state
                .array_overrides
                .contains(&vec!["user".to_string(), "age".to_string()])
        );
    }

    #[test]
    fn h_collapses_an_expanded_container_but_never_expands_it() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('h'));
        assert!(state.collapsed.contains(&vec!["user".to_string()]));
        assert_eq!(
            state.cursor, 0,
            "collapsing in place must not move the cursor"
        );
    }

    #[test]
    fn h_on_a_leaf_jumps_to_its_parent_without_collapsing_anything() {
        let mut state = fixture();
        state.cursor = 1; // "name", child of "user"
        handle_key(&mut state, KeyCode::Char('h'));
        assert_eq!(state.cursor, 0, "must land on \"user\"");
        assert!(
            state.collapsed.is_empty(),
            "unlike Backspace, h must not collapse the parent it jumps to"
        );
    }

    #[test]
    fn h_on_the_array_truncation_marker_jumps_to_the_arrays_own_line_not_past_it() {
        let mut state = fixture();
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        handle_key(&mut state, KeyCode::Char('h'));
        assert_eq!(
            state.cursor, 2,
            "must land on the array's own line (\"age\"), not overshoot to its parent (\"user\")"
        );
    }

    #[test]
    fn count_prefix_works_with_the_vim_j_k_aliases_too() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('2'));
        handle_key(&mut state, KeyCode::Char('j'));
        assert_eq!(
            state.cursor, 2,
            "g2j must move down 2 lines, same as g2\u{2193}"
        );
    }

    #[test]
    fn h_on_an_already_collapsed_container_jumps_to_its_parent() {
        let mut state = fixture();
        state.lines = vec![
            line("root", true, &["root"]),
            line("user", true, &["root", "user"]),
        ];
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        state.cursor = 1;
        handle_key(&mut state, KeyCode::Char('h'));
        assert_eq!(state.cursor, 0, "must land on the parent \"root\"");
    }

    #[test]
    fn h_at_the_root_does_nothing() {
        let mut state = fixture();
        state.cursor = 1; // "name"
        state.lines[0].has_children = false; // pretend "user" has no children either
        // Simulate already being at a top-level leaf with no parent.
        state.lines[1].path = vec!["name".to_string()];
        state.cursor = 1;
        handle_key(&mut state, KeyCode::Char('h'));
        assert_eq!(state.cursor, 1, "a top-level node has no parent to jump to");
    }

    #[test]
    fn bare_shift_g_jumps_straight_to_the_last_line() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('G'));
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn gg_jumps_to_the_first_line() {
        let mut state = fixture();
        state.cursor = 2;
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('g'));
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn g_digits_g_jumps_to_the_absolute_one_based_line() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('2'));
        handle_key(&mut state, KeyCode::Char('g'));
        assert_eq!(state.cursor, 1, "line 2 (1-based) is index 1");
    }

    #[test]
    fn g_digits_shift_g_also_jumps_to_the_absolute_line() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('2'));
        handle_key(&mut state, KeyCode::Char('G'));
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn g_digits_g_clamps_past_the_last_line() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('g'));
        handle_key(&mut state, KeyCode::Char('9'));
        handle_key(&mut state, KeyCode::Char('g'));
        assert_eq!(state.cursor, 2, "must clamp to the last line, not panic");
    }

    #[test]
    fn n_and_shift_n_repeat_the_last_search_forward_and_backward() {
        // All three fixture paths ("user", "user.name", "user.age") contain
        // "user", so with the cursor sitting on "user" (match index 0),
        // forward wraps to the next match and backward wraps to the last.
        let mut state = fixture();
        state.search = "user".to_string();
        handle_key(&mut state, KeyCode::Char('n'));
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["user".to_string(), "name".to_string()].as_slice())
        );
        handle_key(&mut state, KeyCode::Char('N'));
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["user".to_string(), "age".to_string()].as_slice())
        );
    }

    #[test]
    fn n_does_nothing_without_a_prior_search() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('n'));
        assert!(state.pending_cursor_path.is_none());
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
            viewport_height: std::cell::Cell::new(0),
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
            pending_cursor_occurrence: 0,
            count_buffer: None,
            popup_visible: false,
            popup_query: String::new(),
            popup_selected: 0,
            inspect_visible: false,
            popup_scroll_offset: std::cell::Cell::new(0),
            pick_mode: false,
            pick_result: None,
            visual_anchor: None,
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
            viewport_height: std::cell::Cell::new(0),
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
            visual_anchor: None,
        }
    }

    #[test]
    fn viewport_nav_keys_do_nothing_on_an_empty_document_instead_of_panicking() {
        let mut state = tall_fixture(0, 0);
        state.viewport_height.set(10);
        for key in [
            KeyCode::Char('H'),
            KeyCode::Char('M'),
            KeyCode::Char('L'),
            KeyCode::PageUp,
            KeyCode::PageDown,
        ] {
            handle_key(&mut state, key);
            assert_eq!(state.cursor, 0);
        }
    }

    #[test]
    fn shift_h_jumps_to_the_top_of_the_viewport() {
        let mut state = tall_fixture(50, 25);
        state.scroll_offset.set(20);
        state.viewport_height.set(10);
        handle_key(&mut state, KeyCode::Char('H'));
        assert_eq!(state.cursor, 20);
    }

    #[test]
    fn shift_l_jumps_to_the_bottom_of_the_viewport() {
        let mut state = tall_fixture(50, 25);
        state.scroll_offset.set(20);
        state.viewport_height.set(10);
        handle_key(&mut state, KeyCode::Char('L'));
        assert_eq!(state.cursor, 29, "20 + (10 - 1)");
    }

    #[test]
    fn shift_l_clamps_to_the_last_line_when_the_viewport_extends_past_the_document() {
        let mut state = tall_fixture(5, 0);
        state.scroll_offset.set(0);
        state.viewport_height.set(20); // taller than the whole document
        handle_key(&mut state, KeyCode::Char('L'));
        assert_eq!(state.cursor, 4, "must clamp to the last real line");
    }

    #[test]
    fn shift_m_jumps_to_the_middle_of_the_viewport() {
        let mut state = tall_fixture(50, 25);
        state.scroll_offset.set(20);
        state.viewport_height.set(10);
        handle_key(&mut state, KeyCode::Char('M'));
        assert_eq!(state.cursor, 24, "midpoint of visible rows 20..=29");
    }

    #[test]
    fn page_down_moves_the_cursor_by_one_viewport_height() {
        let mut state = tall_fixture(50, 5);
        state.viewport_height.set(10);
        handle_key(&mut state, KeyCode::PageDown);
        assert_eq!(state.cursor, 15);
    }

    #[test]
    fn page_down_clamps_to_the_last_line() {
        let mut state = tall_fixture(20, 15);
        state.viewport_height.set(10);
        handle_key(&mut state, KeyCode::PageDown);
        assert_eq!(state.cursor, 19);
    }

    #[test]
    fn page_up_moves_the_cursor_by_one_viewport_height() {
        let mut state = tall_fixture(50, 25);
        state.viewport_height.set(10);
        handle_key(&mut state, KeyCode::PageUp);
        assert_eq!(state.cursor, 15);
    }

    #[test]
    fn page_up_clamps_to_the_first_line_instead_of_underflowing() {
        let mut state = tall_fixture(50, 5);
        state.viewport_height.set(10);
        handle_key(&mut state, KeyCode::PageUp);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn page_up_and_down_fall_back_to_a_single_line_before_the_first_frame_renders() {
        let mut state = tall_fixture(50, 10);
        // viewport_height defaults to 0 before render_tree ever runs.
        handle_key(&mut state, KeyCode::PageDown);
        assert_eq!(state.cursor, 11);
        handle_key(&mut state, KeyCode::PageUp);
        handle_key(&mut state, KeyCode::PageUp);
        assert_eq!(state.cursor, 9);
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
    fn tab_while_searching_cycles_to_the_next_match_instead_of_toggling_collapse() {
        let mut state = fixture();
        state.searching = true;
        state.search = "user".to_string();
        let quit = handle_key(&mut state, KeyCode::Tab);
        assert!(!quit);
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["user".to_string(), "name".to_string()].as_slice())
        );
    }

    #[test]
    fn backtab_while_searching_cycles_to_the_previous_match_and_wraps() {
        let mut state = fixture();
        state.searching = true;
        state.search = "user".to_string();
        // cursor starts on the first match ("user"); backward wraps to the last.
        handle_key(&mut state, KeyCode::BackTab);
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["user".to_string(), "age".to_string()].as_slice())
        );
    }

    #[test]
    fn cycle_search_match_does_nothing_for_empty_search() {
        let mut state = fixture();
        cycle_search_match(&mut state, 1);
        assert!(state.pending_cursor_path.is_none());
    }

    #[test]
    fn cycle_search_match_does_nothing_when_there_are_no_matches() {
        let mut state = fixture();
        state.search = "zzz".to_string();
        cycle_search_match(&mut state, 1);
        assert!(state.pending_cursor_path.is_none());
    }

    #[test]
    fn cycle_search_match_jumps_to_the_first_match_when_the_cursor_is_not_on_one() {
        let mut state = fixture();
        // cursor sits on "user" (line 0), which "name" alone does not match.
        state.search = "name".to_string();
        cycle_search_match(&mut state, 1);
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["user".to_string(), "name".to_string()].as_slice())
        );
    }

    #[test]
    fn cycle_search_match_expands_a_collapsed_ancestor_to_reach_a_hidden_match() {
        let mut state = fixture();
        state.collapsed.insert(vec!["user".to_string()]);
        state.search = "age".to_string();
        cycle_search_match(&mut state, 1);
        assert!(
            !state.collapsed.contains(&vec!["user".to_string()]),
            "the collapsed ancestor must be expanded to reach the match"
        );
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["user".to_string(), "age".to_string()].as_slice())
        );
    }

    /// Three lines sharing the exact same path -- e.g. `<item>1</item>
    /// <item>2</item><item>3</item>` in XML, where sibling elements with
    /// the same tag name have no `[N]`-style disambiguation the way a JSON
    /// array does.
    fn duplicate_path_state() -> AppState {
        let mut state = fixture();
        state.lines = (1..=3)
            .map(|i| Line {
                depth: 0,
                key: "item".to_string(),
                value: Some((i.to_string(), TqColor::Number)),
                path: vec!["item".to_string()],
                has_children: false,
                is_array_summary: false,
                type_label: "number".to_string(),
            })
            .collect();
        state.all_paths = (1..=3)
            .map(|i: i32| (vec!["item".to_string()], format!("item: {i}")))
            .collect();
        state.cursor = 0;
        state
    }

    #[test]
    fn cycle_search_match_advances_through_lines_sharing_the_same_path_instead_of_getting_stuck() {
        let mut state = duplicate_path_state();
        state.searching = true;
        state.search = "item".to_string();

        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(
            (
                state.pending_cursor_path.clone(),
                state.pending_cursor_occurrence
            ),
            (Some(vec!["item".to_string()]), 1),
            "must advance to the 2nd occurrence, not stay on the 1st"
        );
        // Simulate what `rebuild_json_lines`'s `apply_pending_cursor_path`
        // does once `lines` rebuilds, without disturbing this test's
        // hand-built `lines` with a real (empty) document rebuild.
        let path = state.pending_cursor_path.take().unwrap();
        let occurrence = std::mem::take(&mut state.pending_cursor_occurrence);
        move_cursor_to_nth_path(&mut state, &path, occurrence);
        assert_eq!(state.cursor, 1, "cursor must land on the 2nd \"item\" line");

        handle_key(&mut state, KeyCode::Tab);
        assert_eq!(
            state.pending_cursor_occurrence, 2,
            "must advance to the 3rd occurrence"
        );
    }

    #[test]
    fn cycle_search_match_wraps_backward_across_lines_sharing_the_same_path() {
        let mut state = duplicate_path_state();
        state.searching = true;
        state.search = "item".to_string();

        handle_key(&mut state, KeyCode::BackTab);
        assert_eq!(
            (
                state.pending_cursor_path.clone(),
                state.pending_cursor_occurrence
            ),
            (Some(vec!["item".to_string()]), 2),
            "backward from the 1st occurrence must wrap to the last (3rd)"
        );
    }

    #[test]
    fn move_cursor_to_nth_path_lands_on_the_requested_occurrence() {
        let mut state = duplicate_path_state();
        move_cursor_to_nth_path(&mut state, &["item".to_string()], 2);
        assert_eq!(state.cursor, 2);
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

    fn three_siblings_state() -> (AppState, crate::json_tree::JsonNode) {
        use super::super::flatten::{collect_all_paths_json, flatten_json};
        use crate::json_tree::JsonNode;

        let value = serde_json::json!({"a": {"x": 1}, "b": {"y": 1}, "c": {"z": 1}});
        let node = JsonNode::from_value(&value);
        let mut all_paths = Vec::new();
        collect_all_paths_json(&node, &[], &mut all_paths);
        let mut lines = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &HashSet::new(), &mut lines);
        let state = AppState {
            lines,
            collapsed: HashSet::new(),
            all_container_paths: HashSet::from([
                vec!["a".to_string()],
                vec!["b".to_string()],
                vec!["c".to_string()],
            ]),
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
            viewport_height: std::cell::Cell::new(0),
            all_paths,
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
            visual_anchor: None,
        };
        (state, node)
    }

    #[test]
    fn collapse_all_repositions_the_cursor_to_the_root_ancestor_not_an_unrelated_node() {
        use super::super::state::rebuild_json_lines;
        let (mut state, node) = three_siblings_state();
        // Flat order is [a, x, b, y, c, z]; put the cursor on "y" (child of "b").
        move_cursor_to_path(&mut state, &["b".to_string(), "y".to_string()]);
        assert_eq!(state.lines[state.cursor].key, "y");

        handle_key(&mut state, KeyCode::Char('c'));
        rebuild_json_lines(&mut state, &node);

        assert_eq!(
            state.lines[state.cursor].path,
            vec!["b".to_string()],
            "cursor must land on the ancestor of the node it was on, not clamp to \
             whatever numeric index happens to survive"
        );
    }

    #[test]
    fn expand_all_repositions_the_cursor_back_onto_the_same_node() {
        use super::super::state::rebuild_json_lines;
        let (mut state, node) = three_siblings_state();
        state.collapsed.insert(vec!["a".to_string()]);
        rebuild_json_lines(&mut state, &node);
        // Flat order is now [a, b, y, c, z]; put the cursor on "b".
        move_cursor_to_path(&mut state, &["b".to_string()]);
        assert_eq!(state.lines[state.cursor].key, "b");

        handle_key(&mut state, KeyCode::Char('e'));
        rebuild_json_lines(&mut state, &node);

        assert_eq!(
            state.lines[state.cursor].path,
            vec!["b".to_string()],
            "cursor must stay on the same logical node once newly-revealed rows \
             shift its numeric index"
        );
    }

    #[test]
    fn expand_all_leaves_the_cursor_on_a_still_truncated_arrays_summary_line() {
        use super::super::flatten::flatten_json;
        use super::super::state::rebuild_json_lines;
        use crate::json_tree::JsonNode;

        // expand_all() only clears `collapsed`, never `array_overrides`, so
        // an over-the-preview-limit array's summary line survives it
        // completely unchanged -- the cursor must stay right there, not
        // jump to the array's own container line.
        let items: Vec<serde_json::Value> = (0..205).map(|_| serde_json::json!("x")).collect();
        let value = serde_json::json!({ "items": items });
        let node = JsonNode::from_value(&value);
        let mut lines = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &HashSet::new(), &mut lines);
        let mut state = AppState {
            lines,
            collapsed: HashSet::new(),
            all_container_paths: HashSet::from([vec!["items".to_string()]]),
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
            viewport_height: std::cell::Cell::new(0),
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
            visual_anchor: None,
        };
        move_cursor_to_path(&mut state, &["items".to_string(), "…more".to_string()]);
        assert!(state.lines[state.cursor].is_array_summary);

        handle_key(&mut state, KeyCode::Char('e'));
        rebuild_json_lines(&mut state, &node);

        assert_eq!(
            state.lines[state.cursor].path,
            vec!["items".to_string(), "…more".to_string()],
            "the summary line never moved, so the cursor shouldn't move off it either"
        );
    }

    #[test]
    fn collapse_all_clears_array_overrides_so_a_recollapsed_array_resets_to_preview() {
        let mut state = fixture();
        state.array_overrides.insert(vec!["user".to_string()]);
        handle_key(&mut state, KeyCode::Char('c'));
        assert!(
            state.array_overrides.is_empty(),
            "collapsing everything must reset every array's expand-past-limit \
             preview, the same invariant Tab/Space already enforces for a single node"
        );
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
    fn fuzzy_match_char_indices_finds_the_greedy_leftmost_positions() {
        assert_eq!(
            fuzzy_match_char_indices("user.name", "usnm"),
            Some(vec![0, 1, 5, 7])
        );
    }

    #[test]
    fn fuzzy_match_char_indices_is_case_insensitive() {
        assert_eq!(
            fuzzy_match_char_indices("USER", "user"),
            Some(vec![0, 1, 2, 3])
        );
    }

    #[test]
    fn fuzzy_match_char_indices_returns_none_when_a_pattern_char_is_missing() {
        assert_eq!(fuzzy_match_char_indices("user", "usnm"), None);
    }

    #[test]
    fn fuzzy_match_char_indices_returns_none_when_order_is_violated() {
        assert_eq!(fuzzy_match_char_indices("abc", "cab"), None);
    }

    #[test]
    fn fuzzy_match_char_indices_of_an_empty_pattern_matches_nothing_at_all() {
        assert_eq!(fuzzy_match_char_indices("anything", ""), Some(vec![]));
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
            viewport_height: std::cell::Cell::new(0),
            all_paths,
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
            visual_anchor: None,
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
    fn enter_in_pick_mode_on_json_prints_a_jq_path_and_quits() {
        let mut state = fixture();
        state.pick_mode = true;
        state.cursor = 1; // "user.name"
        assert!(handle_key(&mut state, KeyCode::Enter));
        assert_eq!(state.pick_result.as_deref(), Some(".user.name"));
    }

    #[test]
    fn enter_in_pick_mode_on_xml_prints_the_dotted_path_and_quits() {
        let mut state = fixture();
        state.pick_mode = true;
        state.is_json = false;
        state.cursor = 1;
        assert!(handle_key(&mut state, KeyCode::Enter));
        assert_eq!(state.pick_result.as_deref(), Some("user.name"));
    }

    #[test]
    fn enter_in_pick_mode_on_the_array_summary_line_picks_the_arrays_real_path() {
        let mut state = fixture();
        state.pick_mode = true;
        state
            .lines
            .push(array_summary_line(&["user", "age", "…more"]));
        state.cursor = 3;
        assert!(handle_key(&mut state, KeyCode::Enter));
        assert_eq!(state.pick_result.as_deref(), Some(".user.age"));
    }

    #[test]
    fn enter_without_pick_mode_does_nothing() {
        let mut state = fixture();
        assert!(!handle_key(&mut state, KeyCode::Enter));
        assert!(state.pick_result.is_none());
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

    #[test]
    fn shift_v_does_nothing_on_an_empty_document_instead_of_panicking() {
        let mut state = tall_fixture(0, 0);
        handle_key(&mut state, KeyCode::Char('V'));
        assert!(
            state.visual_anchor.is_none(),
            "must not enter visual mode with no lines to select"
        );
    }

    #[test]
    fn visual_actions_do_not_panic_if_lines_become_empty_while_a_stale_anchor_is_set() {
        let mut state = tall_fixture(0, 0);
        state.visual_anchor = Some(0);
        for key in [
            KeyCode::Char('l'),
            KeyCode::Char('h'),
            KeyCode::Char('c'),
            KeyCode::Char('e'),
        ] {
            state.visual_anchor = Some(0);
            handle_key(&mut state, key);
        }
    }

    #[test]
    fn shift_v_enters_visual_mode_anchored_at_the_cursor() {
        let mut state = fixture();
        state.cursor = 1;
        handle_key(&mut state, KeyCode::Char('V'));
        assert_eq!(state.visual_anchor, Some(1));
    }

    #[test]
    fn j_and_k_extend_the_selection_without_leaving_visual_mode() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('j'));
        assert!(state.visual_anchor.is_some(), "still in visual mode");
        assert_eq!(state.cursor, 1);
        assert_eq!(visual_range(&state), (0, 1));
    }

    #[test]
    fn esc_cancels_visual_mode_without_any_changes() {
        let mut state = nested_fixture();
        state.cursor = 0;
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('j'));
        handle_key(&mut state, KeyCode::Esc);
        assert!(state.visual_anchor.is_none());
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn shift_v_again_also_cancels_visual_mode() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('V'));
        assert!(state.visual_anchor.is_none());
    }

    #[test]
    fn q_still_quits_from_visual_mode() {
        let mut state = fixture();
        handle_key(&mut state, KeyCode::Char('V'));
        assert!(handle_key(&mut state, KeyCode::Char('q')));
    }

    #[test]
    fn visual_l_expands_every_selected_container_and_exits_visual_mode() {
        let mut state = nested_fixture();
        state
            .collapsed
            .insert(vec!["root".to_string(), "user".to_string()]);
        state.collapsed.insert(vec![
            "root".to_string(),
            "user".to_string(),
            "address".to_string(),
        ]);
        state.cursor = 0;
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('j'));
        handle_key(&mut state, KeyCode::Char('j'));
        handle_key(&mut state, KeyCode::Char('l'));
        assert!(state.visual_anchor.is_none(), "must auto-exit visual mode");
        assert!(
            !state
                .collapsed
                .contains(&vec!["root".to_string(), "user".to_string()])
        );
        assert!(!state.collapsed.contains(&vec![
            "root".to_string(),
            "user".to_string(),
            "address".to_string()
        ]));
    }

    #[test]
    fn visual_h_collapses_every_selected_container_and_exits_visual_mode() {
        let mut state = nested_fixture();
        state.cursor = 0;
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('j'));
        handle_key(&mut state, KeyCode::Char('j'));
        handle_key(&mut state, KeyCode::Char('h'));
        assert!(state.visual_anchor.is_none());
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
    }

    #[test]
    fn visual_h_never_jumps_to_a_parent_unlike_single_line_h() {
        let mut state = nested_fixture();
        state.cursor = 3; // "city", a leaf
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('h'));
        assert_eq!(
            state.cursor, 3,
            "unlike single-line h, must not jump anywhere"
        );
        assert!(state.collapsed.is_empty());
    }

    #[test]
    fn visual_c_collapses_every_descendant_of_the_selected_lines() {
        let mut state = nested_fixture();
        state.cursor = 0; // just "root" selected
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('c'));
        assert!(state.visual_anchor.is_none());
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
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["root".to_string()].as_slice())
        );
    }

    #[test]
    fn visual_e_expands_every_descendant_of_the_selected_lines() {
        let mut state = nested_fixture();
        state.collapsed = state.all_container_paths.clone();
        state.cursor = 0; // just "root" selected
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('e'));
        assert!(state.visual_anchor.is_none());
        assert!(state.collapsed.is_empty());
        assert_eq!(
            state.pending_cursor_path.as_deref(),
            Some(vec!["root".to_string()].as_slice())
        );
    }

    #[test]
    fn selection_direction_does_not_matter() {
        let mut state = nested_fixture();
        state.cursor = 2; // "address"
        handle_key(&mut state, KeyCode::Char('V'));
        handle_key(&mut state, KeyCode::Char('k'));
        handle_key(&mut state, KeyCode::Char('k'));
        assert_eq!(state.cursor, 0);
        assert_eq!(visual_range(&state), (0, 2));
        handle_key(&mut state, KeyCode::Char('h'));
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
    }
}
