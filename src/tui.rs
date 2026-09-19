use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use std::collections::HashSet;
use std::io;

struct Line {
    depth: usize,
    label: String,
    path: Vec<String>,
    has_children: bool,
}

struct AppState {
    lines: Vec<Line>,
    collapsed: HashSet<Vec<String>>,
    cursor: usize,
    search: String,
    searching: bool,
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
        let has_children = !matches!(child, JsonNode::Scalar(_));
        let display = match child {
            JsonNode::Scalar(s) => format!("{label}: {s}"),
            _ => label.clone(),
        };
        out.push(Line {
            depth,
            label: display,
            path: child_path.clone(),
            has_children,
        });
        if has_children && !collapsed.contains(&child_path) {
            flatten_json(child, &child_path, depth + 1, collapsed, out);
        }
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
        let mut display = child.name.clone();
        if let Some(text) = &child.text {
            display.push_str(&format!(": {text}"));
        }
        out.push(Line {
            depth,
            label: display,
            path: child_path.clone(),
            has_children,
        });
        if has_children && !collapsed.contains(&child_path) {
            flatten_xml(child, &child_path, depth + 1, collapsed, out);
        }
    }
}

fn render(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let items: Vec<ListItem> = state
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let indent = "  ".repeat(line.depth);
            let marker = if line.has_children { "▸ " } else { "" };
            let text = format!("{indent}{marker}{}", line.label);
            let style = if i == state.cursor {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );
    let status = if state.searching {
        format!("/{}", state.search)
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
            .label
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

pub fn run_json_tui(node: &JsonNode) -> io::Result<()> {
    let collapsed = HashSet::new();
    let mut lines = Vec::new();
    flatten_json(node, &[], 0, &collapsed, &mut lines);
    let state = AppState {
        lines,
        collapsed,
        cursor: 0,
        search: String::new(),
        searching: false,
    };
    run_loop(state, |s| {
        let mut lines = Vec::new();
        flatten_json(node, &[], 0, &s.collapsed, &mut lines);
        s.lines = lines;
    })
}

pub fn run_xml_tui(node: &XmlNode) -> io::Result<()> {
    let collapsed = HashSet::new();
    let mut lines = Vec::new();
    flatten_xml(node, &[], 0, &collapsed, &mut lines);
    let state = AppState {
        lines,
        collapsed,
        cursor: 0,
        search: String::new(),
        searching: false,
    };
    run_loop(state, |s| {
        let mut lines = Vec::new();
        flatten_xml(node, &[], 0, &s.collapsed, &mut lines);
        s.lines = lines;
    })
}
