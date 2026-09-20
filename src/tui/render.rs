use super::keys::fuzzy_matches;
use super::state::{AppState, HELP_LEGEND, Line, ratatui_color, tag_color};
use crate::color::Color as TqColor;
use ratatui::prelude::*;
use ratatui::text::Line as RtLine;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

fn line_spans(line: &Line, use_color: bool) -> Vec<Span<'static>> {
    let indent = "  ".repeat(line.depth);
    let marker = if line.has_children { "▸ " } else { "" };
    let structural_style = if use_color {
        Style::default().fg(ratatui_color(TqColor::Structural))
    } else {
        Style::default()
    };
    let key_style = if use_color {
        Style::default().fg(ratatui_color(TqColor::Key))
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
                && fuzzy_matches(&line.path.join("."), &state.search)
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
