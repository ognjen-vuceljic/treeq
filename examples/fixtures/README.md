# Test fixtures

Graduated JSON/XML fixtures for manually exercising treeq, from trivial to
"throw everything at it at once". Each level has a matching `.json` and
`.xml` file testing the same shape of complexity where it makes sense.

| Level | File | What it stresses |
|-------|------|-------------------|
| 1 | `level-1-flat` | A flat object, no nesting. Sanity check. |
| 2 | `level-2-flat-array` / `level-2-attributes` | A top-level array of scalars; `null`; XML attributes + repeated sibling elements. |
| 3 | `level-3-one-nest` | One level of object nesting (`user.address`). |
| 4 | `level-4-mixed-types` / `level-4-mixed-content` | Arrays of objects, booleans + null side by side; XML mixed content (text + child elements interleaved). |
| 5 | `level-5-array-of-objects` / `level-5-array-of-elements` | A moderate-size (20-item) array/repeated-element list, each with its own nested array field. |
| 6 | `level-6-deep-config` / `level-6-deep-namespaced` | 5-7 levels of nesting (a realistic config tree); XML adds namespaces and self-closing empty elements. |
| 7 | `level-7-large-array` / `level-7-large-catalog` | A wide array/element list (150 items), ~1200-2000 lines — tests scrolling/perf in the TUI and `--stats`/`--paths` on real volume. |
| 8 | `level-8-deep-recursive` | A binary comment-thread tree ~20 levels deep (2 branches per level) — tests deep nesting rendering, collapse/expand, and `--depth` truncation. |
| 9 | `level-9-very-wide` | 3000 sibling keys/elements at shallow depth — tests breadth rather than depth (a very long flat list to scroll/search through). |
| 10 | `level-10-stress-test` | Everything at once: ~20-level-deep random branching tree, empty objects/arrays, very long strings, huge arrays, unicode (Japanese, accented, emoji), and special/escaped characters (newlines, tabs, quotes, `CDATA`, HTML-like entities). The adversarial case. |

## Suggested manual test pass

For each file, worth trying:

```bash
treeq examples/fixtures/json/level-N-*.json                  # interactive TUI
treeq --static examples/fixtures/json/level-N-*.json         # static tree
treeq --stats examples/fixtures/json/level-N-*.json          # shape summary
treeq --paths examples/fixtures/json/level-N-*.json | fzf     # fzf piping
treeq --schema examples/fixtures/json/level-N-*.json         # inferred schema
treeq --depth 2 examples/fixtures/json/level-N-*.json         # truncation
```

Things worth specifically checking at the higher levels:
- **Level 7/9** (wide): TUI scroll performance, search (`/`) responsiveness,
  `c`/`e` collapse/expand-all on a list this size.
- **Level 8** (deep): does `--depth` truncate cleanly at every level? Does
  collapse/expand work correctly many levels deep?
- **Level 10** (stress): unicode rendering/alignment in the TUI, long
  single-line values not breaking the layout, empty object/array display,
  and that special characters in values don't corrupt the tree rendering.
