use super::state::{AppState, Line, ratatui_color};
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
        state
            .lines
            .get(state.cursor)
            .map(|l| l.path.join("."))
            .unwrap_or_default()
    };
    frame.render_widget(Paragraph::new(status), chunks[1]);
}
