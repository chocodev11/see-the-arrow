//! The arrow on screen: two layered windows. The visual one has `WS_EX_TRANSPARENT`, so every
//! click goes through it. The hit window above it is invisible (alpha 1) exactly where a click
//! should count and empty (alpha 0) elsewhere; Windows only hit-tests pixels with alpha > 0, so no
//! pointer polling is needed. Neither window can be activated, so the focus never moves.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use tiny_skia::Pixmap;
use windows::Win32::Foundation::{
    COLORREF, CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE,
    WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, ClientToScreen,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateEventW, CreateWaitableTimerExW, INFINITE,
    OpenProcess, PROCESS_SYNCHRONIZE, SetEvent, SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GA_ROOT, GetAncestor,
    HWND_TOPMOST, IDC_ARROW, LoadCursorW, MA_NOACTIVATE, MSG, MWMO_INPUTAVAILABLE,
    MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT, RegisterClassExW, SW_HIDE,
    SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow,
    TranslateMessage, ULW_ALPHA, UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_CLOSE, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_INPUT, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, HSTRING, w};

use super::pointer;
use crate::model::error::{Error, Result};
use crate::model::geom::Point;
use crate::platform::{Event, PixelRect};

const CLASS: windows::core::PCWSTR = w!("BigArrowOverlay");
const RI_MOUSE_LEFT_BUTTON_DOWN: u16 = 0x0001;
const WM_MOUSELEAVE: u32 = 0x02A3;

thread_local! {
    static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    /// Another window came to the front since the overlay last took the top back.
    static FOREGROUND_CHANGED: Cell<bool> = const { Cell::new(false) };
}

/// The stop event, for the console control handler.
static STOP_EVENT: AtomicIsize = AtomicIsize::new(0);

pub fn stop_event_name(pid: u32) -> String {
    format!(r"Local\bigarrow-{pid}")
}

fn push(event: Event) {
    EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        // Window changes and pointer moves come in bursts; the newest one is enough.
        if matches!(
            (events.back(), event),
            (Some(Event::WindowsChanged), Event::WindowsChanged)
                | (Some(Event::Pointer(_)), Event::Pointer(_))
        ) {
            events.pop_back();
        }
        events.push_back(event);
    });
}

/// Wakes the overlay's loop from another thread.
pub struct Waker(isize);

impl Waker {
    pub fn wake(&self) {
        let _ = unsafe { SetEvent(HANDLE(self.0 as _)) };
    }
}

/// A 32-bit top-down DIB section with a memory DC, premultiplied BGRA.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl Surface {
    fn new(width: u32, height: u32) -> Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let failed = || Error::bad_input("cannot create the overlay bitmap");
        let dc = unsafe { CreateCompatibleDC(None) };
        let mut bits = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
                .map_err(|_| failed())?;
        let previous = unsafe { SelectObject(dc, bitmap.into()) };
        Ok(Self {
            dc,
            bitmap,
            previous,
            bits: bits as *mut u8,
            width,
            height,
        })
    }

    fn pixels(&mut self) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(self.bits, (self.width * self.height * 4) as usize)
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct Overlay {
    visual: HWND,
    hit: HWND,
    rect: PixelRect,
    surface: Surface,
    hit_surface: Surface,
    alpha: u8,
    shown: bool,
    stop: HANDLE,
    wake: HANDLE,
    timer: HANDLE,
    owner: Option<HANDLE>,
    hooks: Vec<HWINEVENTHOOK>,
}

