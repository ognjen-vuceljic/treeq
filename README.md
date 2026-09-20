# treeq

[![CI](https://github.com/ognjen-vuceljic/treeq/actions/workflows/ci.yml/badge.svg)](https://github.com/ognjen-vuceljic/treeq/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**A keyboard-driven tree view for JSON and XML, in your terminal.** Built
for humans who are tired of squinting at minified blobs, and for coding
agents that need to explore a large document without dumping the whole
thing into a tool result.

Minified, deeply-nested API responses and config files are hard to read in
a plain terminal. `treeq` parses JSON or XML and gives you a real,
navigable tree — either a static, scriptable render, or a fast interactive
TUI — instead of a wall of brackets.

**Recently added:** an fzf-style popup that searches the *entire* document
at once — including collapsed subtrees and truncated arrays — an
8-color tagging palette, and vim/tmux-style count-prefixed jumps
(`g5` + `↓` moves 5 lines down). See [TUI keybindings](#tui-keybindings)
below.

## See it

![treeq interactive TUI demo: collapsing/expanding, tagging nodes with colors, yanking a path, count-prefixed jumps, the fzf-style search popup, and full-document search reaching a collapsed subtree](assets/demo.gif)

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
booleans, null each a distinct color) and dropping `--static` opens the
interactive TUI instead. `--stats` sizes a document up before you dig in:

```sh
$ treeq --stats sample.json
input_bytes: 162
max_depth: 3
objects: 3
arrays: 1
scalars: 6
```

## Why not just `jq`?

`jq` is the tool for *transforming and extracting* JSON — its query
language is mature, ubiquitous, and something coding agents already know
cold. `treeq` doesn't compete with that. It solves the step *before*: "what
does this even look like?" — seeing the shape and structure of an
unfamiliar payload, visually, before you write the `jq` filter that
extracts what you actually need. Think of it as `jq`'s companion, not its
replacement, in the same spirit that a tool like `jless` or `otree`
occupies the "look at it" niche next to `jq`'s "query it" niche.

Where `treeq` differs from those: JSON *and* XML in one tool, colorized
type-aware output, and CLI flags (`--path`, `--depth`, `--stats`,
`--paths`) designed so a coding agent can explore a large document in
bounded slices instead of dumping the whole thing into a tool result.

## Features

- **Two input formats**, auto-detected: JSON and XML.
- **Search that actually finds things** — incremental fuzzy search (`/`,
  matches non-contiguous characters in order, e.g. "nme" finds "name") for
  quick one-at-a-time jumps, plus an fzf-style popup (`F`) that lists
  *every* match across the whole document in one screen — including
  nodes buried under a collapsed ancestor or past an array's 200-item
  preview limit — and jumps straight there on `Enter`, auto-expanding
  whatever was in the way.
- **Fast keyboard navigation** — arrows to move, `g` for vim/tmux-style
  count-prefixed jumps (type digits, then `↑`/`↓` — `g20↓` jumps 20 lines
  at once), `Tab`/`Space` to collapse/expand, `Backspace` to collapse the
  nearest parent, `Shift+C` to collapse every ancestor in one keystroke,
  `c`/`e` to collapse/expand everything.
- **8-color node tagging** (`1`-`8`) for marking up spots you care about
  mid-investigation — dim, low-saturation background tints layered on top
  of the type-based syntax colors, tuned to stay readable rather than
  clashing with them, plus bold keys for an extra visual anchor.
- **Copy what you're looking at** — yank the current node's dotted path to
  the clipboard (`y`, via OSC 52 — works over SSH with no extra config),
  or as a ready-to-run jq filter (`Y`, e.g. `.user.tags[0]`, JSON/YAML
  only).
- **Static, scriptable output** (`--static`) for piping into other tools or
  reading in a Claude Code / agent tool result.
- **`--path <dotted.path>`** — render only a subtree.
- **`--depth <N>`** — truncate the tree at a given depth.
- **`--stats`** — structural summary (max depth, node-kind counts, input
  size) instead of a full dump, for sizing up a document before drilling in.
- **`--agent`** — bundles `--stats` and a shallow `--static` tree render
  (default depth 3, or your explicit `--depth`) into one non-interactive
  call, for agents that would otherwise chain `--stats` then `--depth` then
  `--path` across multiple invocations. Composes with `--path`.
- **`--paths`** — flat list of every path in the document, one per line —
  pairs naturally with [`fzf`](https://github.com/junegunn/fzf):
  ```sh
  treeq --path "$(treeq --paths file.json | fzf)" file.json
  ```
- **`--schema`** — inferred shape summary (field names and types) instead
  of a full dump, for sizing up an unfamiliar document before writing a
  `jq` filter against it:
  ```sh
  $ treeq --schema sample.json
  name: string
  roles: array<string>
  address: object
    city: string
    zip: string
  active: boolean
  ```
- **Type-aware colors** (keys, strings, numbers, booleans, null each
  distinct), auto-disabled when output isn't a terminal or `NO_COLOR` is set.
- **Type recoverable from plain text alone**, not just color: strings are
  quoted (`"Alice"`, with embedded `"` and `\` backslash-escaped),
  numbers/booleans/null stay bare (`30`, `true`, `null`) — readable in
  piped output, by colorblind users, and by agents that see color-stripped
  text.
- **`--ndjson`** — treat input as NDJSON / JSON Lines (one JSON value per
  line, as produced by `kubectl`, `docker`, and many log streams) and view
  it as an array of records. JSON only; combining it with `--format xml`
  is an error.
- **`--array-limit <N>`** — truncate JSON (and YAML) arrays in `--static`
  output to their first `N` elements, folding the rest into `… (M more)`.
  Off by default (arrays render in full); not supported for XML input
  (which has no array concept), and combining the two is an error. In the
  interactive TUI, arrays longer than 200 elements are always previewed
  this way, with a summary line you can `Tab`/`Space` to expand back to
  every element — collapsing the array again resets it to the preview.

## Install

Not yet published to crates.io or a package manager. For now, build from
source:

```sh
git clone https://github.com/ognjen-vuceljic/treeq.git
cd treeq
cargo build --release
./target/release/treeq --help
```

Requires a recent stable Rust toolchain (edition 2024).

## Usage

```sh
treeq file.json                       # interactive TUI in a terminal
cat file.json | treeq --static        # plain tree, for piping/scripting
treeq file.json --path user.address   # only that subtree
treeq file.json --depth 2             # truncate deep nesting
treeq file.json --stats               # size/shape summary, no full dump
treeq file.json --agent               # stats + shallow tree, in one call
treeq file.xml                        # XML works the same way
```

## TUI keybindings

| Key | Action |
|---|---|
| `↑` / `↓` | Move cursor |
| `g` | Count-prefixed jump: type digits, then `↑`/`↓` to move that many lines |
| `Tab` / `Space` | Collapse/expand current node |
| `Backspace` | Collapse nearest parent, move cursor there |
| `Shift+C` | Collapse every ancestor up to the root |
| `c` | Collapse all |
| `e` | Expand all |
| `/` | Incremental fuzzy search |
| `F` | Open an fzf-style popup listing every match across the whole document (`Enter` jumps, `Esc` closes) |
| `i` | Inspect the current node: type, size, full path, and tag |
| `1-8` | Tag/untag current node with a highlight color |
| `y` | Yank current node's dotted path to clipboard (OSC 52) |
| `Y` | Yank current node as a jq filter, e.g. `.user.tags[0]` (JSON/YAML only) |
| `?` | Toggle the in-app keybinding help overlay |
| `q` / `Esc` | Quit |

The status bar shows a `?: help` hint whenever it isn't displaying a
search prompt or a status message, so you don't need to remember this
table while using the TUI.

## Roadmap

Open design/feature ideas are tracked as
[GitHub issues](https://github.com/ognjen-vuceljic/treeq/issues) — covering
TUI ergonomics (ancestor collapse, range-collapse for large arrays,
multi-color highlighting), agent-friendliness (`--schema`, NDJSON support,
better parse-error context), YAML support, and a `jq`-filter export for
the current node.

Contributions and issue discussion welcome.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for code style and conventions.
`cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be clean,
and `cargo test` must pass — CI enforces both on every PR.

## License

[MIT](LICENSE)
