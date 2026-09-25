mod flatten;
mod keys;
mod render;
mod state;

use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
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
use std::fs::OpenOptions;
use std::io;

/// Best-effort terminal restore: raw mode off, and `LeaveAlternateScreen`
/// written straight to `/dev/tty` (not `out`, which the panic hook below
/// doesn't have access to, and which is the same terminal in both pick and
/// normal mode -- pick mode is the one case `out` differs from it, and pick
/// mode renders over `/dev/tty` for exactly this reason). Errors are
/// swallowed: this only ever runs while already unwinding an error or a
/// panic, and a second error here must not shadow or block that.
fn restore_terminal_best_effort() {
    let _ = disable_raw_mode();
    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.execute(LeaveAlternateScreen);
    }
}

/// Installs a panic hook that restores the terminal before the default
/// hook prints the panic message -- otherwise a panic anywhere in the loop
/// (a `render`/`handle_key` bug, not just an `io::Result` `?`) leaves the
/// terminal in raw mode on the alternate screen with no visible message
/// and no working shell until the user runs `reset` (issue #127). Installs
/// only once per process; a second TUI session in the same run (there
/// isn't one today, but nothing prevents it) doesn't stack duplicate hooks.
fn install_panic_restore_hook() {
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal_best_effort();
            previous(info);
        }));
    });
}

/// Renders over `/dev/tty` in pick mode rather than stdout, since pick
/// mode's whole point is capturing stdout (`$(treeq --pick file.json)` or
/// a pipe into `tmux load-buffer`) -- the UI must stay off that stream.
fn run_loop<W, F>(mut state: AppState, mut rebuild: F, out: W) -> io::Result<Option<String>>
where
    W: io::Write,
    F: FnMut(&mut AppState),
{
    install_panic_restore_hook();
    enable_raw_mode()?;
    let mut out = out;
    if let Err(e) = out.execute(EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }
    let backend = CrosstermBackend::new(out);
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            let _ = disable_raw_mode();
            return Err(e);
        }
    };

    // However the loop below ends -- a clean quit, or an `io::Result` `?`
    // propagating out of `draw`/`event::read` -- the terminal must be
    // restored on every path, not just the success one (issue #127).
    let result = (|| -> io::Result<Option<String>> {
        let mut picked = None;
        loop {
            terminal.draw(|f| render_frame(f, &state))?;
            // Non-key events (notably `Resize`) just fall through to the redraw
            // at the top of the loop; key releases are dropped so Windows
            // doesn't see every key twice (issue #125).
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                if is_ctrl_c(&key) {
                    break;
                }
                let quit = handle_key(&mut state, key.code);
                rebuild(&mut state);
                if state.pick_result.is_some() {
                    picked = state.pick_result.take();
                }
                if quit {
                    break;
                }
            }
        }
        Ok(picked)
    })();

    let _ = disable_raw_mode();
    let _ = terminal.backend_mut().execute(LeaveAlternateScreen);
    result
}

/// Raw mode delivers Ctrl+C as a plain key press, and `handle_key` only
/// sees `key.code` -- without this it ran `c` (collapse all) instead.
fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn open_tty() -> io::Result<std::fs::File> {
    OpenOptions::new().read(true).write(true).open("/dev/tty")
}

pub fn run_json_tui(node: &JsonNode, use_color: bool, pick: bool) -> io::Result<Option<String>> {
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
        viewport_height: std::cell::Cell::new(0),
        all_paths,
        pending_cursor_path: None,
        pending_cursor_occurrence: 0,
        count_buffer: None,
        popup_visible: false,
        popup_query: String::new(),
        popup_selected: 0,
        inspect_visible: false,
        popup_scroll_offset: std::cell::Cell::new(0),
        pick_mode: pick,
        pick_result: None,
        visual_anchor: None,
    };
    if pick {
        run_loop(state, |s| rebuild_json_lines(s, node), open_tty()?)
    } else {
        run_loop(state, |s| rebuild_json_lines(s, node), io::stdout())
    }
}

pub fn run_xml_tui(node: &XmlNode, use_color: bool, pick: bool) -> io::Result<Option<String>> {
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
        viewport_height: std::cell::Cell::new(0),
        all_paths,
        pending_cursor_path: None,
        pending_cursor_occurrence: 0,
        count_buffer: None,
        popup_visible: false,
        popup_query: String::new(),
        popup_selected: 0,
        inspect_visible: false,
        popup_scroll_offset: std::cell::Cell::new(0),
        pick_mode: pick,
        pick_result: None,
        visual_anchor: None,
    };
    if pick {
        run_loop(state, |s| rebuild_xml_lines(s, node), open_tty()?)
    } else {
        run_loop(state, |s| rebuild_xml_lines(s, node), io::stdout())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_quits_but_plain_c_does_not() {
        assert!(is_ctrl_c(&KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_ctrl_c(&KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE
        )));
    }

    /// Regression guard for issue #127: `run_json_tui`/`run_xml_tui` call
    /// this on every invocation, so installing it more than once (as a
    /// panic hook, a process-wide resource) must be safe and a no-op past
    /// the first call. Doesn't drive an actual panic through it: the panic
    /// hook is process-global, and replacing it here would also intercept
    /// unrelated `#[should_panic]`/`catch_unwind` tests running
    /// concurrently in the same test binary.
    #[test]
    fn panic_restore_hook_installs_idempotently() {
        install_panic_restore_hook();
        install_panic_restore_hook();
    }
}
