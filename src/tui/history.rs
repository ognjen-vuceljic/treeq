//! Jump history (`[` / `]`, `Ctrl+o` for back): a bounded back/forward stack of
//! node paths, recorded when a key moves the cursor far (search, `:`, `F`
//! popup, `gg`/`G`/`g<n>`), not on single-line steps. `Ctrl+i` would be Tab in
//! a terminal, so forward is `]`.

use super::state::{AppState, ensure_all_paths};
use crossterm::event::{KeyCode, KeyEvent};

const MAX_HISTORY: usize = 100;

#[derive(Default)]
pub(super) struct JumpHistory {
    back: Vec<Vec<String>>,
    forward: Vec<Vec<String>>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Dir {
    Back,
    Forward,
}

/// Keys that can jump the cursor; compared before/after to filter out no-ops.
fn is_jump_key(state: &AppState, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('g' | 'G' | 'n' | 'N') | KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab => {
            true
        }
        KeyCode::Down | KeyCode::Up => state.count_buffer.is_some(),
        _ => false,
    }
}

/// Where the cursor is (or is about to land, for an unresolved search target).
fn here(state: &AppState) -> Option<Vec<String>> {
    state
        .pending_cursor_path
        .clone()
        .or_else(|| state.lines.get(state.cursor).map(|l| l.path.clone()))
}

/// Call with the result of `before` and after the key was handled.
pub(super) fn before(state: &AppState, key: &KeyEvent) -> Option<Vec<String>> {
    is_jump_key(state, key).then(|| here(state)).flatten()
}

pub(super) fn record(state: &mut AppState, from: Vec<String>) {
    if here(state).is_none_or(|now| now == from) {
        return;
    }
    let h = &mut state.history;
    if h.back.last() != Some(&from) {
        h.back.push(from);
        if h.back.len() > MAX_HISTORY {
            h.back.remove(0);
        }
    }
    h.forward.clear();
}

pub(super) fn step(state: &mut AppState, dir: Dir) {
    let popped = match dir {
        Dir::Back => state.history.back.pop(),
        Dir::Forward => state.history.forward.pop(),
    };
    let Some(target) = popped else {
        state.status_message = Some(match dir {
            Dir::Back => "no older jump".to_string(),
            Dir::Forward => "no newer jump".to_string(),
        });
        return;
    };
    ensure_all_paths(state);
    if !state.all_paths.iter().any(|(p, _)| *p == target) {
        state.status_message = Some("jump target no longer exists".to_string());
        return;
    }
    if let Some(now) = here(state) {
        let other = match dir {
            Dir::Back => &mut state.history.forward,
            Dir::Forward => &mut state.history.back,
        };
        other.push(now);
    }
    reveal(state, &target);
    state.status_message = Some(format!(
        "{}: {}",
        if dir == Dir::Back { "back" } else { "forward" },
        target.join(".")
    ));
    state.pending_cursor_path = Some(target);
}

/// Expands every ancestor so the target line exists after the rebuild.
fn reveal(state: &mut AppState, target: &[String]) {
    for end in 1..target.len() {
        let ancestor = target[..end].to_vec();
        state.collapsed.remove(&ancestor);
        state.array_overrides.insert(ancestor);
    }
}
