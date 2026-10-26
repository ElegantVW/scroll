# Scroll — living book (page carousel)

Canonical repo: `ElegantVW/scroll`. faeOS keeps only a thin launcher.

**Role:** One **page per app**; Tab teaches faeOS; last leaf is PATH (print to prompt).
**Status:** 35 static pages + egg leaf + PATH leaf = 37 leaves (switcher + tv added 2026-09-28).

## Navigation
- **Tab / Shift-Tab** — next / previous page (curriculum order)
- **1–9** — cast footer runes (**run** the spell)
- **g / G + digit** — vim-style page jump (11–20 / 21–30)
- **j/k · wheel** — scroll within a long page
- **PATH leaf** — filter · Enter → print name for shell `print -z`
- Layout: near-fullscreen panels with small margin so box edges show

## Curriculum
Python fallback order is in `CURRICULUM` in `scroll_pages.py`; Rust order and
app/command copy lives in `data/pages.json`. Both end with **switcher**, **tv**,
then the **Sealed Leaf** (or **Kur** if hatched), then **PATH**. Keep both
copies aligned while the Python rollback remains supported. Appending new pages
at the end keeps existing g/G jump positions stable.

## Rust app and launcher
- Build with `cargo build --release`; launch `target/release/scroll-rs`.
- `Ctrl-F` opens the searchable app index and quick-jump.
- The Rust app is self-contained: it reads faeOS egg state directly and
  optionally uses its own mpv ambience socket. It does not import Pixie
  internals or edit faeOS shared libraries.
- faeOS `bin/scroll` prefers Rust from the installed faeOS locations or
  `~/scroll/target/release/scroll-rs`, then falls back to Python. `bin/summon`
  remains the Python argv/env adapter that selects the PATH leaf.
- For shell capture, Rust draws through `/dev/tty` while stdout contains only
  the selected command; a PTY test verifies this contract. This lets the
  `pixie.zsh` `summon` and `scroll --path` wrappers continue to use `print -z`.
- Roll back to Python for a command with
  `SCROLL_BIN="$HOME/scroll/scroll" summon` (or `scroll --path`).
- Rust preserves the shared `~/.cache/pixie/summon.list` format and six-hour
  TTL; cache writes are atomic and refresh errors are reported.
- Rust page text is maintained in `data/pages.json` against current faeOS
  app/CLI documentation. Keep `scroll_pages.py` aligned until the Python
  rollback is retired.

## Sealed Leaf / egg
- Story: an **egg in the Menagerie** to hatch; points at **`murmur`** (babbling glass).
- No hand-holding, no true name, no solve path in docs.
- Leaf turns only after the player completes hatch tasks (not from murmur alone).

## Python fallback runtime deps (house libs, stay in faeOS)
`fae_termart` (frames), `fae_egg` (hatch state), `fae_ambience` (book music).
`scroll` puts its own dir first on `sys.path`, then `~/bin`.
