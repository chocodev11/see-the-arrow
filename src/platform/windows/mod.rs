//! Windows 10/11: Win32 layered windows, DWM, UI Automation, SAPI. The process is per-monitor-v2
//! DPI aware, so every coordinate is a physical pixel of the virtual screen.

mod apps;
mod capture;
mod overlay;
mod speech;
mod surface;
mod uia;

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde_json::{Map, json};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, HWND, LPARAM, POINT, RECT, SetHandleInformation,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, OpenEventW,
    SYNCHRONIZATION_ACCESS_RIGHTS, SetEvent,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::Win32::UI::WindowsAndMessaging::{
    FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FlashWindowEx, GetCursorPos, MONITORINFOF_PRIMARY,
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

pub use apps::{activate, title_bar_center, window_list};
pub use capture::capture;
pub use overlay::{Overlay, Waker};
pub use speech::Speech;
pub use uia::{elements, select_tab};

use super::Capabilities;
use crate::model::display::{Display, Screens};
use crate::model::error::{Error, Result};
use crate::model::geom::{Point, Rect};

const CF_UNICODETEXT: u32 = 13;

pub fn init() {
    // Fails only if the awareness is already set, e.g. by a manifest; either way it is per monitor.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

pub fn screens() -> Result<Screens> {
    let mut monitors: Vec<(bool, Display)> = Vec::new();
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = unsafe { &mut *(data.0 as *mut Vec<(bool, Display)>) };
        let mut info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: size_of::<MONITORINFOEXW>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) }.as_bool() {
            return true.into();
        }
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let device = String::from_utf16_lossy(&info.szDevice);
        let id = device
            .trim_end_matches('\0')
            .trim_start_matches(r"\\.\DISPLAY")
            .parse()
            .unwrap_or(monitors.len() as u32 + 1);
        let rect = |r: RECT| {
            Rect::new(
                r.left.into(),
                r.top.into(),
                (r.right - r.left).into(),
                (r.bottom - r.top).into(),
            )
        };
        let display = Display {
            index: 0,
            id,
            frame: rect(info.monitorInfo.rcMonitor),
            visible: rect(info.monitorInfo.rcWork),
            scale: f64::from(dpi_x) / 96.0,
        };
        monitors.push((
            info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            display,
        ));
        true.into()
    }
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut monitors as *mut _ as isize),
        )
    };
    // The primary display is number 1; the rest keep the system's order.
    monitors.sort_by_key(|(primary, _)| !primary);
    let displays: Vec<Display> = monitors
        .into_iter()
        .enumerate()
        .map(|(index, (_, display))| Display { index, ..display })
        .collect();
    if displays.is_empty() {
        return Err(Error::bad_input("no display found"));
    }
    Ok(Screens { displays })
}

pub fn mouse_location() -> Result<Point> {
    Ok(pointer())
}

fn pointer() -> Point {
    let mut point = POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    Point::new(point.x.into(), point.y.into())
}

/// `%LOCALAPPDATA%\bigarrow\run`, from the known folder, not the environment: agents and hooks may
/// run with different `TEMP` settings, but must see the same arrows.
pub fn registry_dir() -> PathBuf {
    let local = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }
        .ok()
        .and_then(|path| {
            let text = unsafe { path.to_string() }.ok();
            unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as _)) };
            text
        })
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    local.join("bigarrow").join("run")
}

/// "Show animations in Windows" off is the system's reduce-motion setting.
pub fn reduce_motion() -> bool {
    let mut enabled = BOOL(1);
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && !enabled.as_bool()
}

/// Signals the arrow's stop event; it fades out and exits.
pub fn stop_arrow(pid: u32) -> bool {
    const EVENT_MODIFY_STATE: SYNCHRONIZATION_ACCESS_RIGHTS = SYNCHRONIZATION_ACCESS_RIGHTS(0x0002);
    let name = HSTRING::from(overlay::stop_event_name(pid));
    let Ok(event) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name) }) else {
        return false;
    };
    let signalled = unsafe { SetEvent(event) }.is_ok();
    let _ = unsafe { CloseHandle(event) };
    signalled
}

