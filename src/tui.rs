mod flatten;
mod keys;
mod render;
mod state;

use crate::json_tree::JsonNode;
use crate::xml_tree::XmlNode;
use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
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
/// UI state carried across an `r` reload, matched by path: whatever no longer
/// exists in the new tree is dropped.
#[derive(Default)]
pub struct SavedUi {
    collapsed: HashSet<Vec<String>>,
    array_overrides: HashSet<Vec<String>>,
    tags: HashMap<Vec<String>, u8>,
    cursor_path: Option<Vec<String>>,
    /// Shown in the status bar on the next start (e.g. a reload error).
    pub message: Option<String>,
}

pub enum TuiExit {
    Done(Option<String>),
    Reload(SavedUi),
}

pub struct TuiOpts {
    pub use_color: bool,
    pub pick: bool,
    pub mouse: bool,
    /// `r` reloads (only offered when the input is a file).
    pub reload_ok: bool,
    pub saved: Option<SavedUi>,
}

fn save_ui(state: &AppState) -> SavedUi {
    SavedUi {
        collapsed: state.collapsed.clone(),
        array_overrides: state.array_overrides.clone(),
        tags: state.tags.clone(),
        cursor_path: state.lines.get(state.cursor).map(|l| {
            if l.is_array_summary {
                keys::array_path_for_summary_line(&l.path).to_vec()
            } else {
                l.path.clone()
            }
        }),
        message: None,
    }
}

/// The saved sets that still make sense for the new tree.
fn restored(
    saved: &SavedUi,
    containers: &HashSet<Vec<String>>,
) -> (HashSet<Vec<String>>, HashSet<Vec<String>>) {
    let keep = |set: &HashSet<Vec<String>>| set.intersection(containers).cloned().collect();
    (keep(&saved.collapsed), keep(&saved.array_overrides))
}

fn finish_restore(state: &mut AppState, saved: SavedUi) {
    let known: HashSet<&Vec<String>> = state.all_paths.iter().map(|(p, _)| p).collect();
    state.tags = saved
        .tags
        .into_iter()
        .filter(|(p, _)| known.contains(p))
        .collect();
    if let Some(p) = saved.cursor_path {
        keys::move_cursor_to_path(state, &p);
    }
    state.status_message = saved.message;
}

