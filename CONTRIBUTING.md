# Contributing

## Branches and PRs

- Branch names must be `<type>/<description>`, where `type` is one of:
  `feat`, `fix`, `chore`, `ci`, `docs`, `refactor`, `test`. Enforced by the
  `branch-name` CI check on every PR.
- `feat`/`fix` PRs must reference an issue in the description (e.g.
  `Closes #12`) — also CI-enforced. `chore`/`ci`/`docs`/`refactor`/`test`
  PRs don't require one, since they often don't have an associated issue.

## Code style

- Run `cargo fmt` and `cargo clippy` before committing; CI enforces both.
- Methods: max ~20 lines (enforced via `clippy::too_many_lines`, see `clippy.toml`).
- Structs/impl blocks: max ~50 lines. Not lint-enforceable — if an impl
  block grows past this, split the struct's responsibilities or move
  related impls into their own module. This is a review-time convention,
  not a CI check.
- Prefer small, focused modules (one per format pipeline / renderer) over
  large multi-purpose files.
- Use traits for polymorphism (e.g. a `Renderer` trait for tree/graph
  output) rather than large match statements on a mode enum.
- Builder-style fluent APIs (`.depth(3).path(x)`, methods taking `mut self`
  and returning `Self`) are preferred for structs with several optional
  config fields; skip this pattern for simple 1-2 field structs.

## Testing and coverage

- CI enforces a 95% line coverage minimum (`cargo llvm-cov --fail-under-lines
  95`) on every PR. Add unit tests alongside the code they cover
  (`#[cfg(test)] mod tests` in the same file) and integration tests in
  `tests/cli.rs` for CLI-facing behavior.
- `src/tui.rs` (not `src/tui/*.rs`) is excluded from the coverage report via
  `--ignore-filename-regex 'src/tui\.rs$'`. It contains only the raw
  terminal-I/O entry points (`enable_raw_mode`, `EnterAlternateScreen`, the
  blocking `event::read()` loop) which require a real TTY and can never run
  under `cargo test` or a piped subprocess. All TUI *logic* — state
  transitions, key handling, flattening, rendering — lives in
  `src/tui/*.rs` and is fully unit-tested, including rendered output via
  `ratatui::backend::TestBackend`.
- The same rationale applies to the two TUI-launch closures inside
  `run_json`/`run_xml` in `src/main.rs` (the `unwrap_or_else` around
  `tui::run_json_tui`/`run_xml_tui`): they only execute when stdout is a
  TTY, which is never true in a test harness. They are accepted as
  untested rather than excluded, since they're a small fraction of an
  otherwise well-covered file.
