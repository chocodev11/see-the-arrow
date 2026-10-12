//! Linux: X11 for drawing and windows (EWMH, RandR, XShape, XInput2), AT-SPI for UI elements.
//! A Wayland session draws through XWayland; what needs global window positions is refused there,
//! because Wayland does not tell apps where windows are.

mod atspi;
mod capture;
mod overlay;
mod x11;

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde_json::Map;

pub use self::atspi::elements;
pub use capture::capture;
pub use overlay::{Overlay, Waker};
pub use x11::{activate, screens, window_list};

use super::Capabilities;
use crate::model::error::{Error, Result};
use crate::model::geom::Point;

pub fn init() {}

/// A Wayland session: windows, pointer and clicks of other apps are out of reach.
pub fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland")
}

/// The exit-4 message for what a Wayland session cannot do.
pub fn needs_x11(feature: &str) -> Error {
    Error::permission(format!(
        "{feature} needs an X11 session, because Wayland hides window positions from apps: log in to an X11 session \
         (e.g. 'GNOME on Xorg', 'Plasma (X11)'), or point with --at or --rect. Run 'bigarrow doctor' to check."
    ))
}

pub fn mouse_location() -> Result<Point> {
    if wayland() {
        return Err(needs_x11("Pointing at the mouse (--mouse)"));
    }
    x11::pointer()
}

/// Half the height of a typical title bar or header bar.
pub fn title_bar_center(scale: f64) -> f64 {
    16.0 * scale
}

/// Asks the window manager to mark the window as wanting attention.
pub fn flash(handle: u64) {
    x11::demand_attention(handle);
}

pub fn select_tab(handle: u64, title: &str) -> bool {
    x11::window_pid(handle).is_some_and(|pid| atspi::select_tab(pid, title))
}

/// `$XDG_RUNTIME_DIR/bigarrow`, private to the user; `/tmp/bigarrow-<uid>` without it.
pub fn registry_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("bigarrow"),
        None => {
            std::env::temp_dir().join(format!("bigarrow-{}", rustix::process::getuid().as_raw()))
        }
    }
}

/// No common desktop setting to read; `--no-animation` turns motion off.
pub fn reduce_motion() -> bool {
    false
}

pub fn stop_arrow(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return false;
    };
    rustix::process::kill_process(pid, rustix::process::Signal::TERM).is_ok()
}

/// Starts the detached arrow in a session of its own, so the agent's terminal never takes it down.
pub fn spawn_overlay() -> std::io::Result<Child> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("__overlay")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| rustix::process::setsid().map(drop).map_err(Into::into));
    }
    command.spawn()
}

thread_local! {
    /// On X11 the clipboard's owner serves its content, so it lives as long as the arrow.
    static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
}

pub fn set_clipboard(text: &str) -> bool {
    CLIPBOARD.with(|clipboard| {
        let mut clipboard = clipboard.borrow_mut();
        if clipboard.is_none() {
            *clipboard = arboard::Clipboard::new().ok();
        }
        clipboard
            .as_mut()
            .is_some_and(|clipboard| clipboard.set_text(text).is_ok())
    })
}

pub fn capabilities() -> Capabilities {
    let session = if wayland() { "wayland" } else { "x11" };
    let accessibility = atspi::available();
    let composited = x11::composited();
    let mut extra = Map::new();
    extra.insert("session".into(), session.into());
    extra.insert("compositor".into(), composited.into());
    extra.insert("atspi".into(), accessibility.into());
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP") {
        extra.insert("desktop".into(), desktop.into());
    }
    let lines = vec![
        format!(
            "session:          {session}{}",
            if wayland() {
                " (drawing through XWayland; --window, --element, --mouse and click watching need X11)"
            } else {
                ""
            }
        ),
        format!(
            "compositor:       {}",
            if composited {
                "yes"
            } else {
                "no (no shadow, hard edges)"
            }
        ),
        format!(
            "accessibility:    {} (AT-SPI, for --element, elements)",
            if accessibility {
                "available"
            } else {
                "missing"
            }
        ),
    ];
    Capabilities {
        os: os_name(),
        accessibility: accessibility && !wayland(),
        screen_recording: !wayland(),
        extra,
        lines,
    }
}

pub fn missing_capability(name: &str, feature: &str) -> String {
    if wayland() {
        return needs_x11(feature).message;
    }
    format!(
        "{feature} needs {name}: the AT-SPI accessibility bus is not running in this session. Install at-spi2-core, \
         run 'bigarrow doctor --request-accessibility', then restart the app. Run 'bigarrow doctor' to check."
    )
}

/// Turns the accessibility bus on for apps started from now on.
pub fn request_accessibility() {
    atspi::enable();
}

pub fn open_accessibility_settings() {
    for (program, argument) in [
        ("gnome-control-center", "universal-access"),
        ("systemsettings", "kcm_access"),
    ] {
        if Command::new(program)
            .arg(argument)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}

/// "Ubuntu 24.04.1 LTS (x11)".
fn os_name() -> String {
    let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let name = release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|name| name.trim_matches('"').to_owned())
        .unwrap_or_else(|| "Linux".to_owned());
    format!("{name} ({})", if wayland() { "wayland" } else { "x11" })
}

/// `--say`: speech-dispatcher, else eSpeak NG, else eSpeak. Dropping it stops the voice.
pub struct Speech {
    child: Child,
}

impl Speech {
    pub fn start(text: &str, voice: Option<&str>) -> Option<Self> {
        let attempts: [(&str, Vec<&str>); 3] = [
            (
                "spd-say",
                voice.map_or(vec!["-w"], |voice| vec!["-w", "-y", voice]),
            ),
            ("espeak-ng", voice.map_or(vec![], |voice| vec!["-v", voice])),
            ("espeak", voice.map_or(vec![], |voice| vec!["-v", voice])),
        ];
        attempts.into_iter().find_map(|(program, arguments)| {
            // A leading space keeps a text starting with '-' from reading as an option.
            Command::new(program)
                .args(arguments)
                .arg(if text.starts_with('-') {
                    format!(" {text}")
                } else {
                    text.to_owned()
                })
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok()
                .map(|child| Self { child })
        })
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = Command::new("spd-say")
            .arg("-C")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
