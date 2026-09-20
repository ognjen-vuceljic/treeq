mod flatten;
mod keys;
mod render;
mod state;

use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use flatten::{
    collect_all_paths_json, collect_all_paths_xml, collect_container_paths_json,
    collect_container_paths_xml, flatten_json, flatten_xml,
};
use keys::handle_key;
use ratatui::prelude::*;
use render::render as render_frame;
use state::{AppState, rebuild_json_lines, rebuild_xml_lines};
use std::collections::{HashMap, HashSet};
use std::io;

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
        terminal.draw(|f| render_frame(f, &state))?;
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
    let array_overrides = HashSet::new();
    let mut lines = Vec::new();
    flatten_json(node, &[], 0, &collapsed, &array_overrides, &mut lines);
    let mut all_container_paths = HashSet::new();
    collect_container_paths_json(node, &[], &mut all_container_paths);
    let mut all_paths = Vec::new();
    collect_all_paths_json(node, &[], &mut all_paths);
    let state = AppState {
        lines,
        collapsed,
        all_container_paths,
        array_overrides,
        tags: HashMap::new(),
        cursor: 0,
        search: String::new(),
        searching: false,
        use_color,
        status_message: None,
        help_visible: false,
        is_json: true,
        scroll_offset: std::cell::Cell::new(0),
        all_paths,
        pending_cursor_path: None,
        count_buffer: None,
        popup_visible: false,
        popup_query: String::new(),
        popup_selected: 0,
        inspect_visible: false,
    };
    run_loop(state, |s| rebuild_json_lines(s, node))
}

pub fn run_xml_tui(node: &XmlNode, use_color: bool) -> io::Result<()> {
    let collapsed = HashSet::new();
    let mut lines = Vec::new();
    flatten_xml(node, &[], 0, &collapsed, &mut lines);
    let mut all_container_paths = HashSet::new();
    collect_container_paths_xml(node, &[], &mut all_container_paths);
    let mut all_paths = Vec::new();
    collect_all_paths_xml(node, &[], &mut all_paths);
    let state = AppState {
        lines,
        collapsed,
        all_container_paths,
        array_overrides: HashSet::new(),
        tags: HashMap::new(),
        cursor: 0,
        search: String::new(),
        searching: false,
        use_color,
        status_message: None,
        help_visible: false,
        is_json: false,
        scroll_offset: std::cell::Cell::new(0),
        all_paths,
        pending_cursor_path: None,
        count_buffer: None,
        popup_visible: false,
        popup_query: String::new(),
        popup_selected: 0,
        inspect_visible: false,
    };
    run_loop(state, |s| rebuild_xml_lines(s, node))
}
