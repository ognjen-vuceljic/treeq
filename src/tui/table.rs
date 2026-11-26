//! `t`: show the array under the cursor as a table (one row per element,
//! one column per key, in order of first appearance). Only arrays whose
//! elements are all objects qualify. A cell cursor (`hjkl`/arrows) moves over
//! it, columns scroll sideways to keep it in view, and the cell's full value
//! is shown in a pane below. Cells are built for the visible rows only, so a
//! 100k-row array opens instantly.

use super::keys::{json_at, real_path};
use super::state::{AppState, Source};
use crate::color::Color as TqColor;
use crate::json_tree::{JsonNode, JsonScalar, is_unsafe_display_char, to_compact_json};
use crossterm::event::KeyCode;
use std::cell::Cell;

const MAX_COL_WIDTH: usize = 24;
/// How wide the selected column may grow to show more of its value.
const MAX_SELECTED_WIDTH: usize = 60;
const SEPARATOR: &str = " │ ";

pub(super) struct TableView {
    pub(super) path: Vec<String>,
    /// Columns in display order (`base_columns`, or sorted by name with `S`).
    pub(super) columns: Vec<String>,
    base_columns: Vec<String>,
    pub(super) rows: usize,
    /// `s`: the column rows are sorted by, and whether descending.
    pub(super) sort: Option<(String, bool)>,
    /// `S`: columns ordered by name (`Some(true)` = descending).
    pub(super) col_sort: Option<bool>,
    /// Display position -> index into the array; empty means unsorted.
    order: Vec<usize>,
    /// Selected cell.
    pub(super) row: usize,
    pub(super) col: usize,
    /// First visible row / column, kept in range by `build` (a `Cell` so the
    /// renderer can update them without `&mut AppState`).
    pub(super) scroll: Cell<usize>,
    pub(super) col_scroll: Cell<usize>,
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

fn field_of<'a>(row: &'a JsonNode, column: &str) -> Option<&'a JsonNode> {
    let JsonNode::Object(fields) = row else {
        return None;
    };
    fields.iter().find(|(k, _)| k == column).map(|(_, v)| v)
}

/// Strings show quoted, nested values as compact JSON, missing keys blank.
pub(super) fn cell_text(row: &JsonNode, column: &str) -> String {
    match field_of(row, column) {
        None => String::new(),
        Some(JsonNode::Scalar(s)) => s.display(),
        Some(nested) => defang(&to_compact_json(nested)),
    }
}

/// The colour a cell is drawn in: its scalar type's colour, dim for nested
/// values, none for a missing key.
fn cell_color(row: &JsonNode, column: &str) -> Option<TqColor> {
    match field_of(row, column)? {
        JsonNode::Scalar(s) => Some(s.color()),
        _ => Some(TqColor::Structural),
    }
}

/// `(type, text)` for the detail pane: the whole value, never truncated.
pub(super) fn cell_detail(row: &JsonNode, column: &str) -> (String, String) {
    match field_of(row, column) {
        None => ("missing".to_string(), String::new()),
        Some(JsonNode::Scalar(JsonScalar::Str(s))) => (
            format!("string, {} chars", s.chars().count()),
            cell_text(row, column),
        ),
        Some(JsonNode::Scalar(JsonScalar::Number(_))) => ("number".into(), cell_text(row, column)),
        Some(JsonNode::Scalar(JsonScalar::Bool(_))) => ("boolean".into(), cell_text(row, column)),
        Some(JsonNode::Scalar(JsonScalar::Null)) => ("null".into(), cell_text(row, column)),
        Some(JsonNode::Object(f)) => (format!("object, {} keys", f.len()), cell_text(row, column)),
        Some(JsonNode::Array(a)) => (format!("array, {} items", a.len()), cell_text(row, column)),
    }
}

/// What `y` copies: a string's raw content, anything else as compact JSON.
pub(super) fn cell_copy_text(row: &JsonNode, column: &str) -> Option<String> {
    match field_of(row, column)? {
        JsonNode::Scalar(JsonScalar::Str(s)) => Some(s.clone()),
        other => Some(to_compact_json(other)),
    }
}

