use super::state::AppState;
use crate::clipboard::copy_to_clipboard;
use crossterm::event::KeyCode;

pub(super) fn jump_to_next_match(state: &mut AppState) {
    if state.search.is_empty() {
        return;
    }
    let n = state.lines.len();
    for offset in 1..=n {
        let idx = (state.cursor + offset) % n;
        if state.lines[idx]
            .key
            .to_lowercase()
            .contains(&state.search.to_lowercase())
        {
            state.cursor = idx;
            return;
        }
    }
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
    state.status_message = None;
    match key {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Down => state.cursor = (state.cursor + 1).min(state.lines.len().saturating_sub(1)),
        KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Enter | KeyCode::Char(' ') => {
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