/// Starts the detached arrow: no console, its own process group, out of the agent's job if the
/// job allows it. The agent's pipes are made non-inheritable first: std always passes inheritable
/// handles to children, and a 300 s child holding the agent's stdout would hang the agent.
pub fn spawn_overlay() -> std::io::Result<Child> {
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if let Ok(handle) = unsafe { GetStdHandle(which) } {
            let _ = unsafe {
                SetHandleInformation(
                    handle,
                    HANDLE_FLAG_INHERIT.0,
                    windows::Win32::Foundation::HANDLE_FLAGS(0),
                )
            };
        }
    }
    let executable = std::env::current_exe()?;
    let spawn = |flags: u32| {
        Command::new(&executable)
            .arg("__overlay")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    let flags = DETACHED_PROCESS.0 | CREATE_NEW_PROCESS_GROUP.0;
    spawn(flags | CREATE_BREAKAWAY_FROM_JOB.0).or_else(|_| spawn(flags))
}

pub fn set_clipboard(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // Another app may hold the clipboard for a moment.
    let opened = (0..10).any(|attempt| {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        unsafe { OpenClipboard(None) }.is_ok()
    });
    if !opened {
        return false;
    }
    let written = unsafe {
        EmptyClipboard().is_ok()
            && GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).is_ok_and(|memory| {
                let target = GlobalLock(memory) as *mut u16;
                if target.is_null() {
                    return false;
                }
                std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
                let _ = GlobalUnlock(memory);
                SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0))).is_ok()
            })
    };
    let _ = unsafe { CloseClipboard() };
    written
}

/// Flashes the window's taskbar button until it comes to the front.
pub fn flash(handle: u64) {
    if handle == 0 {
        return;
    }
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd: HWND(handle as _),
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    let _ = unsafe { FlashWindowEx(&info) };
}

pub fn capabilities() -> Capabilities {
    let elevated = apps::is_elevated(None);
    let mut extra = Map::new();
    extra.insert("elevated".into(), elevated.into());
    extra.insert("uiAutomation".into(), true.into());
    let mut lines = vec![
        "ui automation:    available (for --element, elements)".to_owned(),
        format!(
            "elevated:         {}",
            if elevated {
                "yes (apps running as administrator can be read)"
            } else {
                "no (apps running as administrator cannot be read)"
            }
        ),
    ];
    if let Some((name, path, pid)) = apps::responsible_app() {
        extra.insert(
            "responsibleApp".into(),
            json!({ "name": name, "path": path, "pid": pid }),
        );
        lines.push(format!("started from:     {name} ({path})"));
    }
    Capabilities {
        os: os_name(),
        accessibility: true,
        screen_recording: true,
        extra,
        lines,
    }
}

/// Windows needs no permission for either; kept for `--require` on the other platforms.
pub fn missing_capability(name: &str, feature: &str) -> String {
    format!(
        "{feature} needs {name}, which this session does not have. Run 'bigarrow doctor' to check."
    )
}

/// Windows has no accessibility switch to turn on.
pub fn request_accessibility() {}

/// Windows has no accessibility permission pane.
pub fn open_accessibility_settings() {}

/// "Windows 11 25H2 (build 26200)".
fn os_name() -> String {
    let read = |value: PCWSTR| {
        let mut buffer = [0u16; 128];
        let mut size = (buffer.len() * 2) as u32;
        let key = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                value,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr() as _),
                Some(&mut size),
            )
        };
        status
            .is_ok()
            .then(|| String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]))
    };
    let build: u32 = read(w!("CurrentBuildNumber"))
        .and_then(|build| build.parse().ok())
        .unwrap_or(0);
    let name = if build >= 22000 {
        "Windows 11"
    } else {
        "Windows 10"
    };
    match read(w!("DisplayVersion")) {
        Some(version) => format!("{name} {version} (build {build})"),
        None => format!("{name} (build {build})"),
    }
}
