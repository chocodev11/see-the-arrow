# AGENTS.md - see-the-arrow

see-the-arrow shows a human where to click: an arrow with a sign above every window, click-through,
never takes the focus, ends by itself. Windows 10/11 and Linux on X11, Rust edition 2024. Its
command, `bigarrow`, is command-line compatible with bigarrow for macOS
(https://github.com/franzenzenhofer/big-arrow-on-the-screen); README.md lists where the platforms
make it differ. Keep that compatibility: subcommands, flags, defaults, messages, exit codes, JSON.

## Layout

- `src/model/`: pure code, no OS calls besides files. Parsing, geometry (`arrow`, `spiral`),
  placement, layout, window and element matching, the registry of live arrows, JSON results.
- `src/render/`: tiny-skia drawing used by the overlay, `--png` and the tests. `text` lays out
  the sign with parley (copy buttons are inline boxes); fonts are bundled in `assets/fonts`.
- `src/cli/`: clap options and validation (`config`), `runner` (raise, resolve, plan, PNG, dry run,
  detach), `session` (one arrow on screen as a state machine), the other subcommands.
- `src/platform/`: `windows/` (layered windows, DWM, UI Automation, SAPI) and `linux/` (x11rb,
  AT-SPI over zbus without tokio). Both export the same names; `platform/mod.rs` holds the shared types.

## Rules

- Layout runs in display-local points, so the look constants hold on every scale; coordinates in and out are
  global input units (physical pixels on Windows, root pixels on X11). Convert with `Display`.
- Drawing needs no permission. Never steal the focus: no activation, no synthetic input.
- Every arrow ends by itself (duration, `stop`, owner exit, display gone). No default lifetime
  may outlive the agent session.
- The session never blocks: UI Automation and AT-SPI calls that can hang run on another thread.
- `cargo test` must never draw on the screen; tests that do are gated by `BIGARROW_SCREEN_TESTS=1`.
- Gates before every commit: `cargo clippy --all-targets -- -D warnings`, `cargo build --release`,
  `cargo test`, on Windows and in a Linux X11 session.