/// Terminal-unsafe characters (C1, bidi, ...) shown as `\uXXXX`; quotes stay
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

/// Most the selected column may take: `MAX_SELECTED_WIDTH`, but never more
/// than the screen holds next to the index, and never less than a normal
/// column.
pub(super) fn selected_cap(index_width: usize, avail: usize) -> usize {
    let room = avail.saturating_sub(index_width + SEPARATOR.chars().count());
    MAX_SELECTED_WIDTH.min(room).max(MAX_COL_WIDTH)
}

/// One column width per column from the header and the visible cells
/// (capped at `MAX_COL_WIDTH`), except the selected column, which grows up to
/// `selected_cap` to show more of its value.
pub(super) fn widths(
    columns: &[String],
    cells: &[Vec<String>],
    selected: Option<(usize, usize)>,
) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let widest = cells
                .iter()
                .map(|r| r[i].chars().count())
                .max()
                .unwrap_or(0);
            let cap = match selected {
                Some((sel, cap)) if sel == i => cap,
                _ => MAX_COL_WIDTH,
            };
            c.chars().count().max(widest).clamp(1, cap)
        })
        .collect()
}

/// How many columns starting at `first` fit in `avail` characters; at least
/// one, so a very narrow terminal still shows something.
pub(super) fn fit_count(widths: &[usize], first: usize, index_width: usize, avail: usize) -> usize {
    let mut used = index_width;
    let mut shown = 0;
    for w in &widths[first.min(widths.len())..] {
        used += SEPARATOR.chars().count() + w;
        if used > avail && shown > 0 {
            break;
        }
        shown += 1;
    }
    shown
}

/// First visible column so that `col` is on screen: stay where we are when
/// possible, otherwise move just far enough.
pub(super) fn scroll_to_col(
    widths: &[usize],
    current: usize,
    col: usize,
    index_width: usize,
    avail: usize,
) -> usize {
    let mut first = current.min(col);
    while col >= first + fit_count(widths, first, index_width, avail) {
        first += 1;
    }
    first
}

pub(super) struct Field {
    pub(super) text: String,
    pub(super) color: Option<TqColor>,
}

/// The visible window of the table, already fitted to column widths.
pub(super) struct GridRow {
    /// Position on screen order (what the cursor row refers to).
    pub(super) pos: usize,
    /// Index in the array: the row number shown, stable across sorting.
    pub(super) index: usize,
    pub(super) cells: Vec<Field>,
}

pub(super) struct Grid {
    pub(super) index_width: usize,
    pub(super) header: Vec<String>,
    pub(super) rows: Vec<GridRow>,
    pub(super) first_col: usize,
    pub(super) hidden_right: usize,
}

/// Builds the window for the current cursor and scroll state, writing the
/// corrected scroll positions back (the cursor is always kept on screen).
pub(super) fn build(view: &TableView, items: &[JsonNode], height: usize, avail: usize) -> Grid {
    view.viewport.set(height);
    let mut first = view.scroll.get().min(items.len().saturating_sub(height));
    if view.row < first {
        first = view.row;
    } else if height > 0 && view.row >= first + height {
        first = view.row + 1 - height;
    }
    view.scroll.set(first);
    let end = (first + height).min(items.len());
    let positions: Vec<(usize, usize)> = (first..end).map(|p| (p, item_index(view, p))).collect();
    let texts: Vec<Vec<String>> = positions
        .iter()
        .map(|&(_, i)| {
            view.columns
                .iter()
                .map(|c| cell_text(&items[i], c))
                .collect()
        })
        .collect();
    let labels = header_labels(view);
    let index_width = items.len().to_string().len().max(1);
    let cap = selected_cap(index_width, avail);
    let widths = widths(&labels, &texts, Some((view.col, cap)));
    let first_col = scroll_to_col(&widths, view.col_scroll.get(), view.col, index_width, avail);
    view.col_scroll.set(first_col);
    let shown = fit_count(&widths, first_col, index_width, avail);
    let span = first_col..first_col + shown;
    let rows = positions
        .iter()
        .zip(texts)
        .map(|(&(pos, index), text)| GridRow {
            pos,
            index,
            cells: span
                .clone()
                .map(|i| Field {
                    text: fit(&text[i], widths[i]),
                    color: cell_color(&items[index], &view.columns[i]),
                })
                .collect(),
        })
        .collect();
    Grid {
        index_width,
        header: span.clone().map(|i| fit(&labels[i], widths[i])).collect(),
        rows,
        first_col,
        hidden_right: view.columns.len() - span.end,
    }
}

