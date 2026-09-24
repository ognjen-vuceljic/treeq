# treeq — richer color system + tmux/neovim integration

> **For agentic workers:** This is a prioritized spec/feature-request doc,
> not a step-by-step plan. Before implementing an item, turn it into a
> proper plan (see `docs/superpowers/specs/2026-09-19-treeq-design.md` and
> `docs/superpowers/plans/2026-09-19-treeq-v1.md` for the house style) and
> use superpowers:subagent-driven-development or
> superpowers:executing-plans to carry it out. Follow `CONTRIBUTING.md`
> (branch naming, ~20-line methods, `cargo fmt`/`clippy`/`cargo test`, an
> issue reference in the PR description, 95% coverage gate).

Date: 2026-09-21
Status: proposed, pre-implementation
Context: follow-up review of v0.3.1. 34 issues already closed; only #82
open. This doc covers two things the current codebase is genuinely thin
on: (A) visual polish/color, and (B) tmux/neovim ecosystem integration,
which is treeq's most obvious and currently untapped differentiator versus
`jless`/`otree`.

Read `src/color.rs`, `src/render.rs`, `src/tui/render.rs`, and
`src/tui/keys.rs` before starting — this doc references their current
behavior throughout rather than re-deriving it.

---

## Part A — Color system

### Current state (baseline, verified in this repo)

- `src/color.rs` maps 6 semantic buckets (`Key`, `Str`, `Number`, `Bool`,
  `Null`, `Structural`) to 6 fixed **16-color ANSI** codes. Used by both
  the static renderer (`render.rs`) and, via `ratatui_color()` in
  `tui/state.rs`, the TUI.
- The TUI's 8-color **tag** palette (`tui/render.rs`'s `TAG_PALETTE`) is
  already dim 24-bit RGB and reads well — it's the one place in the
  codebase that already does what Part A asks for everywhere else. Use it
  as the reference aesthetic.
- No depth-based differentiation: every `├──`/`│` guide line at every
  nesting level is the same dim gray. On a deeply nested document, the
  tree structure is legible from indentation alone, not from color.
- No color for terminal-detected special string content (URLs, ISO 8601
  timestamps, UUIDs) — every string is the same green regardless of shape.
- No visual distinction for a search match's *matched substring* — only
  cursor-row reverse-video and tag backgrounds exist as row-level
  highlighting (`tui/render.rs`, `Modifier::REVERSED` / `.bg(tag_color(..))`).
- No color capability tiering: `use_color()` in `main.rs` is a binary
  TTY + `NO_COLOR` check. No 24-bit vs 256-color vs 16-color fallback, and
  no way to force color on into a pipe (e.g. `treeq --static file.json |
  less -R`).

### A1. Truecolor semantic palette (Priority: high, effort: medium)

Replace the 6 flat ANSI codes with dim 24-bit RGB tones in the same
family as the existing `TAG_PALETTE` — same "dim/dark tint, consistent
luminance" aesthetic, just applied to types instead of tags. Concretely:
give `Key`, `Str`, `Number`, `Bool`, `Null` each a distinct RGB tone at
roughly the same perceived brightness (reuse the luminance band the tag
palette test already asserts — max channel in `50..=100` — as a starting
point, or a slightly brighter band since this is foreground text, not a
background wash).

- Detect terminal color capability: `COLORTERM=truecolor|24bit` →
  24-bit; else fall back to today's 16-color ANSI codes; `NO_COLOR` or
  non-TTY → no color, unchanged.
- `src/color.rs`'s `Color` enum and `code()` function stay the shape they
  are; add a capability parameter (or a second `code_truecolor()` used
  when supported) rather than a new abstraction — this is a lookup table
  swap, not a new color-management layer.
- Update the existing `wraps_text_in_ansi_codes_when_enabled` /
  `returns_plain_text_when_disabled` tests plus add one per capability
  tier.
- `tui/state.rs`'s `ratatui_color()` gets the equivalent RGB variants —
  `ratatui::style::Color::Rgb` already used by `tag_color()`, so no new
  ratatui-side capability needed.

### A2. Depth-indent tinting (Priority: high, effort: low-medium)

Cycle the `Structural` guide-line color (the `├──`/`│`/`└──` prefix) through
a short sequence of dim, desaturated hues keyed on `depth % N` (5-6 tones
is plenty — more becomes noise, not signal). This is the single highest
"looks beautiful and is actually more legible" change available: on a
6-level-deep API response, today every guide line is identical dim gray;
with depth tinting the eye can track "which level am I at" the way
indent-guide plugins do in editors.

