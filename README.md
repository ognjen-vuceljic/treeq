# treeq

[![CI](https://github.com/ognjen-vuceljic/treeq/actions/workflows/ci.yml/badge.svg)](https://github.com/ognjen-vuceljic/treeq/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**A keyboard-driven tree view for JSON and XML, in your terminal.** Built
for humans who are tired of squinting at minified blobs, and for coding
agents that need to explore a large document without dumping the whole
thing into a tool result.

![treeq interactive TUI demo: piping JSON into treeq for a colorized static render, tagging nodes with the polished dim-tint palette, count-prefixed jumps with viewport scrolling, the inspect popup, the syntax-colored fzf-style search popup with Tab-cycling, and a key/value combo search](assets/demo.gif)

## Contents

- [Why treeq](#why-treeq)
- [Install](#install)
- [Usage](#usage)
- [Features](#features)
- [TUI keybindings](#tui-keybindings)
- [tmux integration](#tmux-integration)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)

## Why treeq

Minified, deeply-nested API responses and config files are hard to read in
a plain terminal. `treeq` parses JSON or XML and gives you a real,
navigable tree — either a static, scriptable render, or a fast interactive
TUI — instead of a wall of brackets.

`jq` is the tool for *transforming and extracting* JSON; `treeq` solves the
step before that: "what does this even look like?" — seeing the shape of
an unfamiliar payload before you write the `jq` filter that extracts what
you need. Think of it as `jq`'s companion, not its replacement, in the
niche also occupied by tools like `jless` or `otree` — but covering JSON
*and* XML in one tool, with CLI flags (`--path`, `--depth`, `--stats`,
`--agent`) built so a coding agent can explore a large document in bounded
slices instead of dumping the whole thing into a tool result.

## Install

**Homebrew:**

```sh
brew tap ognjen-vuceljic/treeq
brew install treeq
```

**From source** (requires a recent stable Rust toolchain, edition 2024):

```sh
git clone https://github.com/ognjen-vuceljic/treeq.git
cd treeq
cargo build --release
./target/release/treeq --help
```

## Usage

```sh
treeq file.json                       # interactive TUI in a terminal
cat file.json | treeq --static        # plain tree, for piping/scripting
treeq file.json --path user.address   # only that subtree
treeq file.json --depth 2             # truncate deep nesting
treeq file.json --stats               # size/shape summary, no full dump
treeq file.json --agent               # stats + shallow tree, in one call
treeq file.xml                        # XML works the same way
treeq --pick file.json                # Enter prints the selected path, then exits
treeq --generate zsh > ~/.zfunc/_treeq  # shell completions (zsh/bash/fish)
```

```sh
$ treeq --static sample.json
root
├── user
│   ├── name: "Alice"
│   ├── roles
│   │   ├── [0]: "admin"
│   │   └── [1]: "editor"
│   └── address
│       ├── city: "London"
│       └── zip: "E1 6AN"
└── active: true
```

In a real terminal, output is colorized by type (keys, strings, numbers,
booleans, null each a distinct color), and dropping `--static` opens the
interactive TUI instead.

### XML in action

XML works the same way as JSON, all through the same keybindings:

![treeq XML demo: navigating a catalog of books, inspecting a price field's type and path, fuzzy-searching for "price" and cycling through matches with Tab, tagging a node, then collapsing and expanding the whole tree](assets/xml-demo.gif)

## Features

- **JSON and XML**, auto-detected, plus YAML and NDJSON via explicit flags.
- **Search** — incremental fuzzy search (`/`, e.g. "nme" finds "name") for
  quick jumps, plus an fzf-style popup (`F`) listing *every* match across
  the whole document — including collapsed subtrees and truncated arrays —
  matching on key, value, or a fragment spanning both.
- **Fast navigation** — count-prefixed jumps (`g20↓`), collapse/expand one
  node, an ancestor chain, or everything at once.
- **8-color node tagging** (`1`-`8`) for marking spots mid-investigation,
  clearable in bulk (`x`).
- **Copy what you're looking at** — yank the current path (`y`, via OSC 52)
  or a ready-to-run jq filter (`Y`).
- **Inspect mode** (`i`) — a node's type, size, full path, and tag.
- **Pick mode** (`--pick`) — Enter prints the selected path (a jq filter for
  JSON, the dotted path otherwise) to stdout and exits, instead of leaving
  the TUI a dead end. The natural hook for
  [tmux integration](#tmux-integration) and scripting:
  `selected="$(treeq --pick file.json)"`.
- **Agent-friendly flags** — `--path`, `--depth`, `--stats`, `--paths`,
  `--schema`, and `--agent` (bundles stats + a shallow tree in one
  non-interactive call) for exploring a document in bounded slices instead
  of dumping it whole. `--paths` pairs naturally with
  [`fzf`](https://github.com/junegunn/fzf):
  ```sh
  treeq --path "$(treeq --paths file.json | fzf)" file.json
  ```
- **Type-aware colors**, auto-disabled when output isn't a terminal or
  `NO_COLOR` is set — and type stays recoverable from plain text alone
  (strings quoted, numbers/booleans/null bare), for piped output,
  colorblind users, and agents reading color-stripped text.
- **Shell completions** — `treeq --generate zsh|bash|fish` prints a
  completion script to stdout.

## TUI keybindings

| Key | Action |
|---|---|
| `↑` / `↓` | Move cursor |
| `j` / `k` | Move cursor (vim alias) |
| `g` | Count-prefixed jump: type digits, then `↑`/`↓` to move that many lines |
| `gg` / `G` | Jump to the first / last line (`g5g` or `g5G` jumps to an absolute line) |
| `Tab` / `Space` | Collapse/expand current node |
| `h` / `l` | Collapse current node (or jump to its parent if already collapsed) / expand current node (vim alias) |
| `Backspace` | Collapse nearest parent, move cursor there |
| `Shift+C` | Collapse every ancestor up to the root |
| `c` | Collapse all |
| `e` | Expand all |
| `/` | Incremental fuzzy search |
| `n` / `N` | Repeat the last search forward / backward |
| `F` | Open an fzf-style popup listing every match across the whole document (`Tab`/`Shift+Tab` cycles, `Enter` jumps, `Esc` closes) |
| `i` | Inspect the current node: type, size, full path, and tag |
| `1-8` | Tag/untag current node with a highlight color |
| `x` | Clear every active tag at once |
| `y` | Yank current node's dotted path to clipboard (OSC 52) |
| `Y` | Yank current node as a jq filter, e.g. `.user.tags[0]` (JSON/YAML only) |
| `?` | Toggle the in-app keybinding help overlay |
| `q` / `Esc` | Quit |

The status bar shows a `?: help` hint whenever it isn't displaying a
search prompt or a status message, so you don't need to remember this
table while using the TUI.

## tmux integration

`--pick` makes treeq a natural fit as a tmux popup: pop it up over the
current pane, pick a path, and hand it straight back to whatever you were
doing — no separate window, no copy-pasting from a scrollback.

Add a binding like this to `~/.tmux.conf` (prefix `T` opens the popup on
the file `treeq.json` in the current directory; adjust the path/binding to
taste):

```tmux
bind-key T display-popup -E -w 80% -h 80% \
  "treeq --pick treeq.json > /tmp/treeq-picked || true"
```

`display-popup -E` runs the command in a real pty over the current pane
and closes the popup when it exits, so treeq's own `/dev/tty` rendering
(see [Pick mode](#features)) works exactly as it would in a normal
terminal — the popup's foreground process just happens to be `treeq`
instead of your shell.

Since `display-popup` doesn't hand a command's stdout back to the
invoking pane directly, redirect `--pick`'s output to a file (or a tmux
buffer) and consume it from a second binding once the popup closes:

```tmux
# Send the last picked path to the active pane as if it were typed.
bind-key P run-shell 'tmux send-keys -t "#{pane_id}" "$(cat /tmp/treeq-picked 2>/dev/null)"'

# Or load it straight into the tmux paste buffer instead.
bind-key T display-popup -E -w 80% -h 80% \
  "treeq --pick treeq.json | tmux load-buffer - || true"
```

The second popup binding above pipes `--pick`'s stdout directly into
`tmux load-buffer -`, which works because the pipe (not the popup's tty)
is what treeq's `/dev/tty`-based rendering leaves untouched — paste it
back into any pane with tmux's normal paste-buffer binding (`prefix ]`).

## Roadmap

Open design/feature ideas are tracked as
[GitHub issues](https://github.com/ognjen-vuceljic/treeq/issues) — covering
TUI ergonomics, agent-friendliness, and format support. Contributions and
issue discussion welcome.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for code style and conventions.
`cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be clean,
and `cargo test` must pass — CI enforces both on every PR.

## License

[MIT](LICENSE)
