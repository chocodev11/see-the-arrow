# see-the-arrow

**Your agent found the button. Now it can show you.**

![Windows 10 | 11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4)
![Linux X11](https://img.shields.io/badge/Linux-X11-FCC624)
![Rust 2024](https://img.shields.io/badge/Rust-2024-B7410E)
![License MIT](https://img.shields.io/badge/license-MIT-green)

![The Windows 11 Control Panel over the default wallpaper, with seven arrows pointing in from the free space around the window: a black sign "Still here in 2026" at the window corner, a red "Firewall: still in here" with a close button on System and Security, a purple zigzag "Device Manager is in here" on Hardware and Sound, a green box "Same list as appwiz.cpl" on Uninstall a program, an orange zigzag "UTF-8 lives in Region" on Clock and Region, a pink ring "Large icons: one page" on the View by menu, and a teal spiral "Or just type it" on the search box](docs/images/control-panel.png)

<sub>A real Windows 11 desktop. Seven `bigarrow start` commands, and every tip on the signs is true.
<a href="#the-control-panel-picture">The commands</a>.</sub>

see-the-arrow draws a big arrow with a sign on top of every window, straight from a shell. An AI
agent or a script uses it to point at the one thing only a human should click: a permission, a
setting that needs an administrator, a payment, a code from a phone. Clicks go through the arrow,
the keyboard stays where it was, and every arrow ends by itself.

It never clicks, never types and never captures the screen. It only points.

## See it

![A fresh Chrome profile on chrome://extensions. A blue arrow draws itself from a sign reading "Turn on Developer mode" to the Developer mode switch, boxes the switch and pulses](docs/images/demo.gif)

```powershell
bigarrow point --app "Google Chrome" --element "Developer mode" --until-click `
  --color blue --from bottom-left --text "Turn on Developer mode"
```

An agent built a Chrome extension and needs the human to load it. `--element` finds the switch by
its label through UI Automation, so the agent never guesses coordinates. `--until-click` keeps the
arrow up until the human clicks the switch; then the agent points at the next step.

## Quick start

```powershell
cargo install --path .                      # from a clone of this repository
bigarrow point --at 960,540 --text "Click here"
bigarrow elements --app "Google Chrome" --match Developer
bigarrow install-skill                      # teaches Claude Code and Codex to use it
```

`bigarrow elements` lists what `--element` can point at, with the exact names and roles the system
reports. `bigarrow doctor` shows the displays and what the system allows.

## What agents use it for

| Situation | Why it needs a human | What the agent runs |
|---|---|---|
| A setting that needs an administrator | The agent runs without elevation, and should | `--app "Windows Explorer:Control Panel" --element "Clock and Region"` |
| Permission and consent prompts | Saying yes is the human's decision | `--element Allow --until-click` |
| Loading what the agent just built | Chrome keeps Developer mode a manual switch | `--element "Developer mode"`, then `--element "Load unpacked"` |
| Codes, PINs, tokens | The human has them, or must see them | `--text "Paste {{FKX7-29QD}} into the code field"`: `{{...}}` becomes a copy button |
| The human is not looking | Nobody reads a terminal from across the room | `--say` reads the sign aloud |
| Learning an app | One step at a time beats a wall of text | one `start` per step, numbered signs |

## Many arrows at once

`bigarrow start` returns at once and leaves the arrow up, so steps can be shown together. Signs keep
off the other arrows' signs and targets, and a sign that sits where a newer arrow points steps
aside by itself. Crossing shafts are avoided where there is room. A window with free desktop
around it gives the signs the most room; a maximized one leaves them nowhere to go but over it.

### The Control Panel picture

```powershell
$cp = "Windows Explorer:Control Panel"   # Control Panel runs inside explorer.exe
bigarrow start --window $cp --anchor top-left --size S --color black --border white-black --from top-left --text "Still here in 2026"
bigarrow start --app $cp --element "System and Security" --role link --size S --color red --close-button --from left --text "Firewall: still in here"
bigarrow start --app $cp --element "Hardware and Sound" --role link --size S --color purple --shape zigzag --from left --text "Device Manager is in here"
bigarrow start --app $cp --element "Uninstall a program" --role link --size S --style box --color green --from bottom-left --text "Same list as appwiz.cpl"
bigarrow start --app $cp --element "Clock and Region" --role link --size S --color orange --shape zigzag --from right --text "UTF-8 lives in Region"
bigarrow start --app $cp --element "Category" --style ring --size S --color pink --from top-right --text "Large icons: one page"
bigarrow start --app $cp --element "Search Box" --role textfield --size S --color teal --shape spiral --from right --text "Or just type it"
bigarrow stop --all                      # when done
```

## How it works

- **Click-through by shape.** Only the sign and the shaft take clicks. On Windows an invisible hit
  window shaped exactly like them sits above the click-through drawing; on X11 the window's input
  shape does the same. The target under the arrowhead stays clickable.
- **Never takes the focus.** The overlay windows cannot be activated. Bringing the target's window
  to the front is the only focus change (`--no-raise` skips it), and no key or click is ever
  synthesized.
- **Event-driven.** `--follow`, hiding while another app covers the target and making room for
  newer arrows all react to window events (WinEvents, X11 notifications), not to a polling timer.
- **Real pixels.** Coordinates in and out are the pixels that UI Automation, AT-SPI and
  full-resolution screenshots report, on every display and DPI.
- **One look everywhere.** Nunito Black and JetBrains Mono are built in, so a sign looks the same on
  every system and renders accents such as Vietnamese correctly.
- **Light.** One small process per arrow; the pulse replays prerendered frames.
- **Leaves nothing behind.** A live arrow holds a file lock; when its process dies, the lock and
  the arrow go with it.

## Install

```powershell
cargo install --path .        # Rust 1.89 or newer
bigarrow install-skill        # Claude Code (~/.claude/skills) and Codex (~/.agents/skills)
```

Windows 10 or 11 needs nothing else. Linux needs an X11 session; a compositing window manager gives
the drop shadow and smooth edges, and `--say` uses `spd-say` or `espeak-ng`.

## Commands

| Command | What it does |
|---|---|
| `point` | Draws an arrow and waits until it ends (8 s by default) |
| `start` | The same in the background, until `bigarrow stop` (300 s at most by default) |
| `clear`, alias `stop` | Removes the newest arrow; `--all`, `--pid`, `--session`, `--hook` |
| `front` | Brings an app, a window or a browser tab to the front |
| `elements` | Lists what `--element` can point at in an app |
| `doctor` | Shows displays, UI Automation or AT-SPI, and elevation |
| `demo` | An arrow, a ring and a box in turn |
| `install-skill` | Installs the agent skill |

- **Targets:** `--at X,Y`, `--rect X,Y,W,H`, `--mouse`, `--window App[:title]` with `--anchor`,
  `--element Label --app App[:title]` with `--role`. `--display N` makes `--at` and `--rect`
  relative to display N.
- **Looks:** `--color` (red, orange, yellow, green, teal, blue, purple, pink, black, white or
  `#RRGGBB`), `--size S|M|L`, `--style arrow|ring|box`, `--shape bend|straight|zigzag|spiral`,
  `--border shadow|white-black|black`, `--corners round|sharp`, `--from`, and colours for the
  border, text, edge and close button.
- **Behaviour:** `--duration`, `--close-button`, `--until-click`, `--until-any-click`, `--follow`,
  `--say`, `--voice`, `--json`, `--dry-run`, `--png`.

Every arrow ends by itself: after its duration, on `bigarrow stop`, when the agent that drew it exits
(`CLAUDE_PID` or `BIGARROW_OWNER_PID`), or when the human answers (`bigarrow stop --hook` as a Claude
Code `UserPromptSubmit` hook).

Exit codes: 0 ok, 2 bad input, 3 target not found, 4 permission missing.

## FAQ

**Does it click or type for me?** No. Pressing the button stays the human's job; that is the point.

**Will it steal my keyboard while I type?** No. The arrow's windows can never be activated. Only
bringing the target window forward changes the focus, and `--no-raise` turns that off.

**Can I click through it?** Everywhere except the sign and the shaft. The target under the
arrowhead takes clicks as usual. A click on the sign closes the arrow; a click on a copy button
copies its value.

**Why `--app "Windows Explorer:Control Panel"`?** On Windows some windows belong to another
program: Control Panel runs inside `explorer.exe`. The part after the colon picks the window by its
title. `bigarrow elements` shows the names to use. A placeholder is not a name: Control Panel's
search field is `Search Box`, not "Search Control Panel".

**Does `--element` see inside web pages?** In Chrome, yes: web content, `chrome://` pages and DevTools
all reach UI Automation without a flag.

**Can it draw over a UAC prompt or the lock screen?** No. Those run on a secure desktop that no
other program can draw on. See [Limits](#limits).

**Why is the command called `bigarrow`?** see-the-arrow keeps the command line of
[bigarrow for macOS](https://github.com/franzenzenhofer/big-arrow-on-the-screen), so agent skills
and scripts written for it run here too.

## Compatibility with bigarrow for macOS

Same subcommands, flags, defaults, messages, exit codes and JSON. Where the systems differ,
see-the-arrow follows Windows and Linux:

| | bigarrow for macOS | see-the-arrow |
|---|---|---|
| Coordinates | logical points | physical pixels (Windows), root window pixels (X11) |
| `--peekaboo`, `--snapshot` | Peekaboo snapshots | not available: Peekaboo is a macOS tool |
| The target's app does not come to the front | exit 3 | Windows refuses to hand over the focus at times: `point` and `start` warn, flash the taskbar button and show the arrow once the window is visible; `front` still exits 3 |
| `doctor --json` | TCC permissions, `macOS` | `os`, plus `elevated` and `uiAutomation` (Windows) or `session`, `compositor`, `atspi` (Linux) |
| `elements --json` | `frame: [[x,y],[w,h]]`, AX fields | `frame: [x,y,w,h]`, `role`, `name`, `value`, `id`, `enabled` |
| Exit 4 | missing Accessibility or Screen Recording | the target runs as administrator (Windows); no accessibility bus or a Wayland session (Linux) |
| `--say` | `say` | SAPI voices, OneCore ones included (Windows); `spd-say` or `espeak-ng` (Linux) |
| `--role` | AX roles | both naming schemes: `button`, `radiobutton`, `textfield`, ... and `Edit`, `TabItem`, `push button`, ... |

## Limits

- **Windows:** the Start menu, Alt-Tab, the lock screen, UAC prompts and exclusive full-screen games
  stay above every other window, the arrow included. An arrow lives on the virtual desktop it
  started on. A `clear` without administrator rights cannot end an arrow started as administrator.
- **Linux:** Wayland sessions draw through XWayland, but `--mouse`, `--window`, `--element`,
  `--until-click` and hiding while covered need X11 and exit 4 there. Without a compositor there is
  no shadow and edges are hard. On X11 a copied value lives as long as the arrow unless a clipboard
  manager takes it.
- Colour emoji are drawn in one colour or not at all.

## How we know it works

- **Measured on Windows 11 25H2 at 125 % scaling:** `start` returns in 113 to 239 ms, `stop` in
  about 240 ms including the 0.2 s fade. A pulsing arrow costs 0.5 to 0.8 % of one CPU core and
  about 20 to 24 MB of memory.
- **Tested:** unit tests for geometry, placement and rendering; command-line contract tests that
  run the built binary; and screen tests that draw real arrows and check focus, click-through and
  clean-up (`BIGARROW_SCREEN_TESTS=1`). The Linux build is checked under X11.
- **Real captures:** the picture and the animation above are screen captures of Windows 11 with
  the arrows on screen, not mock-ups.

## Development

```powershell
cargo clippy --all-targets -- -D warnings
cargo build --release
cargo test                                  # never draws on the screen
$env:BIGARROW_SCREEN_TESTS = 1; cargo test  # Windows: draws real arrows
bigarrow point --at 760,500 --text "Click here" --png arrow.png   # look without drawing
```

[AGENTS.md](AGENTS.md) holds the rules of the code base.

## Credits and license

MIT, see [LICENSE](LICENSE). The idea, the command line, the look and the agent skill come from
[bigarrow for macOS](https://github.com/franzenzenhofer/big-arrow-on-the-screen) by Franz
Enzenhofer (MIT). Icons from Feather, fonts Nunito and JetBrains Mono (OFL); see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
