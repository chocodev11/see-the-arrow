//! Windows, the apps that own them, raising, elevation and the app a shell runs in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, FindWindowExW, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, InternalGetWindowText,
    IsHungAppWindow, IsIconic, IsWindowVisible, SM_CXPADDEDBORDER, SM_CYCAPTION, SM_CYSIZEFRAME,
    SW_RESTORE, SetForegroundWindow, ShowWindow, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::model::error::Result;
use crate::model::geom::Rect;
use crate::model::matcher::{AppInfo, WindowInfo};
use crate::platform::WindowList;

const RAISE_LIMIT: Duration = Duration::from_millis(1500);
/// Shell windows that are not app windows: the desktop and the taskbars.
const SHELL_CLASSES: [&str; 4] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

thread_local! {
    /// App names by pid; reading version resources is too slow to repeat for every window check.
    static NAMES: RefCell<HashMap<u32, Vec<String>>> = RefCell::new(HashMap::new());
}

fn top_level_windows() -> Vec<HWND> {
    let mut windows: Vec<HWND> = Vec::new();
    unsafe extern "system" fn collect(window: HWND, data: LPARAM) -> BOOL {
        unsafe { &mut *(data.0 as *mut Vec<HWND>) }.push(window);
        true.into()
    }
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize)) };
    windows
}

fn class_name(window: HWND) -> String {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

fn title(window: HWND) -> String {
    let mut buffer = [0u16; 512];
    let length = unsafe { InternalGetWindowText(window, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

/// The process a window belongs to; a UWP app's frame belongs to ApplicationFrameHost, so its
/// content window's process counts.
fn owner(window: HWND) -> u32 {
    let pid = |window: HWND| {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        pid
    };
    if class_name(window) == "ApplicationFrameWindow"
        && let Ok(core) = unsafe {
            FindWindowExW(
                Some(window),
                None,
                w!("Windows.UI.Core.CoreWindow"),
                PCWSTR::null(),
            )
        }
    {
        return pid(core);
    }
    pid(window)
}

fn is_cloaked(window: HWND) -> bool {
    let mut cloaked = 0u32;
    let read = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as _,
            size_of::<u32>() as u32,
        )
    };
    read.is_ok() && cloaked != 0
}

/// The visible frame, without the invisible resize borders.
fn bounds(window: HWND) -> Rect {
    let mut rect = RECT::default();
    let framed = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as _,
            size_of::<RECT>() as u32,
        )
    };
    if framed.is_err() {
        let _ = unsafe { GetWindowRect(window, &mut rect) };
    }
    Rect::new(
        rect.left.into(),
        rect.top.into(),
        (rect.right - rect.left).into(),
        (rect.bottom - rect.top).into(),
    )
}

/// A window an app shows to a human: no tool windows, no always-on-top panels, not the shell.
fn is_app_window(window: HWND, minimized: bool) -> bool {
    if !unsafe { IsWindowVisible(window) }.as_bool()
        || (!minimized && unsafe { IsIconic(window) }.as_bool())
        || is_cloaked(window)
    {
        return false;
    }
    let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;
    if style & WS_EX_TOOLWINDOW.0 != 0 && style & WS_EX_APPWINDOW.0 == 0
        || style & WS_EX_TOPMOST.0 != 0
    {
        return false;
    }
    !SHELL_CLASSES.contains(&class_name(window).as_str())
}

pub fn window_list() -> Result<WindowList> {
    let own = unsafe { GetCurrentProcessId() };
    let mut list = WindowList::default();
    let foreground = unsafe { GetForegroundWindow() };
    if !foreground.is_invalid() {
        list.active = Some(owner(foreground));
    }
    // Minimized windows are not on screen, but their apps can still be named and raised.
    for window in top_level_windows() {
        if !is_app_window(window, true) {
            continue;
        }
        let pid = owner(window);
        if pid == own {
            continue;
        }
        if !unsafe { IsIconic(window) }.as_bool() {
            list.windows.push(WindowInfo {
                pid,
                bounds: bounds(window),
                title: title(window),
                handle: window.0 as u64,
            });
        }
        if !list.apps.iter().any(|app| app.pid == pid) {
            list.apps.push(AppInfo {
                pid,
                names: app_names(pid),
            });
        }
    }
    Ok(list)
}

/// Product name ("Google Chrome") first, then the executable's name ("chrome").
fn app_names(pid: u32) -> Vec<String> {
    NAMES.with(|names| {
        names
            .borrow_mut()
            .entry(pid)
            .or_insert_with(|| {
                let Some(path) = image_path(pid) else {
                    return vec![format!("pid {pid}")];
                };
                let stem = std::path::Path::new(&path)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut found: Vec<String> = file_description(&path)
                    .into_iter()
                    .filter(|name| !name.is_empty())
                    .collect();
                if !found.iter().any(|name| name.eq_ignore_ascii_case(&stem)) {
                    found.push(stem);
                }
                found
            })
            .clone()
    })
}

pub fn image_path(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 1024];
    let mut length = buffer.len() as u32;
    let read = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    read.is_ok()
        .then(|| String::from_utf16_lossy(&buffer[..length as usize]))
}

