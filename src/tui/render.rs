use super::keys::{array_path_for_summary_line, fuzzy_matches, line_search_text, popup_matches};
use super::state::{AppState, HELP_LEGEND, Line, ratatui_color, tag_color};
use crate::color::Color as TqColor;
use ratatui::prelude::*;
use ratatui::text::Line as RtLine;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

fn line_spans(line: &Line, use_color: bool) -> Vec<Span<'static>> {
    let indent = "  ".repeat(line.depth);
    let marker = if line.has_children { "▸ " } else { "" };
    let structural_style = if use_color {
        Style::default().fg(ratatui_color(TqColor::Structural))
    } else {
        Style::default()
    };
    let key_style = if use_color {
        // Bold on top of the key's own color (see issue #43), so a key is
        // distinguishable from its value by weight alone, not just hue --
        // useful for anyone who finds cyan-vs-green hard to tell apart.
        Style::default()
            .fg(ratatui_color(TqColor::Key))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut spans = vec![
        Span::styled(format!("{indent}{marker}"), structural_style),
        Span::styled(line.key.clone(), key_style),
    ];
    if let Some((text, color)) = &line.value {
        let value_style = if use_color {
            Style::default().fg(ratatui_color(*color))
        } else {
            Style::default()
        };
        spans.push(Span::raw(": "));
        spans.push(Span::styled(text.clone(), value_style));
    }
    spans
}

pub(super) fn render(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    if state.help_visible {
        render_help(frame, area, state.use_color);
        return;
    }
    render_tree(frame, area, state);
    if state.popup_visible {
        // A floating card centered over the tree (see issue #41), not a
        // full-screen replacement like the help overlay: the popup is meant
        // to feel like picking from a list layered on top of context that's
        // still visible around its edges.
        let popup_area = centered_rect(70, 60, area);
        frame.render_widget(Clear, popup_area);
        render_search_popup(frame, popup_area, state);
    }
    if state.inspect_visible {
        let popup_area = centered_rect(50, 30, area);
        frame.render_widget(Clear, popup_area);
        render_inspect_popup(frame, popup_area, state);
    }
}

