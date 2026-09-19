# treeq

Fast, readable JSON/XML tree visualization in the terminal — for you, and
for coding agents.

Minified, deeply-nested API responses and config files are hard to read in
a plain terminal. `treeq` parses JSON or XML and gives you a real tree —
either a static, scriptable render, or an interactive TUI — instead of a
wall of brackets.

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
- **`--paths`** — flat list of every path in the document, one per line —
  pairs naturally with [`fzf`](https://github.com/junegunn/fzf):
  ```sh
  treeq --path "$(treeq --paths file.json | fzf)" file.json
  ```
- **Type-aware colors** (keys, strings, numbers, booleans, null each
  distinct), auto-disabled when output isn't a terminal or `NO_COLOR` is set.

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
treeq file.xml                        # XML works the same way
```

## Roadmap

Open design/feature ideas are tracked as
[GitHub issues](https://github.com/ognjen-vuceljic/treeq/issues) — covering
TUI ergonomics (ancestor collapse, range-collapse for large arrays,
multi-color highlighting), agent-friendliness (`--schema`, `--agent`,
NDJSON support, better parse-error context), YAML support, and a
`jq`-filter export for the current node.

Contributions and issue discussion welcome.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for code style and conventions.
`cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be clean,
and `cargo test` must pass — CI enforces both on every PR.

## License

[MIT](LICENSE)