/// The `FileDescription` from the executable's version resource.
pub fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let path = PCWSTR(wide.as_ptr());
    let size = unsafe { GetFileVersionInfoSizeW(path, None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(path, None, size, data.as_mut_ptr() as _) }.ok()?;
    let query = |key: &str| -> Option<(*const u8, u32)> {
        let key: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
        let (mut value, mut length) = (std::ptr::null_mut(), 0u32);
        unsafe {
            VerQueryValueW(
                data.as_ptr() as _,
                PCWSTR(key.as_ptr()),
                &mut value,
                &mut length,
            )
        }
        .as_bool()
        .then_some((value as *const u8, length))
    };
    let (translation, length) = query(r"\VarFileInfo\Translation")?;
    if length < 4 {
        return None;
    }
    let (language, codepage) = unsafe {
        (
            *(translation as *const u16),
            *(translation as *const u16).add(1),
        )
    };
    let (text, length) = query(&format!(
        r"\StringFileInfo\{language:04x}{codepage:04x}\FileDescription"
    ))?;
    let text = unsafe { std::slice::from_raw_parts(text as *const u16, length as usize) };
    Some(
        String::from_utf16_lossy(text)
            .trim_end_matches('\0')
            .trim()
            .to_owned(),
    )
}

/// Half the height of a standard title bar at `scale`.
pub fn title_bar_center(scale: f64) -> f64 {
    let dpi = (96.0 * scale).round() as u32;
    let metric = |index| f64::from(unsafe { GetSystemMetricsForDpi(index, dpi) });
    (metric(SM_CYCAPTION) + metric(SM_CYSIZEFRAME) + metric(SM_CXPADDEDBORDER)) / 2.0
}

/// Brings `window` (or the app's front window, minimized ones included) to the front. Windows
/// lets a background process do that only in some cases; attaching to the foreground thread's
/// input usually gets through. Returns whether it worked, and the window it tried.
pub fn activate(pid: u32, window: Option<u64>) -> (bool, u64) {
    let target = window.map(|handle| HWND(handle as _)).or_else(|| {
        top_level_windows()
            .into_iter()
            .find(|window| is_app_window(*window, true) && owner(*window) == pid)
    });
    let Some(target) = target else {
        return (false, 0);
    };
    if unsafe { IsIconic(target) }.as_bool() {
        let _ = unsafe { ShowWindow(target, SW_RESTORE) };
    }
    if !unsafe { SetForegroundWindow(target) }.as_bool() {
        let foreground = unsafe { GetForegroundWindow() };
        if !foreground.is_invalid() && !unsafe { IsHungAppWindow(foreground) }.as_bool() {
            let theirs = unsafe { GetWindowThreadProcessId(foreground, None) };
            let ours = unsafe { GetCurrentThreadId() };
            if unsafe { AttachThreadInput(ours, theirs, true) }.as_bool() {
                let _ = unsafe { BringWindowToTop(target) };
                let _ = unsafe { SetForegroundWindow(target) };
                let _ = unsafe { AttachThreadInput(ours, theirs, false) };
            }
        }
    }
    let start = Instant::now();
    while start.elapsed() < RAISE_LIMIT {
        let foreground = unsafe { GetForegroundWindow() };
        if foreground == target || (!foreground.is_invalid() && owner(foreground) == pid) {
            std::thread::sleep(Duration::from_millis(100));
            return (true, target.0 as u64);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    (false, target.0 as u64)
}

/// True when the process (`None`: this one) runs elevated. A process we may not even ask about
/// runs at a higher integrity level, which counts as elevated too.
pub fn is_elevated(pid: Option<u32>) -> bool {
    let process = match pid {
        None => unsafe { GetCurrentProcess() },
        Some(pid) => match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(process) => process,
            Err(_) => return true,
        },
    };
    let mut token = HANDLE::default();
    let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) };
    let elevated = opened.is_ok() && {
        let mut elevation = TOKEN_ELEVATION::default();
        let mut length = 0;
        let read = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as _),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        };
        read.is_ok() && elevation.TokenIsElevated != 0
    };
    if opened.is_ok() {
        let _ = unsafe { CloseHandle(token) };
    }
    if pid.is_some() {
        let _ = unsafe { CloseHandle(process) };
    }
    elevated || (pid.is_some() && opened.is_err())
}

/// The first ancestor of this process that shows a window: the terminal or editor the agent's
/// shell runs in. Name, path, pid.
pub fn responsible_app() -> Option<(String, String, u32)> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let mut parents = HashMap::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snapshot) };
    let with_windows: Vec<u32> = top_level_windows()
        .into_iter()
        .filter(|window| is_app_window(*window, true))
        .map(owner)
        .collect();
    let mut pid = unsafe { GetCurrentProcessId() };
    for _ in 0..32 {
        pid = *parents.get(&pid)?;
        if with_windows.contains(&pid) {
            let path = image_path(pid)?;
            let name = file_description(&path)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| path.clone());
            return Some((name, path, pid));
        }
    }
    None
}
