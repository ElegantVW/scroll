# Scroll changelog

## v1.2.0 — Grove first-run experience (2026-10-26)

- Reframed Scroll's opening lesson as a first-time Grove guide, with clear
  controls and paths to lesson search, runes, and PATH.
- Added rounded panels and a restrained semantic palette, with ASCII-border
  and `NO_COLOR` fallbacks.
- Added a skippable sigil/title/frame splash and an eased vertical push for
  next/previous lesson cycles, both with a reduced-motion path.
- Added canonical developer and AI-agent guidance for Codex to harvest.
- Replaced Rust `j`/`k` scrolling and `g`/`G` jumps with arrows and
  PageUp/PageDown, while retaining the Grove Tab / Shift+Tab cycle.
- Kept the Python implementation as a minimal rollback rather than promising
  visual or keyboard parity.

## v1.1.0 — Rust migration (2026-10-26)

- Added standalone Rust TUI with embedded curriculum data, searchable app index and quick-jump.
- Reconciled Rust app pages with current faeOS app docs and command surfaces.
- Preserved PATH filtering, the shared six-hour cache format, name output, matching execution, runtime egg/Kur state, mouse input, and optional per-app ambience.
- Made cache replacement atomic and refresh/write failures visible; malformed or stale cache data is rebuilt.
- Rust now draws on the controlling TTY when stdout is captured, reserving stdout for only the PATH selection; zsh PTY checks verified captured output, `print -z` insertion, and quit-without-edit behavior.
- faeOS `bin/scroll` prefers the release binary and retains the Python launcher as a `SCROLL_BIN` rollback.
- Tab and Shift-Tab briefly accent the page-title glyph; `SCROLL_REDUCED_MOTION=1` preserves static titles without changing page content or PATH selection.

## v1.0.0 1nk (2026-10-11)

- First stable number. Living book: 35 pages, Tab teaches, last leaf opens PATH. Read-only web reader on Grove.
