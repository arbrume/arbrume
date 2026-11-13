# Arbrume coding rules (for AI and humans)

Formatting and linting are mandatory. Before every commit, run and fix until clean:

    cargo fmt --all
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    ruff format && ruff check --fix
    python3 tools/check_comments.py .

CI runs the same checks in --check mode and rejects any warning.

- Never add `#[allow(...)]`, `# noqa`, or raise a threshold to get past a lint. Fix the code instead.
  If a suppression is truly needed, add it at the narrowest scope with a comment explaining why.
- Functions are capped at 100 code lines (Rust) / 100 statements (Python), 7 arguments, nesting depth 5.
  When a function nears the cap, split it into named steps, don't compress it.

## Comments

- A comment inside code is at most 2 lines (CI enforces this with `tools/check_comments.py`).
  Longer explanations go in doc headers: `//!` for a module, `///` for an item, docstrings in Python.
- A comment says what the code does or why it is that way, and must make sense to a reader with
  only the code. Never write history or conversation narrative ("changed X to Y", "now we",
  "as discussed", "fixed the bug where", "previously"). That belongs in the commit message.

## Commits

- No `Co-Authored-By` trailer, no "Generated with" line, no model names in commits or PRs.
- Sign off every commit (`git commit -s`, DCO).
- Public commits never mention private planning material.

## Layout

Cargo workspace under `crates/`: `arbrume-core` (plant graph, RNG, curves, species schema,
mesh buffers), `arbrume-grow` (growth cycle), `arbrume-mesh` (geometry), `arbrume-export`
(`.glb` and sidecars), `arbrume-cli` (batch generation), `arbrume-py` (PyO3 bindings).
Output depends only on (species, parameters, seed, version), never on thread count: random
numbers come from the counter-based generator in `arbrume-core`, keyed by stable ids.