fn restore_terminal_best_effort() {
    let _ = disable_raw_mode();
    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.execute(DisableMouseCapture);
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
fn run_loop<W, F>(
    mut state: AppState,
    mut rebuild: F,
    out: W,
    mouse: bool,
    reload_ok: bool,
) -> io::Result<TuiExit>
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
    if mouse {
        let _ = out.execute(EnableMouseCapture);
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
    let result = (|| -> io::Result<TuiExit> {
        let mut picked = None;
        loop {
            terminal.draw(|f| render_frame(f, &state))?;
            // Non-key events (notably `Resize`) just fall through to the redraw
            // at the top of the loop; key releases are dropped so Windows
            // doesn't see every key twice (issue #125).
            let event = event::read()?;
            if let Event::Mouse(m) = event {
                handle_mouse(&mut state, m);
                if matches!(m.kind, MouseEventKind::Down(_)) || state.pending_cursor_path.is_some()
                {
                    rebuild(&mut state);
                }
            } else if let Event::Key(key) = event
                && key.kind == KeyEventKind::Press
            {
                if reload_ok
                    && key.code == KeyCode::Char('r')
                    && key.modifiers.is_empty()
                    && !modal_active(&state)
                {
                    return Ok(TuiExit::Reload(save_ui(&state)));
                }
                let quit = handle_key_event(&mut state, key);
                if key_may_change_lines(&state, &key) {
                    rebuild(&mut state);
                }
                if state.pick_result.is_some() {
                    picked = state.pick_result.take();
                }
                if quit {
                    break;
                }
            }
        }
        Ok(TuiExit::Done(picked))
    })();

    let _ = disable_raw_mode();
    if mouse {
        let _ = terminal.backend_mut().execute(DisableMouseCapture);
    }
    let _ = terminal.backend_mut().execute(LeaveAlternateScreen);
    result
}

/// Routes one key press; `true` means quit. `handle_key` only sees
/// `key.code`, so a Ctrl chord used to arrive as its bare letter: Ctrl+C ran
/// `c` (collapse all, issue #125) and Ctrl+E ran `e` (expand all). Now Ctrl+C
/// quits, Ctrl+d/u page (issue #88), and every other Ctrl chord is ignored.
/// Ctrl+Alt is skipped: on some layouts AltGr reports as Ctrl+Alt and is how
/// `[`, `{`, `@` etc. are typed, into the search box included.
fn handle_key_event(state: &mut AppState, key: KeyEvent) -> bool {
    let ctrl =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    if !ctrl {
        return handle_key(state, key.code);
    }
    match key.code {
        KeyCode::Char('c') => return true,
        KeyCode::Char('d') => keys::half_page_down(state),
        KeyCode::Char('u') => keys::half_page_up(state),
        _ => {}
    }
    false
}

/// Wheel moves the cursor three rows; a left click selects the row, and
/// clicking the row that is already selected toggles it. Ignored while a
/// modal (help, popup, inspect, text entry) owns the keyboard.
/// Plain navigation can't change which lines exist, so skip the full
/// re-flatten for it (issue #132) -- unless a search/goto left a cursor
/// target to resolve, or filter text is being typed.
fn key_may_change_lines(state: &AppState, key: &KeyEvent) -> bool {
    if state.pending_cursor_path.is_some() || state.filter_typing {
        return true;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return false; // Ctrl+d/u paging; every other chord is ignored
    }
    !matches!(
        key.code,
        KeyCode::Down
            | KeyCode::Up
            | KeyCode::PageDown
            | KeyCode::PageUp
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Char('j' | 'k' | 'G' | 'H' | 'M' | 'L' | 'i' | 'y' | 'Y' | 'v' | '?')
    )
}

fn modal_active(state: &AppState) -> bool {
    state.help_visible
        || state.popup_visible
        || state.inspect_visible
        || state.searching
        || state.filter_typing
        || state.goto_input.is_some()
        || state.awaiting_depth
        || state.visual_anchor.is_some()
}

fn handle_mouse(state: &mut AppState, m: MouseEvent) {
    if modal_active(state) {
        return;
    }
    match m.kind {
        MouseEventKind::ScrollDown => (0..3).for_each(|_| {
            handle_key(state, KeyCode::Down);
        }),
        MouseEventKind::ScrollUp => (0..3).for_each(|_| {
            handle_key(state, KeyCode::Up);
        }),
        MouseEventKind::Down(MouseButton::Left) => {
            // Row 0 is the tree's top border.
            let Some(row) = (m.row as usize).checked_sub(1) else {
                return;
            };
            if row >= state.viewport_height.get() {
                return;
            }
            let idx = state.scroll_offset.get() + row;
            if idx >= state.lines.len() {
                return;
            }
            if idx == state.cursor {
                handle_key(state, KeyCode::Tab);
            } else {
                state.cursor = idx;
            }
        }
        _ => {}
    }
}

fn open_tty() -> io::Result<std::fs::File> {
    OpenOptions::new().read(true).write(true).open("/dev/tty")
}

/// Every field's starting value, in one place: adding an `AppState` field
/// is one edit here instead of one per entry point.
#[allow(clippy::too_many_arguments)]
fn initial_state<'a>(
    lines: Vec<state::Line>,
    collapsed: HashSet<Vec<String>>,
    all_container_paths: HashSet<Vec<String>>,
    array_overrides: HashSet<Vec<String>>,
    all_paths: Vec<(Vec<String>, String)>,
    is_json: bool,
    use_color: bool,
    pick: bool,
    source: state::Source<'a>,
) -> AppState<'a> {
    AppState {
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
        is_json,
        scroll_offset: std::cell::Cell::new(0),
        viewport_height: std::cell::Cell::new(0),
        all_paths,
        pending_cursor_path: None,
        pending_cursor_occurrence: 0,
        awaiting_depth: false,
        inspect_scroll: std::cell::Cell::new(0),
        count_buffer: None,
        popup_visible: false,
        popup_query: String::new(),
        popup_selected: 0,
        inspect_visible: false,
        popup_scroll_offset: std::cell::Cell::new(0),
        pick_mode: pick,
        pick_result: None,
        visual_anchor: None,
        goto_input: None,
        filter: None,
        filter_typing: false,
        source,
    }
}

