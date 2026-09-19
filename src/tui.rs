use crate::clipboard::copy_to_clipboard;
use crate::color::Color as TqColor;
use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::text::Line as RtLine;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use std::collections::HashSet;
use std::io;

struct Line {
    depth: usize,
    key: String,
    value: Option<(String, TqColor)>,
    path: Vec<String>,
    has_children: bool,
}

struct AppState {
    lines: Vec<Line>,
    collapsed: HashSet<Vec<String>>,
    all_container_paths: HashSet<Vec<String>>,
    cursor: usize,
    search: String,
    searching: bool,
    use_color: bool,
    status_message: Option<String>,
}

fn ratatui_color(color: TqColor) -> Color {
    match color {
        TqColor::Key => Color::Cyan,
        TqColor::Str => Color::Green,
        TqColor::Number => Color::Yellow,
        TqColor::Bool => Color::Magenta,
        TqColor::Null | TqColor::Structural => Color::DarkGray,
    }
}

fn flatten_json(
    node: &JsonNode,
    path: &[String],
    depth: usize,
    collapsed: &HashSet<Vec<String>>,
    out: &mut Vec<Line>,
) {
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), v))
            .collect(),
        JsonNode::Scalar(_) => return,
    };
    for (label, child) in entries {
        let mut child_path = path.to_vec();
        child_path.push(label.clone());
        let (value, has_children) = match child {
            JsonNode::Scalar(s) => (Some((s.display(), s.color())), false),
            _ => (None, true),
        };
        out.push(Line {
            depth,
            key: label,
            value,
            path: child_path.clone(),
            has_children,
        });
        if has_children && !collapsed.contains(&child_path) {
            flatten_json(child, &child_path, depth + 1, collapsed, out);
        }
    }
}

fn collect_container_paths_json(node: &JsonNode, path: &[String], out: &mut HashSet<Vec<String>>) {
    let entries: Vec<(String, &JsonNode)> = match node {
        JsonNode::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v)).collect(),
        JsonNode::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("[{i}]"), v))
            .collect(),
        JsonNode::Scalar(_) => return,
    };
    for (label, child) in entries {
        if matches!(child, JsonNode::Scalar(_)) {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(label);
        out.insert(child_path.clone());
        collect_container_paths_json(child, &child_path, out);
    }
}

fn collect_container_paths_xml(node: &XmlNode, path: &[String], out: &mut HashSet<Vec<String>>) {
    for child in &node.children {
        if child.children.is_empty() {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(child.name.clone());
        out.insert(child_path.clone());
        collect_container_paths_xml(child, &child_path, out);
    }
}

fn flatten_xml(
    node: &XmlNode,
    path: &[String],
    depth: usize,
    collapsed: &HashSet<Vec<String>>,
    out: &mut Vec<Line>,
) {
    for child in &node.children {
        let mut child_path = path.to_vec();
        child_path.push(child.name.clone());
        let has_children = !child.children.is_empty();
        let value = child.text.clone().map(|t| (t, TqColor::Str));
        out.push(Line {
            depth,
            key: child.name.clone(),
            value,
            path: child_path.clone(),
            has_children,
        });
        if has_children && !collapsed.contains(&child_path) {
            flatten_xml(child, &child_path, depth + 1, collapsed, out);
        }
    }
}

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

fn render(frame: &mut Frame, state: &AppState) {
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

fn jump_to_next_match(state: &mut AppState) {
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

fn handle_key(state: &mut AppState, key: KeyCode) -> bool {
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

fn run_loop<F>(mut state: AppState, mut rebuild: F) -> io::Result<()>
where
    F: FnMut(&mut AppState),
{
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| render(f, &state))?;
        if let Event::Key(key) = event::read()? {
            let quit = handle_key(&mut state, key.code);
            rebuild(&mut state);
            if quit {
                break;
            }
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

pub fn run_json_tui(node: &JsonNode, use_color: bool) -> io::Result<()> {
    let collapsed = HashSet::new();
    let mut lines = Vec::new();
    flatten_json(node, &[], 0, &collapsed, &mut lines);
    let mut all_container_paths = HashSet::new();
    collect_container_paths_json(node, &[], &mut all_container_paths);
    let state = AppState {
        lines,
        collapsed,
        all_container_paths,
        cursor: 0,
        search: String::new(),
        searching: false,
        use_color,
        status_message: None,
    };
    run_loop(state, |s| {
        let mut lines = Vec::new();
        flatten_json(node, &[], 0, &s.collapsed, &mut lines);
        s.lines = lines;
        s.cursor = s.cursor.min(s.lines.len().saturating_sub(1));
    })
}

pub fn run_xml_tui(node: &XmlNode, use_color: bool) -> io::Result<()> {
    let collapsed = HashSet::new();
    let mut lines = Vec::new();
    flatten_xml(node, &[], 0, &collapsed, &mut lines);
    let mut all_container_paths = HashSet::new();
    collect_container_paths_xml(node, &[], &mut all_container_paths);
    let state = AppState {
        lines,
        collapsed,
        all_container_paths,
        cursor: 0,
        search: String::new(),
        searching: false,
        use_color,
        status_message: None,
    };
    run_loop(state, |s| {
        let mut lines = Vec::new();
        flatten_xml(node, &[], 0, &s.collapsed, &mut lines);
        s.lines = lines;
        s.cursor = s.cursor.min(s.lines.len().saturating_sub(1));
    })
}
