use super::state::{AppState, HELP_LEGEND, Line, ratatui_color};
use crate::color::Color as TqColor;
use ratatui::prelude::*;
use ratatui::text::Line as RtLine;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

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
        render_help(frame, area);
        return;
    }
    let items: Vec<ListItem> = state
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let spans = line_spans(line, state.use_color);
            let style = if i == state.cursor {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(RtLine::from(spans)).style(style)
        })
        .collect();
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );
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

fn render_help(frame: &mut Frame, area: Rect) {
    let mut lines: Vec<RtLine> = vec![RtLine::from("Keybindings"), RtLine::from("")];
    for (key, desc) in HELP_LEGEND {
        lines.push(RtLine::from(format!("{key:<14} {desc}")));
    }
    lines.push(RtLine::from(""));
    lines.push(RtLine::from("press ? or Esc to close"));
    let paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Help"));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use std::collections::HashSet;

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
            cursor,
            search: String::new(),
            searching: false,
            use_color: false,
            status_message: None,
            help_visible: false,
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
}
