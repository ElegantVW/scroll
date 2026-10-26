# Scroll changelog

## v1.1.0 — Rust migration (2026-10-26)

- Added standalone Rust TUI with embedded curriculum data, searchable app index and quick-jump.
- Reconciled Rust app pages with current faeOS app docs and command surfaces.
- Preserved PATH filtering, the shared six-hour cache format, name output, matching execution, runtime egg/Kur state, mouse input, and optional per-app ambience.
- Made cache replacement atomic and refresh/write failures visible; malformed or stale cache data is rebuilt.
- Rust now draws on the controlling TTY when stdout is captured, reserving stdout for only the PATH selection; zsh PTY checks verified captured output, `print -z` insertion, and quit-without-edit behavior.
- faeOS `bin/scroll` prefers the release binary and retains the Python launcher as a `SCROLL_BIN` rollback.

## v1.0.0 1nk (2026-10-11)

- First stable number. Living book: 35 pages, Tab teaches, last leaf opens PATH. Read-only web reader on Grove.
