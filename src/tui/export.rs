//! `w`: write the node under the cursor to a file. A one-line prompt (like
//! `:`) with a suggested name; an existing file needs a second Enter.
//! Output is pretty JSON for every tree-shaped input (JSON, YAML, TOML,
//! NDJSON, JSONC) and the element's XML for XML; there is no size cap.

use super::keys::{json_at, real_path, xml_text_at};
use super::state::{AppState, Source};
use crossterm::event::KeyCode;
use std::path::Path;

pub(super) struct ExportPrompt {
    name: String,
    confirm_overwrite: bool,
}

impl ExportPrompt {
    pub(super) fn prompt_text(&self) -> String {
        let hint = if self.confirm_overwrite {
            "  (exists: Enter overwrites)"
        } else {
            ""
        };
        format!("write to: {}{hint}", self.name)
    }
}

/// `(file contents, extension)` for the node under the cursor.
fn content_at_cursor(state: &AppState) -> Option<(String, &'static str)> {
    let path = real_path(state.lines.get(state.cursor)?);
    match state.source {
        Source::Json(root) => {
            let compact = crate::json_tree::to_compact_json(json_at(root, path)?);
            let value: serde_json::Value = serde_json::from_str(&compact).ok()?;
            Some((serde_json::to_string_pretty(&value).ok()? + "\n", "json"))
        }
        Source::Xml(root) => {
            let is_attr = path.last().is_some_and(|s| s.starts_with('@'));
            let ext = if is_attr { "txt" } else { "xml" };
            Some((xml_text_at(root, path)? + "\n", ext))
        }
        Source::None => None,
    }
}

/// `tags.json`, `book[1].xml`, `id.txt`: the node's last path segment.
pub(super) fn suggested_name(path: &[String], ext: &str) -> String {
    let last = path.last().map_or("export", String::as_str);
    let stem: String = last
        .trim_start_matches('@')
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.{ext}", if stem.is_empty() { "export" } else { &stem })
}

pub(super) fn open(state: &mut AppState) {
    if state.pick_mode {
        state.status_message = Some("write: not available in pick mode".to_string());
        return;
    }
    let Some((_, ext)) = content_at_cursor(state) else {
        state.status_message = Some("nothing to write".to_string());
        return;
    };
    let path = real_path(&state.lines[state.cursor]);
    state.export = Some(ExportPrompt {
        name: suggested_name(path, ext),
        confirm_overwrite: false,
    });
}

pub(super) fn handle(state: &mut AppState, key: KeyCode) -> bool {
    let Some(prompt) = state.export.as_mut() else {
        return false;
    };
    match key {
        KeyCode::Esc => state.export = None,
        KeyCode::Enter => commit(state),
        KeyCode::Backspace => {
            prompt.name.pop();
            prompt.confirm_overwrite = false;
        }
        KeyCode::Char(c) => {
            prompt.name.push(c);
            prompt.confirm_overwrite = false;
        }
        _ => {}
    }
    false
}

fn commit(state: &mut AppState) {
    let Some(prompt) = state.export.as_mut() else {
        return;
    };
    let name = prompt.name.trim().to_string();
    if name.is_empty() {
        state.export = None;
        state.status_message = Some("write cancelled: no file name".to_string());
        return;
    }
    if Path::new(&name).exists() && !prompt.confirm_overwrite {
        prompt.confirm_overwrite = true;
        return;
    }
    state.export = None;
    state.status_message = Some(match content_at_cursor(state) {
        None => "nothing to write".to_string(),
        Some((text, _)) => match std::fs::write(&name, &text) {
            Ok(()) => format!("wrote {} bytes to {name}", text.len()),
            Err(e) => format!("write failed: {e}"),
        },
    });
}
