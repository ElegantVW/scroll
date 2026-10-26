# Scroll development guide

Scroll is Grove's user-facing guide to its apps. The Rust application is the
canonical interactive implementation; Codex's Library mirrors this guide and
the repository's agent instructions for maintainers and AI agents.

## Architecture and sources of truth

- `src/main.rs` owns CLI parsing, help, and dispatch between list, PATH, and
  interactive modes.
- `src/ui.rs` owns the Crossterm event loop, terminal lifecycle, rendering,
  colors, panels, splash, vertical page transitions, hit targets, and shell
  command output.
- `src/model.rs` defines app pages and runtime egg leaves. Static curriculum
  data is embedded from `data/pages.json`.
- `src/path_index.rs` scans and caches PATH entries and implements matching,
  filtering, and cache replacement.
- `src/grove_keys.rs` is generated. Grove owns the binding catalog; do not
  edit this adapter manually.
- `scroll_pages.py` is the minimal Python rollback. When shared curriculum
  facts change, keep its content accurate, but it is not required to duplicate
  Rust-only animations or input behavior.

Keep lesson order in `data/pages.json` and the model's runtime leaves aligned.
The PATH leaf remains last; appending a lesson should not silently reorder
existing pages.

## Interaction contract

- Tab and Shift+Tab cycle lessons through the generated Grove
  `cycle_next`/`cycle_previous` predicates. Regenerate with
  `python3 ../faeOS/tools/sync-keyboard-shortcuts.py` and verify with
  `--check`.
- Up/Down scroll lesson text or select a PATH/search result. PageUp/PageDown
  move a viewport. Mouse wheel remains optional convenience.
- The splash is a short, skippable startup animation. Reduced motion is
  selected with `SCROLL_REDUCED_MOTION=1`; it must never add a blocking sleep.
- Page changes use a short vertical push: next moves upward from below;
  previous enters from above. Keep the render loop event-responsive and return
  to the idle polling cadence when motion ends.
- `NO_COLOR` disables color. `SCROLL_UNICODE=ascii` selects ASCII borders and
  glyphs.

## Rendering and geometry

The UI builds plain `String` rows, measures/wraps them using Unicode display
width, and derives mouse hit boxes from that geometry. Color is applied only
in `draw`; never insert ANSI escape sequences into measured lesson text.
Transitions move rendered rows without changing destination-page hit boxes.
Do not dispatch clicks against translated hit boxes during a transition.
Keep content, rune labels, and PATH selection stable while animating the
chrome or switching lessons.

## Terminal and shell contract

`TerminalGuard` restores raw mode, cursor visibility, mouse capture, and the
alternate screen on every exit path. If stdout is captured, the interactive
UI writes to `/dev/tty` while stdout carries only the selected PATH command
for the shell's `print -z` insertion. Do not log, print splash frames, or emit
diagnostics to stdout in interactive selection mode.

The faeOS and user-local `scroll` launchers prefer the Rust release executable
and retain the Python script as an explicit rollback. Verify the active
launcher as well as the crate when changing launch behavior.

## Validation

From this repository:

```bash
cargo fmt --check
cargo test --offline --locked
cargo build --release --offline --locked
python3 -m pytest tests/ -q
```

From the Grove checkout:

```bash
python3 tools/sync-keyboard-shortcuts.py --check
```

When changing TTY setup, output routing, PATH selection, splash interruption,
or page transitions, run a PTY check for both motion-enabled and
`SCROLL_REDUCED_MOTION=1` behavior. Verify normal rounded/color rendering,
ASCII/no-color fallback, resize/exit cleanup, and that captured stdout is
exactly the selected command.

## Codex reference

Codex harvests root `AGENTS.md` and Markdown in `docs/` from this repository
into `~/codex/trees/scroll/`. Keep this repository canonical; update the Codex
mirror using its harvest/sync workflow rather than editing the mirror alone.
The root `AGENTS.md` is agent guidance and is deliberately excluded from the
public guest pack.