/// Column names, with the sort arrow on the column rows are sorted by.
fn header_labels(view: &TableView) -> Vec<String> {
    view.columns
        .iter()
        .map(|c| match &view.sort {
            Some((name, desc)) if name == c => format!("{c} {}", if *desc { '▼' } else { '▲' }),
            _ => c.clone(),
        })
        .collect()
}

/// Array index shown at display position `pos`.
pub(super) fn item_index(view: &TableView, pos: usize) -> usize {
    view.order.get(pos).copied().unwrap_or(pos)
}

/// jq's ordering between types: null < false < true < numbers < strings <
/// arrays < objects.
fn type_rank(n: &JsonNode) -> u8 {
    match n {
        JsonNode::Scalar(JsonScalar::Null) => 0,
        JsonNode::Scalar(JsonScalar::Bool(false)) => 1,
        JsonNode::Scalar(JsonScalar::Bool(true)) => 2,
        JsonNode::Scalar(JsonScalar::Number(_)) => 3,
        JsonNode::Scalar(JsonScalar::Str(_)) => 4,
        JsonNode::Array(_) => 5,
        JsonNode::Object(_) => 6,
    }
}

fn compare_values(a: &JsonNode, b: &JsonNode) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match type_rank(a).cmp(&type_rank(b)) {
        Ordering::Equal => {}
        other => return other,
    }
    match (a, b) {
        (JsonNode::Scalar(JsonScalar::Number(x)), JsonNode::Scalar(JsonScalar::Number(y))) => {
            match (x.parse::<f64>(), y.parse::<f64>()) {
                (Ok(fx), Ok(fy)) => fx.partial_cmp(&fy).unwrap_or(Ordering::Equal),
                _ => x.cmp(y),
            }
        }
        (JsonNode::Scalar(JsonScalar::Str(x)), JsonNode::Scalar(JsonScalar::Str(y))) => x.cmp(y),
        (JsonNode::Array(x), JsonNode::Array(y)) => cmp_arrays(x, y),
        // ponytail: objects compare by compact text, not jq's keys-then-values
        (JsonNode::Object(_), _) => to_compact_json(a).cmp(&to_compact_json(b)),
        _ => Ordering::Equal,
    }
}

fn cmp_arrays(x: &[JsonNode], y: &[JsonNode]) -> std::cmp::Ordering {
    x.iter()
        .zip(y)
        .map(|(a, b)| compare_values(a, b))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| x.len().cmp(&y.len()))
}

/// Array indexes ordered by `column`; missing cells always last, ties keep
/// the original order (both directions).
pub(super) fn sorted_order(items: &[JsonNode], column: &str, desc: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(
        |&x, &y| match (field_of(&items[x], column), field_of(&items[y], column)) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(a), Some(b)) => {
                let o = compare_values(a, b);
                if desc { o.reverse() } else { o }
            }
        },
    );
    order
}

pub(super) fn items_at<'a>(state: &AppState<'a>, path: &[String]) -> Option<&'a [JsonNode]> {
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
        base_columns: columns.clone(),
        columns,
        sort: None,
        col_sort: None,
        order: Vec::new(),
        rows,
        row: 0,
        col: 0,
        scroll: Cell::new(0),
        col_scroll: Cell::new(0),
        viewport: Cell::new(10),
    });
}

