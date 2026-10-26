# Scroll — living book 📜

![Scroll hero](assets/hero/scroll.svg)

> **one page per app, Tab teaches Grove — the last leaf opens PATH**

Scroll is how the house teaches itself. Each page is one creature: who she
is, how to live with her (keys included), which spells to type, and footer
runes that cast her. The last leaf is PATH — every tool on `$PATH`, printed
to your prompt so you may finish the line yourself.

For: Grove onboarding, one page per app. The Rust build is standalone; no
Pixie or faeOS Python libraries are required. The faeOS launcher prefers the
Rust release binary when available and keeps the Python app as a rollback.

## Look

![Scroll summon page](assets/screenshots/scroll-summon.png)
![Scroll pixie page](assets/screenshots/scroll-pixie.png)

```
 ___
|---|
|---|
|___|
```

## Status

The book has 35 static pages plus the egg leaf (`hermetic` or `kur`, based on
runtime hatch state) and the virtual PATH leaf: 37 leaves in Tab order.

The Rust app is `scroll-rs`; faeOS `bin/scroll` prefers
`~/.local/lib/faeos/scroll-rs`, `/usr/local/lib/faeos/scroll-rs`, then
`~/scroll/target/release/scroll-rs`, before trying the existing Python
fallbacks. When stdout is captured by the faeOS `summon` / `print -z` wrapper,
Rust draws through `/dev/tty` and writes only the selected command to stdout.
Set `SCROLL_BIN="$HOME/scroll/scroll"` to explicitly roll back to Python.

App information for Rust lives in `data/pages.json`. Keep it in curriculum
order with `src/model.rs`'s runtime egg pages; while the Python fallback is
retained, mirror curriculum updates in `scroll_pages.py` too.

Developers and AI agents should start with
[`docs/development.md`](docs/development.md) and the repository's
[`AGENTS.md`](AGENTS.md). Codex mirrors these technical references; this
repository remains their source of truth.

## Use

```bash
cargo build --release
./target/release/scroll-rs              # open the book on page one
./target/release/scroll-rs --path       # PATH leaf (same as summon)
./target/release/scroll-rs list         # every page as text (for grepping)
./target/release/scroll-rs -x <cmd>     # run a PATH match without the TUI
```

On every launch, Scroll presents a skippable animated welcome: a sigil forms,
the title letters resolve, and the rounded frame opens. Any key or mouse
input skips to the book; `q` exits. Set `SCROLL_REDUCED_MOTION=1` to skip the
splash and keep page changes static.

Tab and Shift+Tab move lessons with a short vertical page-turn: the next page
enters from below, and the previous page from above. The animation is
interruptible and reduced-motion safe.

Keys: `Tab` / `Shift-Tab` pages (`cycle_next` / `cycle_previous` in Grove's
canonical keyboard catalog) · `↑` / `↓` scroll lesson text · `PageUp` /
`PageDown` move by a screen · `1–9` runes · `Ctrl-F` searches lessons · `m`
ambience · `q` quit. Use the opening Scroll lesson as the quick orientation.

On PATH, type to filter, use `↑` / `↓` to select and `PageUp` / `PageDown` to
move by a screen, then Enter to print the selected command name. Double-click
also prints it. In a shell capture, the picker uses the controlling TTY and
keeps stdout reserved for that command name.
`--refresh` refreshes the shared
six-hour PATH cache. Refreshes replace the cache atomically; if a
refresh cannot be written, Scroll reports the error and uses the freshly
scanned PATH for the current run. An unreadable/corrupt cache is rebuilt.

## Build and parity

Rust uses a standalone Cargo manifest and does not import Pixie internals.
Egg state is read from the standard faeOS egg JSON files. Ambience is optional:
it needs the chiptune track and `mpv`; its IPC socket is kept in
`$XDG_RUNTIME_DIR` (or `~/.cache/faeos/runtime` if unset), never Siren's socket.

```bash
cargo fmt --check
cargo test
cargo build --release
python3 -m pytest tests/ -q
```

Rust tests cover page inventory/content shape, runtime egg states, navigation
and search, splash storyboard phases, vertical cycle direction/easing and
retargeting, reduced-motion behavior, PATH scanning/filtering/cache expiry and
write failures, and rune argument parsing.
zsh PTY checks cover the faeOS stdout-capture /
controlling-TTY contract, `print -z` inserting the picked command, and quitting
without changing the ZLE buffer. The Python fallback still requires its
existing faeOS `bin/` libraries.

The generated binding adapter is `src/grove_keys.rs`; regenerate from Grove
with `python3 ../faeOS/tools/sync-keyboard-shortcuts.py` and verify with
`--check`.

## License

MIT — see [LICENSE](LICENSE).