- Both `render.rs` (`render_json_children`/`render_xml_children` already
  thread `depth` through — just pass it to the paint call instead of a
  fixed `Color::Structural`) and `tui/render.rs` (which also has `depth`
  on each `Line`) need this.
- Keep it *only* on the guide characters, not the key/value text — the
  point is a structural cue, not a rainbow of content.
- Add a snapshot-style test asserting depth 0 and depth 1 guide lines get
  different color codes (mirroring the existing
  `renders_json_tree_with_color` test pattern).

### A3. Search-match substring highlight (Priority: high, effort: low)

Right now finding a match only moves the cursor to that row; there's no
visual mark on *what* matched within the line, in either the main tree
view, the `/` incremental search, or the `F` popup list. Add
underline/reverse styling on exactly the matched substring (key or value
text) on:

1. the popup's listed matches (`tui/render.rs` popup rendering already
   has per-row match data available via `popup_match_entries`/
   `line_search_text` in `tui/keys.rs`),
2. the current line when `/` search is active.

This is cheap because the matching logic (`fuzzy_matches`,
`line_search_text`) already exists — this only adds a span split at the
matched range for styling, no new matching logic.

### A4. Semantic string subtypes (Priority: medium, effort: low-medium)

Cheap pattern checks on scalar string content to paint a distinct accent
color for recognizable shapes, without a general type-inference system:

- ISO 8601 timestamp (`^\d{4}-\d{2}-\d{2}[T ]`)
- UUID (`^[0-9a-f]{8}-[0-9a-f]{4}-...`)
- URL (`^https?://`)

Implement as a small ordered list of cheap prefix/regex checks in
`json_tree.rs`/`xml_tree.rs` next to the existing scalar-to-`Color`
mapping (`JsonScalar::color()` per the render.rs usage) — a new
`Color` variant per subtype (e.g. `Color::Timestamp`, `Color::Uuid`,
`Color::Url`), not a generic "detected type" struct. Skip anything beyond
these three unless a real fixture shows another shape worth calling out —
this is the kind of thing that's easy to over-build; three high-value,
unambiguous patterns and stop.

### A5. `--color=always|auto|never` (Priority: low, effort: trivial)

`use_color()` today only checks `is_terminal()` + `NO_COLOR`. Add an
explicit clap flag (clap's `ValueEnum`, same pattern as the existing
`FormatArg`) so output can be forced into `less -R`, a file, or a
non-interactive pipe that still wants color. `auto` preserves today's
behavior exactly; this is additive, no existing behavior changes.

### Explicitly deferred (don't build proactively)

- A user-configurable theme/palette file (`~/.config/treeq/config.toml`).
  Hardcoded palette is the right call until there's a real complaint about
  the specific colors chosen — this project's own history (34 closed
  issues, each solving one concretely-reported problem) is the pattern to
  keep following, not a config surface built ahead of demand.

---

## Part B — tmux / neovim / product features