pub(super) fn handle(state: &mut AppState, key: KeyCode) -> bool {
    let Some(view) = state.table.as_mut() else {
        return false;
    };
    let page = view.viewport.get().max(1);
    let (last_row, last_col) = (
        view.rows.saturating_sub(1),
        view.columns.len().saturating_sub(1),
    );
    match key {
        KeyCode::Esc | KeyCode::Char('t' | 'q') => state.table = None,
        KeyCode::Down | KeyCode::Char('j') => view.row = (view.row + 1).min(last_row),
        KeyCode::Up | KeyCode::Char('k') => view.row = view.row.saturating_sub(1),
        KeyCode::PageDown => view.row = (view.row + page).min(last_row),
        KeyCode::PageUp => view.row = view.row.saturating_sub(page),
        KeyCode::Home | KeyCode::Char('g') => view.row = 0,
        KeyCode::End | KeyCode::Char('G') => view.row = last_row,
        KeyCode::Right | KeyCode::Char('l') => view.col = (view.col + 1).min(last_col),
        KeyCode::Left | KeyCode::Char('h') => view.col = view.col.saturating_sub(1),
        KeyCode::Char('0' | '^') => view.col = 0,
        KeyCode::Char('$') => view.col = last_col,
        KeyCode::Char('y') => copy_cell(state),
        KeyCode::Char('s') => sort_rows(state),
        KeyCode::Char('S') => sort_columns(state),
        _ => {}
    }
    false
}

fn copy_cell(state: &mut AppState) {
    let Some(view) = state.table.as_ref() else {
        return;
    };
    let text = items_at(state, &view.path)
        .and_then(|items| items.get(item_index(view, view.row)))
        .and_then(|row| cell_copy_text(row, &view.columns[view.col]));
    state.status_message = Some(match text {
        None => "nothing to copy (missing cell)".to_string(),
        Some(t) => match crate::clipboard::copy_to_clipboard(&t) {
            Ok(()) => format!("copied cell ({} bytes)", t.len()),
            Err(e) => format!("copy failed: {e}"),
        },
    });
}

/// `s`: sort rows by the selected column: ascending, then descending, then
/// back to the original order. The cursor stays on the same element.
fn sort_rows(state: &mut AppState) {
    let Some(view) = state.table.as_ref() else {
        return;
    };
    let Some(items) = items_at(state, &view.path) else {
        return;
    };
    let name = view.columns[view.col].clone();
    let next = match &view.sort {
        Some((n, false)) if *n == name => Some((name, true)),
        Some((n, true)) if *n == name => None,
        _ => Some((name, false)),
    };
    let Some(view) = state.table.as_mut() else {
        return;
    };
    let current = item_index(view, view.row);
    view.order = match &next {
        Some((n, desc)) => sorted_order(items, n, *desc),
        None => Vec::new(),
    };
    view.sort = next;
    view.row = view
        .order
        .iter()
        .position(|&i| i == current)
        .unwrap_or(current);
}

/// `S`: order the columns by name: ascending, descending, then the original
/// order. The cursor stays on the same column.
fn sort_columns(state: &mut AppState) {
    let Some(view) = state.table.as_mut() else {
        return;
    };
    let selected = view.columns[view.col].clone();
    view.col_sort = match view.col_sort {
        None => Some(false),
        Some(false) => Some(true),
        Some(true) => None,
    };
    let mut columns = view.base_columns.clone();
    if let Some(desc) = view.col_sort {
        columns.sort_by_key(|c| (c.to_lowercase(), c.clone()));
        if desc {
            columns.reverse();
        }
    }
    view.col = columns.iter().position(|c| *c == selected).unwrap_or(0);
    view.columns = columns;
}