impl Overlay {
    pub fn new(rect: PixelRect, owner: Option<u32>) -> Result<Self> {
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .map_err(|_| Error::bad_input("no module handle"))?
            .into();
        register_class(instance);
        let create = |extra, owner: Option<HWND>| unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | extra,
                CLASS,
                w!("bigarrow"),
                WS_POPUP,
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                owner,
                None,
                Some(instance),
                None,
            )
        };
        let failed = |_| Error::bad_input("cannot create the overlay window");
        let visual = create(WS_EX_TRANSPARENT, None).map_err(failed)?;
        let hit = create(Default::default(), Some(visual)).map_err(failed)?;
        let name = HSTRING::from(stop_event_name(std::process::id()));
        let stop = unsafe { CreateEventW(None, false, false, &name) }
            .map_err(|_| Error::bad_input("cannot create the stop event"))?;
        STOP_EVENT.store(stop.0 as isize, Ordering::SeqCst);
        let _ = unsafe { SetConsoleCtrlHandler(Some(console_control), true) };
        let wake = unsafe { CreateEventW(None, false, false, None) }
            .map_err(|_| Error::bad_input("cannot create an event"))?;
        let timer = unsafe {
            CreateWaitableTimerExW(
                None,
                None,
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
        }
        .or_else(|_| unsafe { CreateWaitableTimerExW(None, None, 0, TIMER_ALL_ACCESS.0) })
        .map_err(|_| Error::bad_input("cannot create a timer"))?;
        let owner = owner.and_then(watch_owner);
        let hooks = [(0x0003, 0x0017), (0x8001, 0x8018)]
            .into_iter()
            .map(|(min, max)| unsafe {
                SetWinEventHook(
                    min,
                    max,
                    None,
                    Some(window_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            })
            .filter(|hook| !hook.is_invalid())
            .collect();
        Ok(Self {
            visual,
            hit,
            rect,
            surface: Surface::new(rect.width, rect.height)?,
            hit_surface: Surface::new(rect.width, rect.height)?,
            alpha: 255,
            shown: false,
            stop,
            wake,
            timer,
            owner,
            hooks,
        })
    }

    /// Layered windows always blend.
    pub fn translucent(&self) -> bool {
        true
    }

    pub fn waker(&self) -> Waker {
        Waker(self.wake.0 as isize)
    }

    /// Raw input reaches this process wherever the click lands, with no permission.
    pub fn watch_clicks(&mut self) -> bool {
        let device = RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: self.visual,
        };
        unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }.is_ok()
    }

    /// Moves and resizes both windows; draw and set the hit mask again afterwards.
    pub fn set_rect(&mut self, rect: PixelRect) {
        if let (Ok(surface), Ok(hit)) = (
            Surface::new(rect.width, rect.height),
            Surface::new(rect.width, rect.height),
        ) {
            (self.rect, self.surface, self.hit_surface) = (rect, surface, hit);
        }
    }

    /// Shows `pixmap` (premultiplied RGBA, the window's size); `dirty` limits the copy.
    pub fn present(&mut self, pixmap: &Pixmap, dirty: Option<PixelRect>) {
        let width = self.rect.width as usize;
        let area = dirty.unwrap_or(self.rect);
        let (left, top) = (
            (area.x - self.rect.x) as usize,
            (area.y - self.rect.y) as usize,
        );
        let source = pixmap.data();
        let target = self.surface.pixels();
        for y in top..top + area.height as usize {
            for x in left..left + area.width as usize {
                let at = (y * width + x) * 4;
                target[at] = source[at + 2];
                target[at + 1] = source[at + 1];
                target[at + 2] = source[at];
                target[at + 3] = source[at + 3];
            }
        }
        let dirty = RECT {
            left: left as i32,
            top: top as i32,
            right: (left + area.width as usize) as i32,
            bottom: (top + area.height as usize) as i32,
        };
        update(
            self.visual,
            self.rect,
            Some(self.surface.dc),
            self.alpha,
            Some(&dirty),
        );
    }

    /// Alpha 255 bytes in `mask` become clickable, everything else clicks through.
    pub fn set_hit_mask(&mut self, mask: &[u8]) {
        let target = self.hit_surface.pixels();
        for (pixel, &alpha) in target.chunks_exact_mut(4).zip(mask) {
            pixel.copy_from_slice(&[0, 0, 0, u8::from(alpha > 0)]);
        }
        update(self.hit, self.rect, Some(self.hit_surface.dc), 255, None);
    }

    /// The whole arrow's opacity, without redrawing.
    pub fn set_alpha(&mut self, alpha: f64) {
        let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        if alpha != self.alpha {
            self.alpha = alpha;
            update(self.visual, self.rect, None, alpha, None);
        }
    }

    pub fn set_visible(&mut self, visible: bool) {
        for window in [self.visual, self.hit] {
            if !visible {
                let _ = unsafe { ShowWindow(window, SW_HIDE) };
            } else if self.shown {
                let _ = unsafe { ShowWindow(window, SW_SHOWNA) };
            } else {
                // The first ShowWindow of a process may follow the launcher's startup flags instead.
                let _ = unsafe {
                    SetWindowPos(
                        window,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                    )
                };
            }
        }
        self.shown |= visible;
    }

    /// Pumps window messages until an event arrives or `deadline` passes. Signalled handles and
    /// queued messages are always looked at first, so a busy session never starves them.
    pub fn wait(&mut self, deadline: Option<Instant>) -> Event {
        const WAIT_TIMEOUT: u32 = 0x102;
        loop {
            if FOREGROUND_CHANGED.replace(false) {
                self.stay_on_top();
            }
            if let Some(event) = EVENTS.with(|events| events.borrow_mut().pop_front()) {
                return event;
            }
            let timeout = match deadline {
                None => INFINITE,
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        0
                    } else {
                        // Relative due time in 100 ns units; the high-resolution timer beats the
                        // wait timeout's 15.6 ms granularity.
                        let due = -((left.as_nanos() / 100).max(1) as i64);
                        let _ = unsafe { SetWaitableTimer(self.timer, &due, 0, None, None, false) };
                        INFINITE
                    }
                }
            };
            let mut handles = vec![self.stop, self.wake, self.timer];
            handles.extend(self.owner);
            let woke = unsafe {
                MsgWaitForMultipleObjectsEx(
                    Some(&handles),
                    timeout,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                )
            };
            if woke.0 == WAIT_TIMEOUT {
                return Event::Timeout;
            }
            match woke.0.wrapping_sub(WAIT_OBJECT_0.0) {
                0 => return Event::Stop,
                1 => return Event::Wake,
                2 => return Event::Timeout,
                3 if self.owner.is_some() => {
                    if let Some(owner) = self.owner.take() {
                        let _ = unsafe { CloseHandle(owner) };
                    }
                    return Event::OwnerGone;
                }
                _ => pump(),
            }
        }
    }

    /// Other always-on-top windows compete for the top; take it back when the foreground changes.
    fn stay_on_top(&self) {
        if self.shown {
            let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
            let _ = unsafe { SetWindowPos(self.visual, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
            let _ = unsafe { SetWindowPos(self.hit, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
        }
    }

    pub fn close(&mut self) {
        for hook in self.hooks.drain(..) {
            let _ = unsafe { UnhookWinEvent(hook) };
        }
        let _ = unsafe { DestroyWindow(self.hit) };
        let _ = unsafe { DestroyWindow(self.visual) };
        for handle in [self.wake, self.timer].into_iter().chain(self.owner.take()) {
            let _ = unsafe { CloseHandle(handle) };
        }
    }
}

/// `UpdateLayeredWindowIndirect`: position and size, optionally new pixels and a dirty rect, and the opacity.
fn update(window: HWND, rect: PixelRect, source: Option<HDC>, alpha: u8, dirty: Option<&RECT>) {
    let position = POINT {
        x: rect.x,
        y: rect.y,
    };
    let size = SIZE {
        cx: rect.width as i32,
        cy: rect.height as i32,
    };
    let origin = POINT::default();
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let info = UPDATELAYEREDWINDOWINFO {
        cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
        hdcDst: HDC::default(),
        pptDst: &position,
        psize: &size,
        hdcSrc: source.unwrap_or_default(),
        pptSrc: if source.is_some() {
            &origin
        } else {
            std::ptr::null()
        },
        crKey: COLORREF(0),
        pblend: &blend,
        dwFlags: ULW_ALPHA,
        prcDirty: dirty.map_or(std::ptr::null(), |dirty| dirty as *const RECT),
    };
    let _ = unsafe { UpdateLayeredWindowIndirect(window, &info) };
}

fn pump() {
    let mut message = MSG::default();
    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
}

fn register_class(instance: HINSTANCE) {
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_procedure),
            hInstance: instance,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
    });
}

