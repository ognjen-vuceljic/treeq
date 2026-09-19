# treeq — terminal JSON/XML visualizer

Date: 2026-09-19
Status: approved, pre-implementation

## Problem

Valid JSON/XML that is minified, deeply nested, or just large is hard to
read in a terminal. There's no good way to see structure (objects, arrays,
elements, attributes, nesting) at a glance in a terminal session — including
inside a Claude Code session, where Claude only sees text returned from a
tool call.

Explicitly out of scope: repairing/recovering malformed input. Input is
assumed well-formed JSON or XML.

## Users

1. The author, interactively, in a terminal.
2. Claude Code, non-interactively, via CLI flags and plain-text stdout.
3. (Future) the wider open-source community.

## Architecture

Two independent format pipelines sharing a CLI shell and two renderer
families (tree, graph). No unified data model between JSON and XML — each
format gets its own node enum and its own renderer, since JSON
(objects/arrays/scalars) and XML (elements/attributes/text/mixed content)
don't map cleanly onto each other and forcing a shared model would be lossy
or surprising.

```
input (stdin | file arg)
  → sniff format: first non-whitespace byte '<' → XML, else JSON
    (or explicit --format json|xml)
  → parse:
      JSON → serde_json::Value
      XML  → roxmltree::Document
  → build format-specific tree:
      JsonNode { Object(Vec<(String, JsonNode)>), Array(Vec<JsonNode>), Scalar(String) }
      XmlNode  { name, attributes: Vec<(String,String)>, text: Option<String>, children: Vec<XmlNode> }
  → mode selection:
      TTY && !--static  → interactive TUI (ratatui)
      else              → static text renderer (respects --path/--depth)
      --graph           → render via Graphviz `dot` → PNG → image-protocol
                          blit (Kitty/iTerm2); falls back to static tree
                          + stderr notice if `dot` missing or terminal
                          doesn't support the image protocol
```

## Dependencies

Full-leverage approach — no hand-rolled parser. This project is a Rust
learning vehicle at the *plumbing* level (ownership, enums, error handling,
trait boundaries, CLI/TUI architecture), not at the parser-implementation
level.

- `serde_json` — JSON parsing
- `roxmltree` — XML parsing (read-only DOM)
- `ratatui` + `crossterm` — interactive TUI
- `clap` — CLI argument parsing
- Graph layout: shell out to system `dot` (Graphviz). No Rust graph-layout
  crate. This is an optional enhancement (`--graph`), not a v1 requirement,
  so depending on an external binary is acceptable — it degrades gracefully
  when absent.
- Image blitting: hand-rolled Kitty/iTerm2 escape sequences, or `viuer` if
  it turns out to fit cleanly — decided during implementation, not a
  blocking design decision.

## CLI surface

```
treeq [FILE]                 # stdin if omitted
  --path <dotted.path>       # print only the subtree at this path
  --depth <N>                # truncate tree at depth N
  --static                   # force non-interactive output even in a TTY
  --graph                    # render via Graphviz + image protocol
  --format json|xml          # override auto-detection
```

`--path` and `--depth` exist specifically so Claude Code can explore a large
document in slices (an overview at low depth, then drill into a path)
instead of dumping an entire tree into a tool result.

## Interactive TUI (v1 scope)

- Arrow-key navigation
- Collapse/expand nodes
- Status bar showing the current node's full path
- Incremental search that filters/jumps to matching keys/values

Explicitly deferred: animations, transitions, theming, clipboard yank. Add
once the core navigation feels right.

## Error handling

- Parse failure: clear message to stderr (with line/byte offset if the
  underlying parser provides one), non-zero exit. No repair attempted.
- `--graph` with `dot` missing, or terminal lacks image protocol support:
  one-line stderr notice, fall back to the static tree on stdout, exit 0.
  An optional enhancement failing must never be a hard failure.
- `--path` that doesn't resolve: error names the first path segment that
  failed to match.

## Testing

Parsing itself is delegated to trusted libraries (serde_json, roxmltree)
and isn't re-tested. Tests focus on treeq's own logic:

- Unit tests: `serde_json::Value` / `roxmltree::Document` → our node enums,
  across representative fixtures (nested objects, arrays, XML attributes,
  text, mixed content).
- Unit tests: `--path` / `--depth` slicing.
- One smoke test per static rendering mode (deterministic string output,
  asserted directly).
- TUI and graph/image rendering are not meaningfully unit-testable; verified
  manually instead of faked.

## Non-goals (v1)

- Malformed input repair/recovery.
- ASCII-rendered graph layouts (node-link diagrams drawn in plain text) —
  the image-protocol path via Graphviz covers the "real graph" use case;
  hand-rolling 2D graph layout in terminal cells is high effort, low value
  given the fallback already exists.
- Streaming/lazy parsing for huge files — target size is small-to-medium
  (KBs to low MBs) for now.
