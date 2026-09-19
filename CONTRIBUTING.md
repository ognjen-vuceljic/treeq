# Contributing

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
