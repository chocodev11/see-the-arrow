# Install see-the-arrow

This page is for an AI agent (Claude Code, Codex, ...) that a human asked to install see-the-arrow.
Run the steps in order yourself; ask the human only where a step says so.

see-the-arrow draws a big click-through arrow with a sign to show the human where to click. Its
command is `bigarrow`. It never clicks, never types and never captures the screen.

## 1. Check the machine

| The machine | Go to |
|---|---|
| Windows 10 or 11 on x64 (`$env:PROCESSOR_ARCHITECTURE` is `AMD64`) | [Windows](#windows) |
| Linux on x86-64 (`uname -m` is `x86_64`) with glibc 2.35 or newer (`ldd --version`) | [Linux](#linux) |
| Any other Windows or Linux | [From source](#from-source) |
| macOS | Not this project: install [bigarrow for macOS](https://github.com/franzenzenhofer/big-arrow-on-the-screen), same commands |

On Linux, `echo $XDG_SESSION_TYPE` should print `x11`. On `wayland` arrows still draw, but
`--mouse`, `--window`, `--element` and `--until-click` need an X11 session: tell the human.

If `bigarrow --version` already answers, this is an update: run `bigarrow stop --all` first, since a
running arrow keeps the old file in use, then install over it.

## 2. Install the binary

### Windows

In PowerShell:

```powershell
$ProgressPreference = "SilentlyContinue"
$dir = Join-Path $env:LOCALAPPDATA "Programs\see-the-arrow"
$zip = Join-Path $env:TEMP "bigarrow.zip"
Invoke-WebRequest "https://github.com/chocodev11/see-the-arrow/releases/latest/download/bigarrow-x86_64-pc-windows-msvc.zip" -OutFile $zip
Expand-Archive $zip $dir -Force
Remove-Item $zip
$path = [Environment]::GetEnvironmentVariable("Path", "User")
if (($path -split ";") -notcontains $dir) {
    [Environment]::SetEnvironmentVariable("Path", "$path;$dir", "User")
}
& "$dir\bigarrow.exe" --version
```

The new `Path` reaches programs started from now on, not your own shell. Until the human restarts
you, call `bigarrow` by its full path, `%LOCALAPPDATA%\Programs\see-the-arrow\bigarrow.exe`.

### Linux

```sh
mkdir -p ~/.local/bin
curl -fsSL https://github.com/chocodev11/see-the-arrow/releases/latest/download/bigarrow-x86_64-unknown-linux-gnu.tar.gz \
  | tar -xz -C ~/.local/bin bigarrow
~/.local/bin/bigarrow --version
```

If `~/.local/bin` is not on `PATH`, add `export PATH="$HOME/.local/bin:$PATH"` to `~/.profile` and
call `~/.local/bin/bigarrow` until the next login. `--say` reads signs aloud with `spd-say` or
`espeak-ng`; if neither is installed, tell the human rather than installing one with sudo.

### From source

Needs Rust 1.89 or newer (https://rustup.rs) and, on Windows, the Visual Studio C++ Build Tools:

```sh
cargo install see-the-arrow --locked
```

Every release also lists the SHA-256 of its archives in `SHA256SUMS`.

## 3. Install the skill

```sh
bigarrow install-skill
```

This teaches Claude Code (`~/.claude/skills/big-arrow`) and Codex (`~/.agents/skills/big-arrow`)
when and how to point; `--claude` or `--codex` installs for one of them. If it reports changed
files, the human edited the skill: ask before rerunning with `--force`.

For Claude Code, also add this hook to `~/.claude/settings.json`, so the arrows of a session go away
when the human answers. Merge it into the file, keep every setting and hook already there, and skip
it if `bigarrow stop --hook` is already present:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "bigarrow stop --hook" }] }
    ]
  }
}
```

## 4. Check it

```sh
bigarrow doctor
bigarrow point --mouse --text "see-the-arrow works" --duration 4
```

On Wayland, point with `--at 600,400` instead of `--mouse`. Exit code 4 means a permission is
missing; `bigarrow doctor` says which.

## 5. Tell the human

Report the version, where `bigarrow` is and that the skill is installed. Ask the human to restart
you (and on Windows any open terminal), so `bigarrow` is on the `PATH` and the skill is loaded.
From then on they can ask "show me where to click" and you point.

## Uninstall

Run `bigarrow stop --all`, then delete the binary (Windows: the folder
`%LOCALAPPDATA%\Programs\see-the-arrow` and its entry in the user `Path`; Linux:
`~/.local/bin/bigarrow`; from source: `cargo uninstall see-the-arrow`), the skill folders
`~/.claude/skills/big-arrow` and `~/.agents/skills/big-arrow`, and the `bigarrow stop --hook` hook
in `~/.claude/settings.json`.
