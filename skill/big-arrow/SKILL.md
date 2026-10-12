---
name: big-arrow
description: "Point at something on the human's screen (Windows, or Linux on X11) with a big arrow and a sign (\"Click Save: keeps your changes\") using the bigarrow CLI. Use whenever the human has to look at, click, type into or choose a specific spot, button, field, window, tab or menu: show me where, point to it, which button, where do I click, chỉ cho tôi chỗ nào, guide me through this UI, the human must approve or sign something, or you need the human's hand and they may not be looking at the terminal. Mandatory when it is time-critical: a code that expires, a login or payment page that times out, a build waiting on the human."
license: MIT
allowed-tools: Bash(bigarrow:*)
metadata:
  short-description: "Point at the screen with a big arrow and a sign"
---

# big-arrow

`bigarrow` (see-the-arrow) draws a click-through arrow with a sign, needs no permission and never
takes the focus. Point at the thing itself, a line in the terminal is easy to miss.

```bash
start ms-settings:privacy-webcam        # PowerShell: Start-Process ms-settings:privacy-webcam
bigarrow start --element "Camera access" --app "Settings" --text "Switch this on: apps may use the camera" --say
# watch for the result (page changed, job continued, file appeared), then:
bigarrow stop
```

## Rules

- Open the exact page or URL yourself, then point at the one control left for the human. Never
  walk them through menus. The human clicks; you point.
- The sign is one full sentence naming the action ("Click Save", never "here"); start with the
  human's name only when you know it. When the click approves, grants, pays, signs, sends or
  deletes, add what happens: "Click Pay: 12 USD for the domain renewal". The human decides; the
  facts go on the sign, not only in the terminal.
- `--say` when the human may not be looking. Time-critical (code expires, page times out, job
  waits): `--color red --say` and say by when: "Enter the 2FA code now, it expires in 60 s".
- Something to paste (a command you may not run, a URL, a code): put it on the sign as
  `{{value}}`. The human sees the value on a copy button (no braces); one click copies all of it
  and the arrow stays. Point at where it goes: "Copy {{winget upgrade --all}} and paste it
  here", then check that it was pasted.
- One arrow per step; `bigarrow stop` the moment the step is done. Arrows also end alone: after
  300 s, when your session exits, when the human answers (hook below).
- A click on the sign or shaft, except a copy button, removes the arrow (target clicks pass
  through). A removed arrow was seen: check the result, do not redraw. Skip `--close-button`
  unless asked.

## Targets

| You know | Use |
|---|---|
| A label in a native, Electron or browser app | `--element "Allow" --app "Settings"` |
| An app or one of its windows | `--window "Notepad"` / `--window "Google Chrome:Inbox"` |
| A spot inside a browser tab | `--rect x,y,w,h --app "Google Chrome:<tab title>"` |
| A spot in a screenshot | `--at X,Y --app "<App>"` |

Always pass `--app` (or `--window`) for a target inside an app: it is raised first and the arrow
hides while another app covers it. `App:title` matches a window title, else a tab. Apps match by
product name ("Google Chrome", "Settings") or executable ("chrome", "msedge"). Coordinates are
global top-left physical pixels, as UI Automation and full-resolution screenshots report them.
Roles: `button`, `textfield`, `checkbox`, `link`, `tab`, `menuitem`, `combobox`, ... Unsure?
`bigarrow elements --app X --match "allow"` or `--dry-run`.

## Errors

Exit 2 bad input, 3 target not found (the message lists what exists), 4 permission missing: run
`bigarrow doctor`. On Windows, 4 means the target app runs as administrator and bigarrow does
not: ask the human to restart that app normally, or to run your terminal as administrator. On
Linux it means a Wayland session or no accessibility bus; the message names the fix. If the app
cannot come to the front (Windows refuses to steal the focus), bigarrow warns, flashes its
taskbar button and shows the arrow once the window is visible.

More (settings pages, browser coordinates, look and timing flags): [reference.md](reference.md).

## Setup once

`~/.claude/settings.json`: `"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": "bigarrow stop --hook"}]}]}`