/// An owner that already exited ends the arrow at once; one we may not watch (it runs elevated)
/// counts as alive.
fn watch_owner(pid: u32) -> Option<HANDLE> {
    const ERROR_INVALID_PARAMETER: u32 = 87;
    match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
        Ok(handle) => Some(handle),
        Err(error) => {
            if error.code() == windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER) {
                push(Event::OwnerGone);
            }
            None
        }
    }
}

unsafe extern "system" fn console_control(_: u32) -> BOOL {
    let _ = unsafe { SetEvent(HANDLE(STOP_EVENT.load(Ordering::SeqCst) as _)) };
    true.into()
}

unsafe extern "system" fn window_event(
    _: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
    // Only whole top-level windows: not carets, not controls inside them.
    if object == 0
        && child == 0
        && !window.is_invalid()
        && unsafe { GetAncestor(window, GA_ROOT) } == window
    {
        if event == EVENT_SYSTEM_FOREGROUND {
            FOREGROUND_CHANGED.set(true);
        }
        push(Event::WindowsChanged);
    }
}

unsafe extern "system" fn window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            if !TRACKING.get() {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: window,
                    dwHoverTime: 0,
                };
                TRACKING.set(unsafe { TrackMouseEvent(&mut track) }.is_ok());
            }
            push(Event::Pointer(Some(client_point(window, lparam))));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            TRACKING.set(false);
            push(Event::Pointer(None));
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            push(Event::Click(client_point(window, lparam)));
            return LRESULT(0);
        }
        WM_INPUT if left_button_down(HRAWINPUT(lparam.0 as _)) => {
            push(Event::GlobalClick(pointer()))
        }
        WM_DISPLAYCHANGE => push(Event::DisplaysChanged),
        WM_DPICHANGED => {
            push(Event::DisplaysChanged);
            return LRESULT(0);
        }
        WM_CLOSE => {
            push(Event::Stop);
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// A mouse message's client coordinates in global pixels.
fn client_point(window: HWND, lparam: LPARAM) -> Point {
    let mut point = POINT {
        x: i32::from(lparam.0 as i16),
        y: i32::from((lparam.0 >> 16) as i16),
    };
    let _ = unsafe { ClientToScreen(window, &mut point) };
    Point::new(point.x.into(), point.y.into())
}

fn left_button_down(input: HRAWINPUT) -> bool {
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    let read = unsafe {
        GetRawInputData(
            input,
            RID_INPUT,
            Some(&mut raw as *mut RAWINPUT as _),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    read != u32::MAX
        && raw.header.dwType == 0
        && unsafe { raw.data.mouse.Anonymous.Anonymous.usButtonFlags } & RI_MOUSE_LEFT_BUTTON_DOWN
            != 0
}