pub fn run_json_tui(node: &JsonNode, opts: TuiOpts) -> io::Result<TuiExit> {
    let mut all_container_paths = HashSet::new();
    collect_container_paths_json(node, &[], &mut all_container_paths);
    let (collapsed, array_overrides) = match &opts.saved {
        Some(saved) => restored(saved, &all_container_paths),
        None => Default::default(),
    };
    let mut lines = Vec::new();
    flatten_json(node, &[], 0, &collapsed, &array_overrides, &mut lines);
    let mut all_paths = Vec::new();
    collect_all_paths_json(node, &[], &mut all_paths);
    let mut state = initial_state(
        lines,
        collapsed,
        all_container_paths,
        array_overrides,
        all_paths,
        true,
        opts.use_color,
        opts.pick,
        state::Source::Json(node),
    );
    if let Some(saved) = opts.saved {
        finish_restore(&mut state, saved);
    }
    let rebuild = |s: &mut AppState| rebuild_json_lines(s, node);
    if opts.pick {
        run_loop(state, rebuild, open_tty()?, opts.mouse, opts.reload_ok)
    } else {
        run_loop(state, rebuild, io::stdout(), opts.mouse, opts.reload_ok)
    }
}

pub fn run_xml_tui(node: &XmlNode, opts: TuiOpts) -> io::Result<TuiExit> {
    let mut all_container_paths = HashSet::new();
    collect_container_paths_xml(node, &[], &mut all_container_paths);
    let (collapsed, _) = match &opts.saved {
        Some(saved) => restored(saved, &all_container_paths),
        None => Default::default(),
    };
    let mut lines = Vec::new();
    flatten_xml(node, &[], 0, &collapsed, &mut lines);
    let mut all_paths = Vec::new();
    collect_all_paths_xml(node, &[], &mut all_paths);
    let mut state = initial_state(
        lines,
        collapsed,
        all_container_paths,
        HashSet::new(),
        all_paths,
        false,
        opts.use_color,
        opts.pick,
        state::Source::Xml(node),
    );
    if let Some(saved) = opts.saved {
        finish_restore(&mut state, saved);
    }
    let rebuild = |s: &mut AppState| rebuild_xml_lines(s, node);
    if opts.pick {
        run_loop(state, rebuild, open_tty()?, opts.mouse, opts.reload_ok)
    } else {
        run_loop(state, rebuild, io::stdout(), opts.mouse, opts.reload_ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_for(json: &str) -> AppState<'static> {
        let node = JsonNode::from_value(&serde_json::from_str(json).unwrap());
        let mut lines = Vec::new();
        flatten_json(&node, &[], 0, &HashSet::new(), &HashSet::new(), &mut lines);
        let mut containers = HashSet::new();
        collect_container_paths_json(&node, &[], &mut containers);
        initial_state(
            lines,
            HashSet::new(),
            containers,
            HashSet::new(),
            Vec::new(),
            true,
            false,
            false,
            state::Source::None,
        )
    }

    fn press(state: &mut AppState, code: KeyCode, modifiers: KeyModifiers) -> bool {
        handle_key_event(state, KeyEvent::new(code, modifiers))
    }

    #[test]
    fn ctrl_c_quits_but_plain_c_collapses_all() {
        let mut state = state_for(r#"{"a":{"b":1}}"#);
        assert!(press(&mut state, KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(
            state.collapsed.is_empty(),
            "Ctrl+C must not run collapse-all"
        );
        assert!(!press(&mut state, KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!state.collapsed.is_empty(), "plain c is still collapse-all");
    }

    #[test]
    fn other_ctrl_chords_are_ignored_instead_of_acting_as_their_bare_letter() {
        // Ctrl+E used to run `e` (expand all); Ctrl+X ran `x` (clear tags).
        let mut state = state_for(r#"{"a":{"b":1}}"#);
        state.collapsed.insert(vec!["a".to_string()]);
        state.tags.insert(vec!["a".to_string()], 1);
        assert!(!press(
            &mut state,
            KeyCode::Char('e'),
            KeyModifiers::CONTROL
        ));
        assert!(!press(
            &mut state,
            KeyCode::Char('x'),
            KeyModifiers::CONTROL
        ));
        assert_eq!(state.collapsed.len(), 1);
        assert_eq!(state.tags.len(), 1);
    }

    #[test]
    fn ctrl_d_and_ctrl_u_move_half_a_viewport() {
        // Issue #88.
        let many: Vec<String> = (0..100).map(|i| format!("\"k{i}\":{i}")).collect();
        let mut state = state_for(&format!("{{{}}}", many.join(",")));
        state.viewport_height.set(20);
        press(&mut state, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(state.cursor, 10);
        press(&mut state, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(state.cursor, 20);
        press(&mut state, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(state.cursor, 10);
        for _ in 0..5 {
            press(&mut state, KeyCode::Char('u'), KeyModifiers::CONTROL);
        }
        assert_eq!(state.cursor, 0, "clamps at the top");
        state.cursor = 95;
        for _ in 0..3 {
            press(&mut state, KeyCode::Char('d'), KeyModifiers::CONTROL);
        }
        assert_eq!(state.cursor, 99, "clamps at the last line");
    }

    #[test]
    fn half_page_is_at_least_one_line_before_the_first_frame_sets_a_height() {
        let mut state = state_for(r#"{"a":1,"b":2,"c":3}"#);
        press(&mut state, KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn altgr_typed_characters_reach_the_search_box() {
        // Some layouts report AltGr as Ctrl+Alt; `[` must still type.
        let mut state = state_for(r#"{"a":1}"#);
        state.searching = true;
        press(
            &mut state,
            KeyCode::Char('['),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert_eq!(state.search, "[");
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

    #[test]
    fn mouse_wheel_moves_the_cursor_and_clicks_select_then_toggle() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut state = state_for(r#"{"a": {"x": 1}, "b": 2, "c": 3}"#);
        state.viewport_height.set(10);
        let ev = |kind, row| MouseEvent {
            kind,
            column: 2,
            row,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut state, ev(MouseEventKind::ScrollDown, 3));
        assert_eq!(state.cursor, 3.min(state.lines.len() - 1));
        // click row 1 (first tree row) selects line 0, a second click collapses it
        handle_mouse(&mut state, ev(MouseEventKind::Down(MouseButton::Left), 1));
        assert_eq!(state.cursor, 0);
        let before = state.collapsed.len();
        handle_mouse(&mut state, ev(MouseEventKind::Down(MouseButton::Left), 1));
        assert_ne!(state.collapsed.len(), before);
        // clicks past the content or on the border do nothing
        handle_mouse(&mut state, ev(MouseEventKind::Down(MouseButton::Left), 0));
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn reload_state_keeps_what_still_exists_and_drops_the_rest() {
        let mut state = state_for(r#"{"a": {"x": 1}, "b": {"y": 2}}"#);
        state.collapsed.insert(vec!["a".to_string()]);
        state.collapsed.insert(vec!["gone".to_string()]);
        state.tags.insert(vec!["b".to_string(), "y".to_string()], 3);
        state.tags.insert(vec!["gone".to_string()], 1);
        state.all_paths = vec![(vec!["b".to_string(), "y".to_string()], String::new())];
        state.cursor = 2;
        let saved = save_ui(&state);
        assert_eq!(saved.cursor_path.as_deref(), Some(&["b".to_string()][..]));

        let new_containers: HashSet<Vec<String>> =
            [vec!["a".to_string()], vec!["b".to_string()]].into();
        let (collapsed, _) = restored(&saved, &new_containers);
        assert_eq!(collapsed, [vec!["a".to_string()]].into());

        let mut fresh = state_for(r#"{"a": {"x": 1}, "b": {"y": 2}}"#);
        fresh.all_paths = state.all_paths.clone();
        finish_restore(&mut fresh, saved);
        assert_eq!(fresh.tags.len(), 1);
        assert_eq!(fresh.cursor, 2);
    }

    #[test]
    fn plain_navigation_skips_the_rebuild_but_structural_keys_and_pending_targets_do_not() {
        let mut state = state_for(r#"{"a": {"x": 1}}"#);
        let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
        assert!(!key_may_change_lines(&state, &key(KeyCode::Char('j'))));
        assert!(!key_may_change_lines(&state, &key(KeyCode::Down)));
        assert!(key_may_change_lines(&state, &key(KeyCode::Tab)));
        assert!(key_may_change_lines(&state, &key(KeyCode::Char('e'))));
        state.filter_typing = true;
        assert!(key_may_change_lines(&state, &key(KeyCode::Char('j'))));
        state.filter_typing = false;
        state.pending_cursor_path = Some(vec!["a".to_string()]);
        assert!(key_may_change_lines(&state, &key(KeyCode::Char('j'))));
    }
}
