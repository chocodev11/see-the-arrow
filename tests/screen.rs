//! The real overlay on the real desktop. Opt-in with `BIGARROW_SCREEN_TESTS=1`, because these
//! draw on the screen of whoever runs them. Windows only.
#![cfg(windows)]

use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, IsWindowVisible, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WindowFromPoint,
};
use windows::core::BOOL;

/// Arrows share the screen: one test at a time.
static SCREEN: Mutex<()> = Mutex::new(());

fn enabled() -> bool {
    let on = std::env::var("BIGARROW_SCREEN_TESTS").is_ok_and(|value| value == "1");
    if !on {
        eprintln!("skipped: set BIGARROW_SCREEN_TESTS=1 to draw on the screen");
    }
    on
}

fn fast_limit() -> Duration {
    if std::env::var("CI").is_ok() {
        Duration::from_millis(1500)
    } else {
        Duration::from_millis(600)
    }
}

fn bigarrow(args: &[&str]) -> (i32, serde_json::Value, Duration) {
    let start = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_bigarrow"))
        .args(args)
        .output()
        .expect("bigarrow runs");
    let text = String::from_utf8_lossy(if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    })
    .into_owned();
    let json = serde_json::from_str(text.trim()).unwrap_or(serde_json::Value::Null);
    (output.status.code().unwrap_or(-1), json, start.elapsed())
}

fn target() -> String {
    let (_, doctor, _) = bigarrow(&["doctor", "--json"]);
    let display = &doctor["displays"][0];
    let x = display["x"].as_i64().unwrap() + display["width"].as_i64().unwrap() * 45 / 100;
    let y = display["y"].as_i64().unwrap() + display["height"].as_i64().unwrap() * 45 / 100;
    format!("{x},{y}")
}

/// The overlay windows of a process, with their extended styles.
fn windows_of(pid: u32) -> Vec<(HWND, u32)> {
    let mut found: Vec<(HWND, u32, u32)> = Vec::new();
    unsafe extern "system" fn collect(window: HWND, data: LPARAM) -> BOOL {
        let found = unsafe { &mut *(data.0 as *mut Vec<(HWND, u32, u32)>) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        let mut class = [0u16; 64];
        let length = unsafe { GetClassNameW(window, &mut class) };
        if String::from_utf16_lossy(&class[..length as usize]) == "BigArrowOverlay"
            && unsafe { IsWindowVisible(window) }.as_bool()
        {
            found.push((
                window,
                pid,
                unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32,
            ));
        }
        true.into()
    }
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut found as *mut _ as isize)) };
    found
        .into_iter()
        .filter(|(_, owner, _)| *owner == pid)
        .map(|(window, _, style)| (window, style))
        .collect()
}

fn window_at(x: i32, y: i32) -> HWND {
    unsafe { WindowFromPoint(POINT { x, y }) }
}

#[test]
fn a_timed_arrow_ends_by_itself() {
    if !enabled() {
        return;
    }
    let _screen = SCREEN.lock().unwrap();
    let (code, json, took) = bigarrow(&[
        "point",
        "--at",
        &target(),
        "--text",
        "Integration test",
        "--duration",
        "1",
        "--json",
    ]);
    assert_eq!(code, 0);
    assert_eq!(json["dismissedReason"], "timeout");
    assert!(took < Duration::from_secs(10));
}

#[test]
fn a_detached_arrow_is_click_through_on_top_and_never_takes_the_focus() {
    if !enabled() {
        return;
    }
    let _screen = SCREEN.lock().unwrap();
    let foreground = unsafe { GetForegroundWindow() };
    let (code, json, took) = bigarrow(&[
        "start",
        "--at",
        &target(),
        "--text",
        "Integration test",
        "--no-animation",
        "--duration",
        "0",
        "--json",
    ]);
    assert_eq!(code, 0, "{json}");
    assert!(took < fast_limit(), "start took {took:?}");
    let pid = json["pid"].as_u64().unwrap() as u32;
    let windows = windows_of(pid);
    assert_eq!(windows.len(), 2, "a visual and a hit window");
    for (_, style) in &windows {
        assert_eq!(
            style & (WS_EX_TOPMOST.0 | WS_EX_NOACTIVATE.0 | WS_EX_LAYERED.0),
            WS_EX_TOPMOST.0 | WS_EX_NOACTIVATE.0 | WS_EX_LAYERED.0
        );
    }
    assert_eq!(
        windows
            .iter()
            .filter(|(_, style)| style & WS_EX_TRANSPARENT.0 != 0)
            .count(),
        1
    );
    assert_eq!(
        unsafe { GetForegroundWindow() },
        foreground,
        "the focus moved"
    );
    let ours: Vec<HWND> = windows.iter().map(|(window, _)| *window).collect();
    let tip: Vec<i32> = target()
        .split(',')
        .map(|value| value.parse().unwrap())
        .collect();
    assert!(
        !ours.contains(&window_at(tip[0], tip[1])),
        "a click at the tip must reach the window below"
    );
    let sign: Vec<i64> = json["sign"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_i64().unwrap())
        .collect();
    let center = window_at(
        (sign[0] + sign[2] / 2) as i32,
        (sign[1] + sign[3] / 2) as i32,
    );
    let covered = !ours.contains(&center);
    if covered {
        eprintln!(
            "skipped the sign hit test: something above every window (a locked screen?) covers the sign"
        );
    }
    let (code, cleared, took) = bigarrow(&["clear", "--pid", &pid.to_string(), "--json"]);
    assert_eq!(code, 0);
    assert_eq!(cleared["cleared"], serde_json::json!([pid]));
    assert!(took < fast_limit(), "clear took {took:?}");
    assert!(windows_of(pid).is_empty());
}

#[test]
fn two_arrows_and_clear_all() {
    if !enabled() {
        return;
    }
    let _screen = SCREEN.lock().unwrap();
    let at = target();
    let (_, first, _) = bigarrow(&[
        "start",
        "--at",
        &at,
        "--text",
        "First",
        "--no-animation",
        "--duration",
        "0",
        "--json",
    ]);
    let (_, second, _) = bigarrow(&[
        "start",
        "--at",
        &at,
        "--text",
        "Second",
        "--color",
        "blue",
        "--no-animation",
        "--duration",
        "0",
        "--json",
    ]);
    let pids = [
        first["pid"].as_u64().unwrap(),
        second["pid"].as_u64().unwrap(),
    ];
    assert_ne!(
        first["sign"], second["sign"],
        "the second sign keeps clear of the first"
    );
    let (code, cleared, _) = bigarrow(&["clear", "--all", "--json"]);
    assert_eq!(code, 0);
    let cleared: Vec<u64> = cleared["cleared"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pid| pid.as_u64().unwrap())
        .collect();
    assert!(pids.iter().all(|pid| cleared.contains(pid)));
    assert!(pids.iter().all(|pid| windows_of(*pid as u32).is_empty()));
}
