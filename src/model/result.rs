use std::collections::BTreeMap;

use serde::Serialize;

/// Why an arrow went away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DismissReason {
    Timeout,
    Signal,
    Clicked,
    Closed,
    TargetGone,
    DisplayGone,
    OwnerGone,
}

impl DismissReason {
    pub fn name(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Signal => "signal",
            Self::Clicked => "clicked",
            Self::Closed => "closed",
            Self::TargetGone => "targetGone",
            Self::DisplayGone => "displayGone",
            Self::OwnerGone => "ownerGone",
        }
    }
}

/// The target of a `point` result. `x`/`y` echo the input; `rect` is in global input units.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResultTarget {
    pub detail: BTreeMap<String, String>,
    pub display: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rect: Option<[f64; 4]>,
    pub source: String,
    pub x: f64,
    pub y: f64,
}

/// The `--json` result of `point` and `start`. Fields in alphabetical order, as bigarrow for macOS prints them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copied: Option<u32>,
    /// Every `{{value}}` copy chip in the sign: x, y, width, height in global input units.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_buttons: Option<Vec<[i64; 4]>>,
    pub detached: bool,
    pub direction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_after: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_reason: Option<DismissReason>,
    pub dry_run: bool,
    /// With `--png`: the image's top-left corner and pixels per coordinate unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<[i64; 3]>,
    pub ok: bool,
    pub pid: u32,
    /// The sign: x, y, width, height in global input units.
    pub sign: [i64; 4],
    pub target: ResultTarget,
}

/// The `--json` error object, also used for every non-zero exit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorResult {
    pub code: i32,
    pub error: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClearResult {
    pub cleared: Vec<u32>,
    pub ok: bool,
}

/// The `--json` result of `screenshot`. Image pixel (x, y) shows the global pixel
/// (rect x + x * rect width / size width, rect y + y * rect height / size height).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScreenshotResult {
    pub ok: bool,
    /// The PNG, as an absolute path.
    pub path: String,
    /// The captured area: x, y, width, height in global input units.
    pub rect: [i64; 4],
    /// The PNG's width and height; smaller than the rect after `--max-size`.
    pub size: [i64; 2],
}
