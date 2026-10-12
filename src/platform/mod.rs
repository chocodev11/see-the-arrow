//! Everything that talks to the operating system. Each platform module exports the same names.

use serde_json::{Map, Value};

use crate::model::geom::{Point, Rect};
use crate::model::matcher::{AppInfo, WindowInfo};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::*;

/// Normal top-level windows front to back, the apps that own them, and the app in front.
#[derive(Debug, Clone, Default)]
pub struct WindowList {
    pub windows: Vec<WindowInfo>,
    pub apps: Vec<AppInfo>,
    pub active: Option<u32>,
}

/// What the overlay's event loop reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The deadline passed.
    Timeout,
    /// The pointer over the click region, global pixels; `None` once it left.
    Pointer(Option<Point>),
    /// A click in the click region, global pixels.
    Click(Point),
    /// A click anywhere, for `--until-click` and `--until-any-click`.
    GlobalClick(Point),
    /// `clear`, Ctrl+C or a signal.
    Stop,
    OwnerGone,
    DisplaysChanged,
    /// Some window moved, appeared, disappeared or came to the front.
    WindowsChanged,
    /// Another thread has news for the session.
    Wake,
}

/// A rect in whole global pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    /// The smallest pixel rect covering `rect`, at least one pixel wide and high.
    pub fn enclosing(rect: Rect) -> PixelRect {
        let (left, top) = (rect.x.floor(), rect.y.floor());
        let (right, bottom) = (rect.max_x().ceil(), rect.max_y().ceil());
        PixelRect {
            x: left as i32,
            y: top as i32,
            width: (right - left).max(1.0) as u32,
            height: (bottom - top).max(1.0) as u32,
        }
    }

    pub fn intersect(&self, other: &PixelRect) -> Option<PixelRect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width as i32).min(other.x + other.width as i32);
        let bottom = (self.y + self.height as i32).min(other.y + other.height as i32);
        (right > left && bottom > top).then(|| PixelRect {
            x: left,
            y: top,
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        })
    }

    pub fn grown(&self, by: i32) -> PixelRect {
        PixelRect {
            x: self.x - by,
            y: self.y - by,
            width: (self.width as i32 + by * 2).max(1) as u32,
            height: (self.height as i32 + by * 2).max(1) as u32,
        }
    }
}

/// What `doctor` reports beyond the displays.
pub struct Capabilities {
    pub os: String,
    /// `--element` and `elements` can read UI elements.
    pub accessibility: bool,
    /// `screenshot` can read the screen; no permission gates it outside macOS.
    pub screen_recording: bool,
    /// Platform-specific `doctor --json` fields.
    pub extra: Map<String, Value>,
    /// Platform-specific `doctor` text lines.
    pub lines: Vec<String>,
}
