# Scroll agent guidance

Scroll is Grove's user-facing app guide. Its developer reference is
[`docs/development.md`](docs/development.md); consult it before changing
navigation, curriculum, rendering, animation, or shell integration.

## Source of truth

- Rust UI and terminal behavior: `src/ui.rs`
- CLI and launch dispatch: `src/main.rs`
- Static lessons/order: `data/pages.json`
- Runtime leaves and lesson model: `src/model.rs`
- PATH scanning/cache: `src/path_index.rs`
- Grove key adapter: generated `src/grove_keys.rs`; edit the Grove catalog,
  then regenerate, never hand-edit this file.

Keep `scroll_pages.py` a working rollback and keep shared curriculum facts
accurate. Do not require Python to duplicate Rust-only animation or controls.

## Invariants

- Grove owns shared Tab / Shift+Tab lesson cycling.
- `SCROLL_REDUCED_MOTION=1` skips splash and page-transition animation; never
  block the event loop with sleeps.
- Preserve terminal cleanup and the rounded/color plus ASCII/no-color paths.
- When stdout is captured, draw the UI on `/dev/tty` and write only the
  selected PATH command to stdout. No debug output or animation data belongs
  on stdout.
- Keep mouse hit geometry tied to the settled destination page; do not trigger
  stale hit boxes while the page is moving.
- Keep canonical documentation in this repository. Codex mirrors the docs;
  do not make Codex's copy the only maintained source.

## Required checks

```bash
cargo fmt --check
cargo test --offline --locked
cargo build --release --offline --locked
python3 -m pytest tests/ -q
python3 ../faeOS/tools/sync-keyboard-shortcuts.py --check
```

For changes to terminal behavior, verify both motion-enabled and reduced-motion
PTY flows, including command-only captured stdout. Never commit or push unless
the user explicitly asks.
