//! `t`: show the array under the cursor as a table (one row per element,
//! one column per key, in order of first appearance). Only arrays whose
//! elements are all objects qualify. Cells are built for the visible rows
//! only, so a 100k-row array opens instantly.

use super::keys::{json_at, real_path};
use super::state::{AppState, Source};
use crate::json_tree::{JsonNode, is_unsafe_display_char, to_compact_json};
use crossterm::event::KeyCode;
use std::cell::Cell;

const MAX_COL_WIDTH: usize = 24;
const SEPARATOR: &str = " │ ";

pub(super) struct TableView {
    pub(super) path: Vec<String>,
    pub(super) columns: Vec<String>,
    pub(super) rows: usize,
    pub(super) scroll: Cell<usize>,
    pub(super) viewport: Cell<usize>,
}

/// Union of the objects' keys in first-seen order; `None` unless the array
/// is non-empty and every element is an object.
pub(super) fn columns_of(items: &[JsonNode]) -> Option<Vec<String>> {
    if items.is_empty() {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut columns = Vec::new();
    for item in items {
        let JsonNode::Object(fields) = item else {
            return None;
        };
        for (key, _) in fields {
            if seen.insert(key.as_str()) {
                columns.push(key.clone());
            }
        }
    }
    Some(columns)
}

/// Strings show quoted, nested values as compact JSON, missing keys blank.
pub(super) fn cell_text(row: &JsonNode, column: &str) -> String {
    let JsonNode::Object(fields) = row else {
        return String::new();
    };
    match fields.iter().find(|(k, _)| k == column).map(|(_, v)| v) {
        None => String::new(),
        Some(JsonNode::Scalar(s)) => s.display(),
        Some(nested) => defang(&to_compact_json(nested)),
    }
}

/// Terminal-unsafe characters (C1, bidi, ...) shown as `\\uXXXX`; quotes stay
/// as they are, since this is already JSON text.
fn defang(s: &str) -> String {
    s.chars()
        .map(|c| {
            if is_unsafe_display_char(c) {
                format!("\\u{:04x}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub(super) fn fit(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len <= width {
        return format!("{text:<width$}");
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Column widths from the header and the visible cells (capped), and how
/// many columns fit in `avail` characters, always at least one.
pub(super) fn layout(
    columns: &[String],
    cells: &[Vec<String>],
    index_width: usize,
    avail: usize,
) -> (Vec<usize>, usize) {
    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let widest = cells
                .iter()
                .map(|r| r[i].chars().count())
                .max()
                .unwrap_or(0);
            c.chars().count().max(widest).clamp(1, MAX_COL_WIDTH)
        })
        .collect();
    let mut used = index_width;
    let mut shown = 0;
    for w in &widths {
        used += SEPARATOR.chars().count() + w;
        if used > avail && shown > 0 {
            break;
        }
        shown += 1;
    }
    (widths, shown)
}

/// Header, rule and the visible rows as plain strings.
pub(super) fn render_rows(
    view: &TableView,
    items: &[JsonNode],
    first: usize,
    height: usize,
    avail: usize,
) -> (Vec<String>, usize) {
    let end = (first + height).min(items.len());
    let cells: Vec<Vec<String>> = items[first..end]
        .iter()
        .map(|row| view.columns.iter().map(|c| cell_text(row, c)).collect())
        .collect();
    let index_width = items.len().to_string().len().max(1);
    let (widths, shown) = layout(&view.columns, &cells, index_width, avail);
    let join = |index: String, parts: Vec<String>| {
        let body: Vec<String> = parts
            .iter()
            .zip(&widths)
            .take(shown)
            .map(|(p, w)| fit(p, *w))
            .collect();
        format!("{index:>index_width$}{SEPARATOR}{}", body.join(SEPARATOR))
    };
    let mut lines = vec![join("#".into(), view.columns.clone())];
    let rule_len = lines[0].chars().count();
    lines.push("─".repeat(rule_len));
    for (offset, row) in cells.into_iter().enumerate() {
        lines.push(join((first + offset).to_string(), row));
    }
    (lines, view.columns.len() - shown)
}

fn items_at<'a>(state: &AppState<'a>, path: &[String]) -> Option<&'a [JsonNode]> {
    let Source::Json(root) = state.source else {
        return None;
    };
    match json_at(root, path)? {
        JsonNode::Array(items) => Some(items),
        _ => None,
    }
}

pub(super) fn open(state: &mut AppState) {
    let Some(line) = state.lines.get(state.cursor) else {
        return;
    };
    let path = real_path(line).to_vec();
    if !state.is_json {
        state.status_message = Some("table: not available for XML".to_string());
        return;
    }
    let Some(columns) = items_at(state, &path).and_then(columns_of) else {
        state.status_message = Some("table: needs an array of objects".to_string());
        return;
    };
    let rows = items_at(state, &path).map_or(0, <[JsonNode]>::len);
    state.table = Some(TableView {
        path,
        columns,
        rows,
        scroll: Cell::new(0),
        viewport: Cell::new(10),
    });
}

pub(super) fn handle(state: &mut AppState, key: KeyCode) -> bool {
    let Some(view) = state.table.as_ref() else {
        return false;
    };
    let page = view.viewport.get().max(1);
    let last = view.rows.saturating_sub(1);
    let scroll = view.scroll.get();
    let target = match key {
        KeyCode::Esc | KeyCode::Char('t' | 'q') => {
            state.table = None;
            return false;
        }
        KeyCode::Down | KeyCode::Char('j') => scroll + 1,
        KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
        KeyCode::PageDown => scroll + page,
        KeyCode::PageUp => scroll.saturating_sub(page),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => last,
        _ => scroll,
    };
    view.scroll.set(target.min(last));
    false
}

/// What the renderer needs: the rows plus the table's own bookkeeping.
pub(super) fn visible<'a>(
    state: &'a AppState,
    height: usize,
    avail: usize,
) -> Option<(&'a TableView, Vec<String>, usize)> {
    let view = state.table.as_ref()?;
    let items = items_at(state, &view.path)?;
    view.viewport.set(height);
    let max_first = items.len().saturating_sub(height);
    let first = view.scroll.get().min(max_first);
    view.scroll.set(first);
    let (lines, hidden) = render_rows(view, items, first, height, avail);
    Some((view, lines, hidden))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(src: &str) -> Vec<JsonNode> {
        match serde_json::from_str::<JsonNode>(src).unwrap() {
            JsonNode::Array(a) => a,
            _ => panic!("not an array"),
        }
    }

    fn view(columns: &[&str], rows: usize) -> TableView {
        TableView {
            path: vec![],
            columns: columns.iter().map(|c| c.to_string()).collect(),
            rows,
            scroll: Cell::new(0),
            viewport: Cell::new(10),
        }
    }

    #[test]
    fn columns_are_the_union_of_keys_in_first_seen_order() {
        let rows = items(r#"[{"b":1,"a":2},{"c":3,"a":4},{}]"#);
        assert_eq!(columns_of(&rows).unwrap(), ["b", "a", "c"]);
    }

    #[test]
    fn only_non_empty_arrays_of_objects_qualify() {
        assert!(columns_of(&items("[]")).is_none());
        assert!(columns_of(&items("[1,2]")).is_none());
        assert!(columns_of(&items(r#"[{"a":1},[1]]"#)).is_none());
        assert!(columns_of(&items(r#"[{"a":1},null]"#)).is_none());
        assert_eq!(columns_of(&items("[{}]")).unwrap().len(), 0);
    }

    #[test]
    fn cells_show_quoted_strings_bare_numbers_nested_json_and_blank_when_missing() {
        let row = &items(r#"[{"s":"x","n":1,"o":{"k":[1,2]},"z":null}]"#)[0];
        assert_eq!(cell_text(row, "s"), "\"x\"");
        assert_eq!(cell_text(row, "n"), "1");
        assert_eq!(cell_text(row, "o"), r#"{"k":[1,2]}"#);
        assert_eq!(cell_text(row, "z"), "null");
        assert_eq!(cell_text(row, "missing"), "");
    }

    #[test]
    fn control_and_bidi_characters_in_cells_are_escaped() {
        let row = &items("[{\"s\":\"a\\u001b[31m\\u202eb\"}]")[0];
        let text = cell_text(row, "s");
        assert!(
            !text.contains('\u{1b}') && !text.contains('\u{202e}'),
            "{text:?}"
        );
    }

    #[test]
    fn fit_pads_short_text_and_truncates_long_text_with_an_ellipsis() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("é", 1), "é");
        assert_eq!(fit("ééé", 2), "é…");
    }

    #[test]
    fn layout_caps_widths_and_drops_columns_that_do_not_fit() {
        let cols: Vec<String> = ["id", "name", "notes"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cells = vec![vec!["1".into(), "\"Alice\"".into(), "x".repeat(100)]];
        let (widths, shown) = layout(&cols, &cells, 1, 200);
        assert_eq!(widths, [2, 7, MAX_COL_WIDTH]);
        assert_eq!(shown, 3);
        let (_, shown) = layout(&cols, &cells, 1, 16);
        assert_eq!(shown, 2);
        let (_, shown) = layout(&cols, &cells, 1, 15);
        assert_eq!(shown, 1);
        let (_, shown) = layout(&cols, &cells, 1, 1);
        assert_eq!(shown, 1, "always shows at least one column");
    }

    #[test]
    fn rows_render_with_header_rule_index_and_missing_cells() {
        let rows = items(r#"[{"id":1,"n":"A"},{"id":2}]"#);
        let v = view(&["id", "n"], 2);
        let (lines, hidden) = render_rows(&v, &rows, 0, 10, 80);
        assert_eq!(hidden, 0);
        assert_eq!(lines[0], "# │ id │ n  ");
        assert!(lines[1].chars().all(|c| c == '─'));
        assert_eq!(lines[2], "0 │ 1  │ \"A\"");
        assert_eq!(lines[3], "1 │ 2  │    ");
    }

    #[test]
    fn only_the_requested_window_of_rows_is_built() {
        let rows = items(&format!(
            "[{}]",
            (0..1000)
                .map(|i| format!("{{\"i\":{i}}}"))
                .collect::<Vec<_>>()
                .join(",")
        ));
        let v = view(&["i"], 1000);
        let (lines, _) = render_rows(&v, &rows, 500, 3, 80);
        assert_eq!(lines.len(), 5);
        assert!(lines[2].trim_start().starts_with("500"));
        assert!(lines[4].trim_start().starts_with("502"));
    }

    #[test]
    fn hundred_thousand_rows_open_and_render_a_window_quickly() {
        let body = (0..100_000)
            .map(|i| format!("{{\"id\":{i},\"v\":\"x\"}}"))
            .collect::<Vec<_>>()
            .join(",");
        let rows = items(&format!("[{body}]"));
        let cols = columns_of(&rows).unwrap();
        let v = view(&["id", "v"], rows.len());
        assert_eq!(cols, ["id", "v"]);
        let t = std::time::Instant::now();
        let (lines, _) = render_rows(&v, &rows, 99_990, 20, 80);
        assert!(t.elapsed().as_millis() < 500);
        assert_eq!(lines.len(), 12);
    }
}