/// The selected cell's column name, type and full text, for the detail pane.
pub(super) fn selected_detail(state: &AppState) -> Option<(String, String, String)> {
    let view = state.table.as_ref()?;
    let row = items_at(state, &view.path)?.get(item_index(view, view.row))?;
    let column = &view.columns[view.col];
    let (kind, text) = cell_detail(row, column);
    Some((column.clone(), kind, text))
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
        let columns: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
        TableView {
            path: vec![],
            base_columns: columns.clone(),
            columns,
            sort: None,
            col_sort: None,
            order: Vec::new(),
            rows,
            row: 0,
            col: 0,
            scroll: Cell::new(0),
            col_scroll: Cell::new(0),
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
    fn cells_are_coloured_by_type_nested_values_dim_and_missing_uncoloured() {
        let row = &items(
            r#"[{"s":"x","n":1,"t":true,"f":false,"z":null,"o":{"a":1},"u":"https://x.io"}]"#,
        )[0];
        assert_eq!(cell_color(row, "s"), Some(TqColor::Str));
        assert_eq!(cell_color(row, "n"), Some(TqColor::Number));
        assert_eq!(cell_color(row, "t"), Some(TqColor::True));
        assert_eq!(cell_color(row, "f"), Some(TqColor::False));
        assert_eq!(cell_color(row, "z"), Some(TqColor::Null));
        assert_eq!(cell_color(row, "o"), Some(TqColor::Structural));
        assert_eq!(cell_color(row, "u"), Some(TqColor::Url));
        assert_eq!(cell_color(row, "missing"), None);
    }

    #[test]
    fn the_detail_text_is_never_truncated_and_names_the_type() {
        let long = "x".repeat(500);
        let rows = items(&format!(
            r#"[{{"s":"{long}","a":[1,2],"o":{{"k":1}},"n":3}}]"#
        ));
        let (kind, text) = cell_detail(&rows[0], "s");
        assert_eq!(kind, "string, 500 chars");
        assert_eq!(text.chars().count(), 502, "all of it plus the quotes");
        assert_eq!(cell_detail(&rows[0], "a").0, "array, 2 items");
        assert_eq!(cell_detail(&rows[0], "o").0, "object, 1 keys");
        assert_eq!(
            cell_detail(&rows[0], "n"),
            ("number".to_string(), "3".to_string())
        );
        assert_eq!(cell_detail(&rows[0], "gone").0, "missing");
    }

    #[test]
    fn copy_text_is_the_raw_string_or_compact_json() {
        let row = &items(r#"[{"s":"a \"b\"","n":1,"o":{"k":[1]}}]"#)[0];
        assert_eq!(cell_copy_text(row, "s").unwrap(), "a \"b\"");
        assert_eq!(cell_copy_text(row, "n").unwrap(), "1");
        assert_eq!(cell_copy_text(row, "o").unwrap(), r#"{"k":[1]}"#);
        assert!(cell_copy_text(row, "missing").is_none());
    }

    #[test]
    fn control_and_bidi_characters_in_cells_are_escaped() {
        let row = &items("[{\"s\":\"a\\u001b[31m\\u202eb\",\"o\":{\"k\":\"\\u202e\"}}]")[0];
        for column in ["s", "o"] {
            let text = cell_text(row, column);
            assert!(
                !text.contains('\u{1b}') && !text.contains('\u{202e}'),
                "{text:?}"
            );
        }
        let (_, detail) = cell_detail(row, "s");
        assert!(
            !detail.contains('\u{202e}'),
            "the detail pane must be safe too"
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
    fn widths_follow_the_widest_visible_cell_up_to_the_cap() {
        let cols: Vec<String> = ["id", "name", "notes"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cells = vec![vec!["1".into(), "\"Alice\"".into(), "x".repeat(100)]];
        assert_eq!(widths(&cols, &cells, None), [2, 7, MAX_COL_WIDTH]);
    }

    #[test]
    fn fit_count_always_shows_at_least_one_column() {
        let w = [2, 7, 24];
        assert_eq!(fit_count(&w, 0, 1, 200), 3);
        assert_eq!(fit_count(&w, 0, 1, 16), 2);
        assert_eq!(fit_count(&w, 0, 1, 15), 1);
        assert_eq!(fit_count(&w, 0, 1, 1), 1);
        assert_eq!(fit_count(&w, 2, 1, 40), 1);
    }

    #[test]
    fn scroll_to_col_moves_just_far_enough_and_never_loses_the_column() {
        let w = [10, 10, 10, 10, 10];
        let (index, avail) = (1, 30); // two 10-wide columns fit after the index
        assert_eq!(fit_count(&w, 0, index, avail), 2);
        assert_eq!(scroll_to_col(&w, 0, 0, index, avail), 0);
        assert_eq!(scroll_to_col(&w, 0, 1, index, avail), 0);
        assert_eq!(scroll_to_col(&w, 0, 2, index, avail), 1);
        assert_eq!(scroll_to_col(&w, 1, 4, index, avail), 3);
        assert_eq!(
            scroll_to_col(&w, 3, 0, index, avail),
            0,
            "scrolls back left too"
        );
        assert_eq!(
            scroll_to_col(&w, 2, 3, index, avail),
            2,
            "stays put when visible"
        );
    }

    #[test]
    fn every_column_is_reachable_however_narrow_the_terminal() {
        let w = [24, 24, 24, 24];
        for col in 0..4 {
            let first = scroll_to_col(&w, 0, col, 1, 10);
            assert!(
                col >= first && col < first + fit_count(&w, first, 1, 10),
                "col {col}"
            );
        }
    }

    #[test]
    fn the_grid_scrolls_columns_sideways_to_follow_the_cursor() {
        let rows = items(r#"[{"aaaaaaaaaa":1,"bbbbbbbbbb":2,"cccccccccc":3,"dddddddddd":4}]"#);
        let mut v = view(&["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc", "dddddddddd"], 1);
        let g = build(&v, &rows, 5, 30);
        assert_eq!((g.header.len(), g.first_col, g.hidden_right), (2, 0, 2));
        v.col = 3;
        let g = build(&v, &rows, 5, 30);
        assert_eq!(g.first_col, 2);
        assert_eq!(g.header.len(), 2);
        assert_eq!(g.hidden_right, 0);
        assert!(g.header[1].starts_with("dddddddddd"));
        assert_eq!(g.rows[0].cells[1].text.trim_end(), "4");
        v.col = 0;
        let g = build(&v, &rows, 5, 30);
        assert_eq!(g.first_col, 0, "and back again");
    }

    #[test]
    fn the_grid_scrolls_rows_to_keep_the_cursor_visible() {
        let rows = items(&format!(
            "[{}]",
            (0..100)
                .map(|i| format!("{{\"i\":{i}}}"))
                .collect::<Vec<_>>()
                .join(",")
        ));
        let mut v = view(&["i"], 100);
        v.row = 57;
        let g = build(&v, &rows, 10, 40);
        assert_eq!(v.scroll.get(), 48);
        assert_eq!(g.rows.first().unwrap().pos, 48);
        assert_eq!(g.rows.last().unwrap().pos, 57);
        v.row = 3;
        let g = build(&v, &rows, 10, 40);
        assert_eq!(g.rows.first().unwrap().pos, 3);
    }

    #[test]
    fn grid_fields_carry_fitted_text_and_colours_and_missing_cells_are_blank() {
        let rows = items(r#"[{"id":1,"n":"A"},{"id":2}]"#);
        let v = view(&["id", "n"], 2);
        let g = build(&v, &rows, 10, 80);
        assert_eq!(g.header, ["id", "n  "]);
        let r1 = &g.rows[1].cells;
        assert_eq!(r1[0].text, "2 ");
        assert_eq!(r1[0].color, Some(TqColor::Number));
        assert_eq!(r1[1].text, "   ");
        assert_eq!(r1[1].color, None);
        assert_eq!(g.rows[0].cells[1].color, Some(TqColor::Str));
    }

    #[test]
    fn only_the_visible_window_of_rows_is_built() {
        let rows = items(&format!(
            "[{}]",
            (0..1000)
                .map(|i| format!("{{\"i\":{i}}}"))
                .collect::<Vec<_>>()
                .join(",")
        ));
        let mut v = view(&["i"], 1000);
        v.row = 502;
        let g = build(&v, &rows, 3, 80);
        assert_eq!(g.rows.len(), 3);
        assert_eq!(g.rows.last().unwrap().pos, 502);
    }

    #[test]
    fn hundred_thousand_rows_open_and_render_a_window_quickly() {
        let body = (0..100_000)
            .map(|i| format!("{{\"id\":{i},\"v\":\"x\"}}"))
            .collect::<Vec<_>>()
            .join(",");
        let rows = items(&format!("[{body}]"));
        assert_eq!(columns_of(&rows).unwrap(), ["id", "v"]);
        let mut v = view(&["id", "v"], rows.len());
        v.row = 99_999;
        let t = std::time::Instant::now();
        let g = build(&v, &rows, 20, 80);
        assert!(t.elapsed().as_millis() < 500);
        assert_eq!(g.rows.len(), 20);
    }

    #[test]
    fn the_selected_column_grows_while_the_others_keep_the_normal_cap() {
        let cols: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let cells = vec![vec!["x".repeat(50), "y".repeat(50)]];
        assert_eq!(widths(&cols, &cells, Some((0, 60))), [50, MAX_COL_WIDTH]);
        assert_eq!(widths(&cols, &cells, Some((1, 60))), [MAX_COL_WIDTH, 50]);
        assert_eq!(
            widths(&cols, &cells, Some((0, 60))),
            widths(&cols, &cells, Some((0, 60)))
        );
    }

    #[test]
    fn the_selected_width_is_capped_by_the_maximum_and_by_the_screen() {
        assert_eq!(selected_cap(1, 500), MAX_SELECTED_WIDTH);
        assert_eq!(
            selected_cap(1, 40),
            36,
            "index + separator come off the screen width"
        );
        assert_eq!(
            selected_cap(1, 10),
            MAX_COL_WIDTH,
            "never below a normal column"
        );
        let cols = vec!["a".to_string()];
        let cells = vec![vec!["x".repeat(200)]];
        assert_eq!(widths(&cols, &cells, Some((0, 60))), [60]);
    }

    #[test]
    fn a_short_selected_value_does_not_grow_the_column() {
        let cols = vec!["id".to_string()];
        let cells = vec![vec!["1".to_string()]];
        assert_eq!(widths(&cols, &cells, Some((0, 60))), [2]);
    }

    #[test]
    fn the_grid_expands_the_selected_column_and_shrinks_it_back_when_deselected() {
        let long = "z".repeat(50);
        let rows = items(&format!(r#"[{{"a":"{long}","b":"{long}"}}]"#));
        let mut v = view(&["a", "b"], 1);
        let g = build(&v, &rows, 5, 120);
        assert_eq!(
            g.header[0].chars().count(),
            52,
            "selected column shows 50 chars + quotes"
        );
        assert_eq!(g.header[1].chars().count(), MAX_COL_WIDTH);
        v.col = 1;
        let g = build(&v, &rows, 5, 120);
        assert_eq!(
            g.header[0].chars().count(),
            MAX_COL_WIDTH,
            "the old one shrinks back"
        );
        assert_eq!(g.header[1].chars().count(), 52);
    }

    #[test]
    fn an_expanded_column_still_scrolls_into_view_in_a_narrow_terminal() {
        let long = "q".repeat(100);
        let rows = items(&format!(r#"[{{"a":1,"b":"{long}","c":2}}]"#));
        let mut v = view(&["a", "b", "c"], 1);
        v.col = 1;
        let g = build(&v, &rows, 5, 50);
        assert!(!g.header.is_empty());
        let cells = &g.rows[0].cells;
        let selected = g.first_col..g.first_col + g.header.len();
        assert!(selected.contains(&1), "the selected column must be visible");
        assert!(cells.iter().any(|c| c.text.chars().count() > MAX_COL_WIDTH));
    }

    fn order(src: &str, column: &str, desc: bool) -> Vec<usize> {
        sorted_order(&items(src), column, desc)
    }

    #[test]
    fn numbers_sort_numerically_not_alphabetically() {
        let rows = r#"[{"n":10},{"n":2},{"n":33}]"#;
        assert_eq!(order(rows, "n", false), [1, 0, 2]);
        assert_eq!(order(rows, "n", true), [2, 0, 1]);
    }

    #[test]
    fn numbers_with_exponents_decimals_and_negatives_compare_by_value() {
        let rows = r#"[{"n":1e3},{"n":999},{"n":-5},{"n":0.5}]"#;
        assert_eq!(order(rows, "n", false), [2, 3, 1, 0]);
    }

    #[test]
    fn strings_sort_by_code_point_like_jq() {
        let rows = r#"[{"s":"b"},{"s":"B"},{"s":"a"}]"#;
        assert_eq!(order(rows, "s", false), [1, 2, 0]);
    }

    #[test]
    fn mixed_types_follow_jqs_order() {
        let rows =
            r#"[{"v":{"o":1}},{"v":"s"},{"v":[1]},{"v":2},{"v":true},{"v":false},{"v":null}]"#;
        assert_eq!(order(rows, "v", false), [6, 5, 4, 3, 1, 2, 0]);
        assert_eq!(order(rows, "v", true), [0, 2, 1, 3, 4, 5, 6]);
    }

    #[test]
    fn missing_cells_are_last_in_both_directions() {
        let rows = r#"[{"a":1},{"b":9},{"a":3},{}]"#;
        assert_eq!(order(rows, "a", false), [0, 2, 1, 3]);
        assert_eq!(order(rows, "a", true), [2, 0, 1, 3]);
    }

    #[test]
    fn equal_values_keep_their_original_order_in_both_directions() {
        let rows = r#"[{"k":1,"i":0},{"k":2,"i":1},{"k":1,"i":2},{"k":2,"i":3}]"#;
        assert_eq!(order(rows, "k", false), [0, 2, 1, 3]);
        assert_eq!(order(rows, "k", true), [1, 3, 0, 2]);
    }

    #[test]
    fn nested_values_sort_by_their_compact_json() {
        let rows = r#"[{"v":[3]},{"v":[1,2]},{"v":[1]}]"#;
        assert_eq!(order(rows, "v", false), [2, 1, 0]);
    }

    #[test]
    fn sorting_an_unknown_column_changes_nothing() {
        assert_eq!(order(r#"[{"a":2},{"a":1}]"#, "nope", false), [0, 1]);
    }

    #[test]
    fn the_sorted_column_header_carries_an_arrow() {
        let mut v = view(&["a", "b"], 1);
        assert_eq!(header_labels(&v), ["a", "b"]);
        v.sort = Some(("b".into(), false));
        assert_eq!(header_labels(&v), ["a", "b ▲"]);
        v.sort = Some(("b".into(), true));
        assert_eq!(header_labels(&v), ["a", "b ▼"]);
    }

    #[test]
    fn the_grid_follows_the_order_but_keeps_the_original_row_numbers() {
        let rows = items(r#"[{"n":10},{"n":2},{"n":33}]"#);
        let mut v = view(&["n"], 3);
        v.order = sorted_order(&rows, "n", false);
        let g = build(&v, &rows, 10, 40);
        let shown: Vec<(usize, usize, String)> = g
            .rows
            .iter()
            .map(|r| (r.pos, r.index, r.cells[0].text.trim_end().to_string()))
            .collect();
        assert_eq!(
            shown,
            [(0, 1, "2".into()), (1, 0, "10".into()), (2, 2, "33".into())]
        );
    }

    #[test]
    fn item_index_is_the_identity_until_a_sort_is_applied() {
        let mut v = view(&["n"], 3);
        assert_eq!(item_index(&v, 2), 2);
        v.order = vec![2, 0, 1];
        assert_eq!(item_index(&v, 0), 2);
    }

    #[test]
    fn a_hundred_thousand_rows_sort_quickly() {
        let body = (0..100_000)
            .map(|i| format!("{{\"n\":{}}}", (i * 7919) % 100_003))
            .collect::<Vec<_>>()
            .join(",");
        let rows = items(&format!("[{body}]"));
        let t = std::time::Instant::now();
        let o = sorted_order(&rows, "n", false);
        assert!(t.elapsed().as_secs() < 3, "{:?}", t.elapsed());
        assert_eq!(o.len(), 100_000);
        let first = field_of(&rows[o[0]], "n").unwrap();
        let last = field_of(&rows[o[99_999]], "n").unwrap();
        assert!(compare_values(first, last).is_le());
    }
}
