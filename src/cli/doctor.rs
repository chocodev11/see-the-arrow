use serde_json::{Map, Value, json};

use super::options::DoctorArgs;
use super::output::{self, number};
use crate::model::error::{Error, Result};
use crate::platform;

/// `doctor`: what this session can do, and the displays in global coordinates.
pub fn run(args: &DoctorArgs) -> Result<()> {
    let required: Vec<String> = args
        .require
        .iter()
        .map(|raw| match raw.to_lowercase().as_str() {
            name @ ("accessibility" | "screen-recording") => Ok(name.to_owned()),
            _ => Err(Error::bad_input(format!(
                "--require '{raw}' is not one of accessibility, screen-recording"
            ))),
        })
        .collect::<Result<_>>()?;
    if args.request_accessibility {
        platform::request_accessibility();
    }
    if args.open_settings {
        platform::open_accessibility_settings();
    }
    let screens = platform::screens()?;
    let capabilities = platform::capabilities();
    let granted = |name: &str| {
        if name == "accessibility" {
            capabilities.accessibility
        } else {
            capabilities.screen_recording
        }
    };
    let missing: Vec<&String> = required.iter().filter(|name| !granted(name)).collect();
    let displays: Vec<Value> = screens
        .displays
        .iter()
        .map(|display| {
            let frame = display.frame;
            json!({
                "height": frame.height as i64, "id": display.id, "number": display.number(), "scale": display.scale,
                "width": frame.width as i64, "x": frame.x as i64, "y": frame.y as i64,
            })
        })
        .collect();
    let mut report = Map::new();
    report.insert("accessibility".into(), capabilities.accessibility.into());
    report.insert("displays".into(), displays.into());
    report.insert("drawingNeedsPermission".into(), false.into());
    report.insert(
        "missing".into(),
        missing
            .iter()
            .map(|name| Value::from(name.as_str()))
            .collect(),
    );
    report.insert("ok".into(), missing.is_empty().into());
    report.insert("os".into(), capabilities.os.clone().into());
    report.insert(
        "screenRecording".into(),
        capabilities.screen_recording.into(),
    );
    report.insert("screensHaveSeparateSpaces".into(), false.into());
    report.insert("version".into(), env!("CARGO_PKG_VERSION").into());
    report.extend(capabilities.extra.clone());
    if args.json {
        output::json(&report);
    } else {
        output::print(&format!(
            "bigarrow {} (see-the-arrow) on {}",
            env!("CARGO_PKG_VERSION"),
            capabilities.os
        ));
        output::print("drawing:          no permission needed");
        for line in &capabilities.lines {
            output::print(line);
        }
        output::print("displays (global top-left pixels):");
        for display in &screens.displays {
            let frame = display.frame;
            output::print(&format!(
                "  {}: origin {},{}  {}x{} @{}x  id {}",
                display.number(),
                number(frame.x),
                number(frame.y),
                number(frame.width),
                number(frame.height),
                number(display.scale),
                display.id
            ));
        }
    }
    match missing.first() {
        Some(name) => Err(Error::permission(platform::missing_capability(
            name,
            "This check",
        ))),
        None => Ok(()),
    }
}
