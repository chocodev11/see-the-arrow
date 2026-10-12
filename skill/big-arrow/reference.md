# big-arrow reference

Detail that the common path in SKILL.md does not need. `bigarrow <subcommand> --help` lists
every flag.

## Settings pages

Windows: `start ms-settings:<page>` (PowerShell `Start-Process ms-settings:<page>`). A wrong page
opens the Settings home, so check the window title.

- Privacy: `privacy-webcam`, `privacy-microphone`, `privacy-location`, `privacy-broadfilesystemaccess`
- `defaultapps`, `startupapps`, `notifications`, `display`, `sound`, `network-status`,
  `bluetooth`, `windowsupdate`, `appsfeatures`, `windowsdefender`

Linux: `gnome-control-center <panel>` (GNOME, e.g. `privacy`, `display`, `sound`),
`systemsettings kcm_<module>` (KDE Plasma), `xfce4-settings-manager` (Xfce).

Web pages: open the URL itself, not the site's start page.

## Browser page content

`--element` finds page content in Chrome, Edge and Firefox on Windows. On Linux, Chrome needs
`--force-renderer-accessibility`. Otherwise take the element's `getBoundingClientRect()` and pass
physical pixels, `d = devicePixelRatio`:
`--rect (screenX + r.x) * d,(screenY + outerHeight - innerHeight + r.y) * d,r.width * d,r.height * d`
with `--app "Google Chrome:<tab title>"`. Check with `--dry-run --json` or `--png`.

## Coordinates from a screenshot

`bigarrow screenshot --png shot.png --json` captures the primary display; `--display N`, `--window
"App[:title]"` (raised first unless `--no-raise`) or `--rect x,y,w,h` pick another area. The JSON
gives `rect` (the area, global pixels) and `size` (the PNG): image pixel (x, y) is global
(rect x + x * rect w / size w, rect y + y * rect h / size h). Use `--max-size 1280`: vision models
shrink larger images on their own and every coordinate read from them shifts. The capture shows
live arrows too; `bigarrow stop` first for a clean one, or keep them to check where they point.
Exit 4: a locked screen (Windows), Wayland or XWayland (Linux).
`--display N` makes `--at` and `--rect` relative to display N (1-based).

## Look

- `--color` red (default), orange, yellow, green, teal, blue, purple, pink, black, white or #hex
- `--style box|ring` marks without covering; `--size S|M|L`
- `--shape bend|straight|zigzag|spiral` (spiral loops once around the sign, then points)
- `--border shadow|white-black|black` (default: white border with drop shadow);
  `--border-color`, `--text-color`, `--edge-color`, `--close-color`, `--close-x-color`
- `--from <side>` preferred side of the sign; `--anchor center|title|top-left|...` with `--window`
- `--no-animation`, `--corners round|sharp`, `--voice` for `--say` (part of a voice name, e.g. `An` for Vietnamese on Windows)
- `{{value}}` in `--text`: a copy button, several per sign; a click copies and keeps the arrow.
  A long value is shortened on screen and copied whole; `--say` speaks it; unbalanced braces
  exit 2. `--json` lists the frames as `copyButtons` and reports `copied` when the arrow ends

## Timing and ending

- `point` shows for 8 s (`--duration N`, 0 = until stopped); `start` = `point --detach --duration 300`
- `--until-click` ends when the human clicks the target, `--until-any-click` on any click;
  `--follow` moves with a `--window` or `--element` target
- `--no-raise` leaves windows as they are
- `stop --all` removes every arrow, `--pid` one, `--session <id>` one session's
- `--json` prints a machine-readable result, `--png <file>` renders offscreen instead of showing