Ordered by (impact × how directly it serves the "lives next to tmux and
neovim" positioning) ÷ effort. treeq currently has **zero** vim-style
keybindings and **zero** terminal-multiplexer or editor integration,
despite that being its natural habitat — this is the biggest open gap,
bigger than color.

### B1. Vim navigation: `h/j/k/l`, `gg`/`G`, `Ctrl-d`/`Ctrl-u`, `n`/`N`
**Priority: highest. Effort: trivial.**
Today navigation is arrows-only plus an awkward `g<digits><arrow>`
count-jump. Add vim aliases next to the existing `KeyCode::Up`/`Down`
arms in `tui/keys.rs::handle_key`; `gg`/`G` jump to first/last line;
`Ctrl-d`/`Ctrl-u` half-page; `n`/`N` repeat last search forward/backward
(reuses `jump_to_next_match`/`cycle_search_match`, which already exist).
No new state needed beyond what `count_buffer` already provides.

### B2. `--pick`: print selected path on Enter, then exit
**Priority: highest. Effort: low.**
The TUI is currently a dead end — quitting just exits. Add a mode where
Enter on a node prints its path (or, for JSON, the jq filter already
computed by `to_jq_path` in `tui/keys.rs`) to stdout and exits 0. This is
the load-bearing primitive for B3 and B4 below; without it neither tmux
nor neovim integration has anything to hook into.

### B3. tmux popup recipe (docs + `--pick` wiring)
**Priority: highest. Effort: trivial once B2 lands.**
Document (README, not new code beyond B2) a `bind-key` snippet using
`tmux display-popup -E` to launch treeq over the current pane, and show
how to pipe `--pick`'s stdout into `tmux send-keys`/`load-buffer` to hand
the selected path back to the invoking pane. This is what actually earns
the "tmux-native" claim.

### B4. Minimal Neovim plugin (single Lua file)
**Priority: high. Effort: medium.**
A `:Treeq` command that opens the current buffer's content in a floating
terminal running `treeq --pick <tmpfile>`, and on exit reads the printed
path and inserts/yanks it (v1) or jumps the cursor to that location (v2,
needs source byte-offset support — see B4a). Ship as one `.lua` file with
no options table until someone asks for configurability — this is the
single feature that actually differentiates treeq from `jless`/`otree`,
neither of which has editor integration.

- **B4a (stretch, higher effort):** track byte offset/line per node
  during parsing (`roxmltree` already exposes spans; `serde_json`
  with `preserve_order` does not by default and may need a
  position-tracking pass) so `:Treeq` can jump the source buffer's cursor
  to the exact location instead of just yanking a path string.

### B5. Shell completions (`clap_complete`)
**Priority: high. Effort: trivial.**
One added dependency, ~10 lines in `main.rs`: `treeq --generate
zsh|bash|fish`. Expected polish for a CLI in this ecosystem.

### B6. Publish to crates.io
**Priority: high. Effort: trivial.**
Currently Homebrew-tap-only (confirmed: no `treeq` crate exists yet).
`cargo install treeq` is the natural install path for the audience this
README already targets. No code change — `cargo publish` + a CI step.

### B7. Full-value viewer for long/multi-line strings
**Priority: medium. Effort: low-medium.**
`i` (inspect) shows type/size but nothing pages a truncated long string —
exactly the shape of treeq's target payloads (embedded stack traces,
stringified JSON blobs in log lines). Add a key (`v`, or reuse `i`) to
open the current leaf's full value in a scrollable popup.

### B8. Regex/exact search toggle
**Priority: medium. Effort: low.**
`/` is fuzzy-only (`fuzzy_matches` in `tui/keys.rs`). Add a toggle key to
switch matcher (fuzzy → substring → regex); all downstream consumers
(`jump_to_next_match`, the `F` popup, cycling) already operate on a single
predicate function, so this is a matcher swap, not new plumbing.

### B9. Mouse support (click to expand/collapse, wheel scroll)
**Priority: medium. Effort: low-medium.**
`EnableMouseCapture` isn't wired at all today. Ratatui/crossterm make
click-to-toggle and wheel-scroll nearly free by routing `MouseEvent`s into
the same handlers `Tab`/`Up`/`Down` already call. Lowers the barrier for
anyone demoing treeq to a non-vim teammate.

### B10. Batch-export tagged paths
**Priority: medium. Effort: low.**
Tags (`1`-`8`, `tui/keys.rs::toggle_tag`) already exist for "marking spots
mid-investigation" but die with the session. Add an exit key that dumps
every tagged path (optionally as jq filters) to stdout — turns an ad hoc
investigation into a reusable script. All state (`AppState.tags`) already
exists.

### B11. Structural `--diff <a> <b>`
**Priority: lower (bigger bet). Effort: high.**
Tree-level diff of two JSON/XML documents (added/removed/changed keys
highlighted). Real differentiator versus `jq`/`jless`/`otree`, but it's a
new diff algorithm plus a new render/TUI mode — scope this as its own
plan, not a quick add.

### B12. `--watch` / tail mode
**Priority: lower (bigger bet). Effort: high.**
Re-poll the file and refresh for a dedicated "watch this growing log"
tmux pane. Requires moving `tui.rs`'s event loop off a blocking
`event::read()` to a timeout-based poll — touches the core loop, not just
a keybinding, so needs its own design pass first.

### Explicitly deferred

- Lazy/paginated NDJSON parsing — only build once a real file size hits
  this; don't build ahead of a reported problem.
- Session persistence (remember collapse/tag state per file across runs)
  — nice, not needed until someone asks.

---

## Suggested order for a single agent picking this up

1. B1 (vim keys) → B2 (`--pick`) → B3 (tmux doc) — these three are
   cheap, independent, and together they're the whole "tmux-native"
   story.
2. A2 (depth tinting) → A3 (match highlight) → A1 (truecolor palette) —
   the color work, roughly in order of visual impact per line of code.
3. B5 + B6 (completions, crates.io) — trivial, do whenever, no
   dependencies on anything else here.
4. Everything else, opened as individual GitHub issues (this repo's
   established workflow) rather than batched, so each gets its own
   `feat/fix` branch and PR per `CONTRIBUTING.md`.