/// The inspect popup (see issue #42): a small floating card with the
/// current node's type/size, full path, and tag — details visible in the
/// tree only indirectly (via color) or not at all (e.g. a string's length).
fn render_inspect_popup(frame: &mut Frame, area: Rect, state: &AppState) {
    let lines: Vec<RtLine> = match state.lines.get(state.cursor) {
        Some(line) => {
            let real_path = if line.is_array_summary {
                array_path_for_summary_line(&line.path)
            } else {
                &line.path
            };
            let tag_text = match state.tags.get(real_path) {
                Some(tag) => format!("{tag}"),
                None => "none".to_string(),
            };
            vec![
                RtLine::from(vec![
                    Span::styled("type: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(line.type_label.clone()),
                ]),
                RtLine::from(vec![
                    Span::styled("path: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(real_path.join(".")),
                ]),
                RtLine::from(vec![
                    Span::styled("tag:  ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(tag_text),
                ]),
            ]
        }
        None => vec![RtLine::from("no node under the cursor")],
    };
    let paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Inspect"));
    frame.render_widget(paragraph, area);
}

/// A `percent_x` x `percent_y` rect centered within `area` (standard
/// ratatui popup-centering idiom).
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn render_search_popup(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Search results (Enter: jump, Tab: cycle, Esc: close)");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(inner);
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("/{}", state.popup_query),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        chunks[0],
    );

    let matches = popup_matches(state);
    if state.popup_query.is_empty() {
        frame.render_widget(
            Paragraph::new("type to search the whole document"),
            chunks[1],
        );
        return;
    }
    if matches.is_empty() {
        frame.render_widget(Paragraph::new("no matches"), chunks[1]);
        return;
    }

    let match_style = if state.use_color {
        Style::default().fg(ratatui_color(TqColor::Key))
    } else {
        Style::default()
    };
    let items: Vec<ListItem> = matches
        .iter()
        .map(|path| ListItem::new(RtLine::from(Span::styled(path.join("."), match_style))))
        .collect();
    let mut list_state = ListState::default()
        .with_offset(state.popup_scroll_offset.get())
        .with_selected(Some(state.popup_selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        chunks[1],
        &mut list_state,
    );
    state.popup_scroll_offset.set(list_state.offset());
}

fn render_tree(frame: &mut Frame, area: Rect, state: &AppState) {
    let items: Vec<ListItem> = state
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let spans = line_spans(line, state.use_color);
            let mut style = Style::default();
            if i == state.cursor {
                // The cursor's own reversed-video highlight takes priority;
                // a tag background would clash visually, so skip it here.
                style = style.add_modifier(Modifier::REVERSED);
            } else if state.use_color
                && !line.is_array_summary
                && let Some(&tag) = state.tags.get(&line.path)
            {
                style = style.bg(tag_color(tag));
            }
            if state.searching
                && !state.search.is_empty()
                && fuzzy_matches(&line_search_text(line), &state.search)
            {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            ListItem::new(RtLine::from(spans)).style(style)
        })
        .collect();
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
    // `ListState` seeded from the offset persisted since the last frame:
    // ratatui only grows/shrinks the window enough to keep the selection
    // visible, so starting from the prior offset (rather than 0 every time)
    // lets the cursor move freely within an already-scrolled viewport
    // instead of re-pinning to the window's last row on every keystroke
    // (see issue #36's fix-review). The resulting offset is saved back for
    // the next frame.
    let mut list_state = ListState::default()
        .with_offset(state.scroll_offset.get())
        .with_selected(Some(state.cursor));
    frame.render_stateful_widget(
        List::new(items).block(Block::default().borders(Borders::ALL)),
        chunks[0],
        &mut list_state,
    );
    state.scroll_offset.set(list_state.offset());
    let status = if state.searching {
        format!("/{}", state.search)
    } else if let Some(buf) = &state.count_buffer {
        format!("g{buf}")
    } else if let Some(msg) = &state.status_message {
        msg.clone()
    } else {
        let path = state
            .lines
            .get(state.cursor)
            .map(|l| l.path.join("."))
            .unwrap_or_default();
        format!("{path}  (?: help)")
    };
    frame.render_widget(Paragraph::new(status), chunks[1]);
}

/// Spans for one "key   description" row of the help legend: the key
/// column styled distinctly (bold, and colored when color is enabled) from
/// the plain description, mirroring how `line_spans` styles a tree row.
fn help_entry_spans(key: &str, desc: &str, use_color: bool) -> Vec<Span<'static>> {
    let key_style = if use_color {
        Style::default()
            .fg(ratatui_color(TqColor::Key))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    vec![
        Span::styled(format!("{key:<14}"), key_style),
        Span::raw(desc.to_string()),
    ]
}

fn render_help(frame: &mut Frame, area: Rect, use_color: bool) {
    let title_style = if use_color {
        Style::default()
            .fg(ratatui_color(TqColor::Str))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let footer_style = if use_color {
        Style::default().fg(ratatui_color(TqColor::Structural))
    } else {
        Style::default()
    };
    let border_style = if use_color {
        Style::default().fg(ratatui_color(TqColor::Key))
    } else {
        Style::default()
    };

    let mut lines: Vec<RtLine> = vec![
        RtLine::from(Span::styled("Keybindings", title_style)),
        RtLine::from(""),
    ];
    for (key, desc) in HELP_LEGEND {
        lines.push(RtLine::from(help_entry_spans(key, desc, use_color)));
    }
    lines.push(RtLine::from(""));
    lines.push(RtLine::from(Span::styled(
        "press ? or Esc to close",
        footer_style,
    )));
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Help")
            .border_style(border_style),
    );
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use std::collections::{HashMap, HashSet};

    fn line(
        key: &str,
        has_children: bool,
        value: Option<(&str, TqColor)>,
        depth: usize,
        path: &[&str],
    ) -> Line {
        Line {
            depth,
            key: key.to_string(),
            value: value.map(|(v, c)| (v.to_string(), c)),
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

    fn buffer_text(buffer: &Buffer) -> String {
        let area = buffer.area;
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn state_with(lines: Vec<Line>, cursor: usize) -> AppState {
        AppState {
            lines,
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
    fn line_spans_for_a_container_has_no_value_segment() {
        let l = line("user", true, None, 0, &["user"]);
        let spans = line_spans(&l, false);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].content.as_ref().contains('▸'));
        assert_eq!(spans[1].content.as_ref(), "user");
    }

    #[test]
    fn line_spans_for_a_leaf_includes_value_segment() {
        let l = line(
            "name",
            false,
            Some(("Alice", TqColor::Str)),
            1,
            &["user", "name"],
        );
        let spans = line_spans(&l, false);
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[1].content.as_ref(), "name");
        assert_eq!(spans[2].content.as_ref(), ": ");
        assert_eq!(spans[3].content.as_ref(), "Alice");
    }

    #[test]
    fn line_spans_indent_scales_with_depth() {
        let l = line("x", false, Some(("1", TqColor::Number)), 3, &["x"]);
        let spans = line_spans(&l, false);
        assert!(spans[0].content.as_ref().starts_with("      "));
    }

    #[test]
    fn key_span_is_bold_when_color_is_enabled_but_not_when_disabled() {
        // Bold distinguishes a key from its value by weight, not just hue
        // (see issue #43); it must not appear when color is off, since
        // --no-color output should stay as plain as it was before.
        let l = line("user", true, None, 0, &["user"]);
        let colored = line_spans(&l, true);
        assert!(colored[1].style.add_modifier.contains(Modifier::BOLD));
        let plain = line_spans(&l, false);
        assert!(!plain[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn tagged_node_renders_with_the_tags_background_color() {
        // Cursor is off this line (index 1) so the tag background isn't
        // masked by the reversed-cursor style.
        let mut state = state_with(
            vec![
                line("root", true, None, 0, &["root"]),
                line("user", true, None, 0, &["user"]),
            ],
            0,
        );
        state.use_color = true;
        state.tags.insert(vec!["user".to_string()], 2);
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        // +1 on both axes: the list widget's border occupies row/col 0.
        assert_eq!(buffer[(1, 2)].bg, tag_color(2));
    }

    #[test]
    fn a_tagged_lines_key_is_still_bold_alongside_the_tag_background() {
        // Regression guard for issue #43: the tag background is applied to
        // the whole ListItem's style, while bold is a per-span style on the
        // key text. Both must survive rendering together, not have one
        // clobber the other.
        let mut state = state_with(
            vec![
                line("root", true, None, 0, &["root"]),
                line("user", true, None, 0, &["user"]),
            ],
            0,
        );
        state.use_color = true;
        state.tags.insert(vec!["user".to_string()], 2);
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        // Row 2 is "▸ user": col 1 is the border, col 2-3 the marker "▸ ",
        // col 4 the start of the key text "user".
        assert_eq!(buffer[(1, 2)].bg, tag_color(2));
        assert!(buffer[(4, 2)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(4, 2)].bg, tag_color(2));
    }

    #[test]
    fn a_tag_is_skipped_on_the_cursors_own_line_to_avoid_clashing_with_reversed_video() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.use_color = true;
        state.tags.insert(vec!["user".to_string()], 2);
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_ne!(buffer[(1, 1)].bg, tag_color(2));
    }

    #[test]
    fn every_fuzzy_search_match_is_underlined_not_just_the_cursors() {
        let mut state = state_with(
            vec![
                line("name", false, Some(("Alice", TqColor::Str)), 0, &["name"]),
                line(
                    "nickname",
                    false,
                    Some(("Al", TqColor::Str)),
                    0,
                    &["nickname"],
                ),
                line("age", false, Some(("30", TqColor::Number)), 0, &["age"]),
            ],
            0,
        );
        state.searching = true;
        state.search = "name".to_string();
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        // +1 on both axes: the list widget's border occupies row/col 0.
        assert!(buffer[(1, 1)].modifier.contains(Modifier::UNDERLINED));
        assert!(buffer[(1, 2)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(1, 3)].modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn a_key_value_spanning_query_underlines_the_matching_line() {
        // Issue #50's own repro: a query spanning a node's key and its
        // rendered value (e.g. `author: "user`) must be treated as a match,
        // not just a query against the dotted path alone.
        let mut state = state_with(
            vec![
                line(
                    "author",
                    false,
                    Some(("\"user0\"", TqColor::Str)),
                    0,
                    &["author"],
                ),
                line("id", false, Some(("\"c7-0\"", TqColor::Str)), 0, &["id"]),
            ],
            0,
        );
        state.searching = true;
        state.search = "author: \"user".to_string();
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer[(1, 1)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buffer[(1, 2)].modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn render_draws_lines_and_status_bar() {
        let state = state_with(
            vec![
                line("user", true, None, 0, &["user"]),
                line(
                    "name",
                    false,
                    Some(("Alice", TqColor::Str)),
                    1,
                    &["user", "name"],
                ),
            ],
            1,
        );
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("user"));
        assert!(text.contains("Alice"));
        assert!(text.contains("user.name"));
    }

    /// The row a cell's symbol matching `needle` first appears on, scanning
    /// left-to-right, top-to-bottom. Lets a test assert on the row a scrolled
    /// item landed on without hardcoding ratatui's scroll-offset math.
    fn row_containing(buffer: &Buffer, needle: &str) -> Option<u16> {
        let area = buffer.area;
        for y in 0..area.height {
            let mut row = String::new();
            for x in 0..area.width {
                row.push_str(buffer[(x, y)].symbol());
            }
            if row.contains(needle) {
                return Some(y);
            }
        }
        None
    }

    fn tall_list(len: usize) -> Vec<Line> {
        (0..len)
            .map(|i| line(&format!("item{i}"), false, None, 0, &["item"]))
            .collect()
    }

    #[test]
    fn viewport_scrolls_to_keep_a_cursor_far_down_a_tall_list_visible() {
        // A plain (non-stateful) List render never scrolls, so on a
        // document taller than the terminal the cursor could sit far below
        // the visible area with no on-screen indication (see issue #36).
        let state = state_with(tall_list(200), 150);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(
            text.contains("item150"),
            "the selected line must be visible on screen: {text}"
        );
        assert!(
            !text.contains("item0"),
            "the viewport must have scrolled past the very first line: {text}"
        );
    }

    #[test]
    fn viewport_shows_the_top_of_the_list_when_cursor_is_at_the_first_line() {
        let state = state_with(tall_list(200), 0);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(
            text.contains("item0"),
            "top of list must be visible: {text}"
        );
    }

    #[test]
    fn viewport_shows_the_bottom_of_the_list_when_cursor_is_at_the_last_line() {
        let state = state_with(tall_list(200), 199);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(
            text.contains("item199"),
            "bottom of list must be visible: {text}"
        );
    }

    #[test]
    fn render_does_not_panic_on_an_empty_document() {
        let state = state_with(vec![], 0);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
    }

    #[test]
    fn scrolled_cursor_line_still_gets_the_reversed_cursor_style() {
        let state = state_with(tall_list(200), 150);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        let y = row_containing(buffer, "item150").expect("cursor line must be on screen");
        assert!(buffer[(1, y)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn tag_background_and_search_underline_still_work_after_scrolling() {
        let mut lines = tall_list(200);
        lines[180] = line("findme", false, None, 0, &["item", "findme"]);
        let mut state = state_with(lines, 185);
        state.use_color = true;
        state
            .tags
            .insert(vec!["item".to_string(), "findme".to_string()], 3);
        state.searching = true;
        state.search = "findme".to_string();
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        let y = row_containing(buffer, "findme").expect("tagged line must be on screen");
        assert_eq!(buffer[(1, y)].bg, tag_color(3));
        assert!(buffer[(1, y)].modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn scroll_offset_persists_across_frames_instead_of_resetting_to_zero() {
        // Regression for the fix-review's finding: rebuilding `ListState`
        // from offset 0 every frame re-pins the cursor to the viewport's
        // last row on every render past one screenful, instead of letting
        // the cursor move within an already-scrolled window. Rendering the
        // same tall list at two adjacent cursor positions should therefore
        // produce the *same* persisted offset, not two independently
        // recomputed ones that both happen to end at the window's edge.
        let mut state = state_with(tall_list(200), 150);
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let offset_at_150 = state.scroll_offset.get();

        state.cursor = 149;
        terminal.draw(|f| render(f, &state)).unwrap();
        let offset_at_149 = state.scroll_offset.get();

        assert_eq!(
            offset_at_150, offset_at_149,
            "moving the cursor up by one within an already-scrolled viewport \
             must not reset the scroll offset"
        );
    }

    #[test]
    fn render_shows_search_prompt_when_searching() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.searching = true;
        state.search = "ali".to_string();
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("/ali"));
    }

    #[test]
    fn render_shows_the_pending_count_buffer_in_the_status_bar() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.count_buffer = Some("12".to_string());
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("g12"));
    }

    #[test]
    fn popup_renders_as_a_centered_card_over_the_tree_not_a_full_replacement() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.popup_visible = true;
        state.popup_query = "user".to_string();
        state.all_paths = vec![(vec!["user".to_string()], "user".to_string())];
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Search results"));
        assert!(text.contains("/user"));
        // The underlying tree must still show around the popup's edges,
        // unlike the full-screen help overlay.
        assert!(
            text.contains("user"),
            "tree content must still be visible behind/around the popup"
        );
    }

    #[test]
    fn popup_shows_a_hint_instead_of_the_whole_document_when_query_is_empty() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.popup_visible = true;
        state.all_paths = vec![
            (vec!["user".to_string()], "user".to_string()),
            (vec!["other".to_string()], "other".to_string()),
        ];
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("type to search"));
    }

    #[test]
    fn popup_shows_no_matches_for_an_unmatched_query() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.popup_visible = true;
        state.popup_query = "zzz".to_string();
        state.all_paths = vec![(vec!["user".to_string()], "user".to_string())];
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("no matches"));
    }

    #[test]
    fn popup_scrolls_to_keep_a_far_down_selection_visible() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.popup_visible = true;
        state.popup_query = "item".to_string();
        state.all_paths = (0..30)
            .map(|i| (vec![format!("item{i}")], format!("item{i}")))
            .collect();
        state.popup_selected = 25;
        let mut terminal = Terminal::new(TestBackend::new(60, 15)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("item25"));
        assert!(!text.contains("item0"));
    }

    #[test]
    fn popup_scrolls_back_up_after_selection_returns_to_the_top() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.popup_visible = true;
        state.popup_query = "item".to_string();
        state.all_paths = (0..30)
            .map(|i| (vec![format!("item{i}")], format!("item{i}")))
            .collect();
        state.popup_selected = 25;
        let mut terminal = Terminal::new(TestBackend::new(60, 15)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();

        state.popup_selected = 0;
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(
            text.contains("item0"),
            "scrolling the selection back to the top must bring item0 back into view"
        );
    }

    fn find_match_row_fg(buffer: &Buffer, expected_text: &str) -> Color {
        let area = buffer.area;
        for y in 0..area.height {
            let row: String = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
            if let Some(col) = row.find(expected_text) {
                return buffer[(col as u16, y)].fg;
            }
        }
        panic!("no row contained {expected_text:?}");
    }

    #[test]
    fn popup_matches_are_plain_without_color() {
        let mut state = state_with(vec![line("root", true, None, 0, &["root"])], 0);
        state.use_color = false;
        state.popup_visible = true;
        state.popup_query = "zq".to_string();
        state.all_paths = vec![(
            vec!["zqx".to_string(), "leaf".to_string()],
            "zqx.leaf".to_string(),
        )];
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        assert_eq!(
            find_match_row_fg(terminal.backend().buffer(), "zqx.leaf"),
            Color::Reset
        );
    }

    #[test]
    fn popup_matches_are_colored_when_color_is_enabled() {
        let mut state = state_with(vec![line("root", true, None, 0, &["root"])], 0);
        state.use_color = true;
        state.popup_visible = true;
        state.popup_query = "zq".to_string();
        state.all_paths = vec![(
            vec!["zqx".to_string(), "leaf".to_string()],
            "zqx.leaf".to_string(),
        )];
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        assert_eq!(
            find_match_row_fg(terminal.backend().buffer(), "zqx.leaf"),
            ratatui_color(TqColor::Key)
        );
    }

    #[test]
    fn inspect_popup_renders_as_a_centered_card_showing_type_path_and_tag() {
        let mut state = state_with(
            vec![line(
                "name",
                false,
                Some(("\"Alice\"", TqColor::Str)),
                0,
                &["user", "name"],
            )],
            0,
        );
        state.inspect_visible = true;
        state
            .tags
            .insert(vec!["user".to_string(), "name".to_string()], 3);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Inspect"));
        assert!(text.contains("string (0 chars)"));
        assert!(text.contains("user.name"));
        assert!(text.contains("3"));
    }

    #[test]
    fn inspect_popup_on_an_array_summary_line_uses_the_arrays_real_path_and_tag() {
        let summary = Line {
            depth: 0,
            key: "…more".to_string(),
            value: Some(("3 more (Tab to show all)".to_string(), TqColor::Structural)),
            path: vec!["tags".to_string(), "…more".to_string()],
            has_children: false,
            is_array_summary: true,
            type_label: "array preview marker".to_string(),
        };
        let mut state = state_with(vec![summary], 0);
        state.inspect_visible = true;
        state.tags.insert(vec!["tags".to_string()], 5);
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(
            !text.contains("path: tags.…more"),
            "inspect popup must show the array's real path, not the synthetic marker segment"
        );
        assert!(text.contains("path: tags"));
        assert!(text.contains("5"));
    }

    #[test]
    fn inspect_popup_shows_none_when_the_node_has_no_tag() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.inspect_visible = true;
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("none"));
    }

    #[test]
    fn render_shows_status_message_over_path_when_present() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.status_message = Some("copied: user".to_string());
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("copied: user"));
    }

    #[test]
    fn status_bar_hints_at_the_help_key() {
        let state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("?: help"));
    }

    #[test]
    fn render_shows_help_overlay_with_every_keybinding_when_visible() {
        let mut state = state_with(vec![line("user", true, None, 0, &["user"])], 0);
        state.help_visible = true;
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Keybindings"));
        for (key, desc) in HELP_LEGEND {
            assert!(
                text.contains(key.split(' ').next().unwrap()),
                "missing key '{key}' in help overlay: {text}"
            );
            assert!(
                text.contains(desc),
                "missing description '{desc}' in help overlay: {text}"
            );
        }
        // The underlying tree must not render behind the help overlay.
        assert!(!text.contains("user"));
    }

    #[test]
    fn help_entry_key_column_is_bold_and_plain_without_color() {
        let spans = help_entry_spans("q / Esc", "quit", false);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].content.starts_with("q / Esc"));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(spans[0].style.fg, None);
        assert_eq!(spans[1].content.as_ref(), "quit");
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn help_entry_key_column_is_colored_when_color_is_enabled() {
        let spans = help_entry_spans("q / Esc", "quit", true);
        assert_eq!(spans[0].style.fg, Some(ratatui_color(TqColor::Key)));
        // Only the key column is colored, not the description.
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn help_overlay_uses_a_distinct_border_and_title_color_when_enabled() {
        let mut state = state_with(vec![], 0);
        state.help_visible = true;
        state.use_color = true;
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal.draw(|f| render(f, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        // The top-left border cell should be styled with the "Key" color.
        assert_eq!(buffer[(0, 0)].fg, ratatui_color(TqColor::Key));
    }

    #[test]
    fn every_help_legend_key_is_documented_in_the_readme() {
        let readme =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();
        for (key, _) in HELP_LEGEND {
            let first_key = key.split(" / ").next().unwrap();
            assert!(
                readme.contains(first_key),
                "README's keybindings table appears to be missing key '{key}' \
                 (HELP_LEGEND and the README have drifted apart)"
            );
        }
    }
}
