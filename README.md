# treeq

[![CI](https://github.com/ognjen-vuceljic/treeq/actions/workflows/ci.yml/badge.svg)](https://github.com/ognjen-vuceljic/treeq/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Fast, readable JSON/XML tree visualization in the terminal — for you, and
for coding agents.

Minified, deeply-nested API responses and config files are hard to read in
a plain terminal. `treeq` parses JSON or XML and gives you a real tree —
either a static, scriptable render, or an interactive TUI — instead of a
wall of brackets.

## See it

```sh
$ treeq --static sample.json
root
├── user
│   ├── name: Alice
│   ├── roles
│   │   ├── [0]: admin
│   │   └── [1]: editor
│   └── address
│       ├── city: London
│       └── zip: E1 6AN
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

A terminal recording/GIF would round this section out properly — tracked as
a follow-up, since it needs to be captured from an actual running session
rather than generated.

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
- **Interactive TUI** — arrow-key navigation, collapse/expand (`Enter`/`Space`),
  collapse-all/expand-all (`c`/`e`), incremental search (`/`), yank current
  path to clipboard (`y`, via OSC 52 — works over SSH).
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
- **`--ndjson`** — treat input as NDJSON / JSON Lines (one JSON value per
  line, as produced by `kubectl`, `docker`, and many log streams) and view
  it as an array of records. JSON only; combining it with `--format xml`
  is an error.

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
| `Enter` / `Space` | Collapse/expand current node |
| `c` | Collapse all |
| `e` | Expand all |
| `/` | Incremental search |
| `y` | Yank current node's path to clipboard (OSC 52) |
| `q` / `Esc` | Quit |

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
