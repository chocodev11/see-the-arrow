//! X11 queries: displays (RandR), the pointer, client windows (EWMH) and raising them.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, MapState, Window,
};
use x11rb::rust_connection::RustConnection;

use crate::model::display::{Display, Screens};
use crate::model::error::{Error, Result};
use crate::model::geom::{Point, Rect};
use crate::model::matcher::{AppInfo, WindowInfo};
use crate::platform::WindowList;

const RAISE_LIMIT: Duration = Duration::from_millis(1500);

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        _NET_ACTIVE_WINDOW,
        _NET_CLIENT_LIST_STACKING,
        _NET_CURRENT_DESKTOP,
        _NET_FRAME_EXTENTS,
        _NET_WM_DESKTOP,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_DEMANDS_ATTENTION,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_WINDOW_OPACITY,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,
        _NET_WORKAREA,
        _GTK_FRAME_EXTENTS,
        UTF8_STRING,
    }
}

/// A connection to the X server, its root window and the atoms everything here needs.
pub struct X {
    pub conn: RustConnection,
    pub screen: usize,
    pub root: Window,
    pub atoms: Atoms,
}

pub fn connect() -> Result<X> {
    let (conn, screen) = x11rb::connect(None).map_err(|_| {
        Error::permission("Drawing needs an X11 display (or XWayland), and DISPLAY reaches none. Run 'bigarrow doctor' to check.")
    })?;
    let failed = |_| Error::bad_input("the X server stopped answering");
    let atoms = Atoms::new(&conn)
        .map_err(failed)?
        .reply()
        .map_err(|_| Error::bad_input("the X server stopped answering"))?;
    let root = conn.setup().roots[screen].root;
    Ok(X {
        conn,
        screen,
        root,
        atoms,
    })
}

impl X {
    /// 32-bit values of a property, empty when it is missing.
    pub fn cardinals(&self, window: Window, property: u32, kind: impl Into<u32>) -> Vec<u32> {
        self.conn
            .get_property(false, window, property, kind, 0, 4096)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(Iterator::collect))
            .unwrap_or_default()
    }

    fn text(&self, window: Window, property: u32, kind: impl Into<u32>) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, property, kind, 0, 4096)
            .ok()?
            .reply()
            .ok()?;
        (!reply.value.is_empty()).then(|| String::from_utf8_lossy(&reply.value).into_owned())
    }

    fn title(&self, window: Window) -> String {
        self.text(window, self.atoms._NET_WM_NAME, self.atoms.UTF8_STRING)
            .or_else(|| self.text(window, AtomEnum::WM_NAME.into(), AtomEnum::STRING))
            .unwrap_or_default()
    }

    /// The frame on screen: the client area plus the window manager's decorations, minus the
    /// shadows client-side decorations draw.
    fn bounds(&self, window: Window) -> Option<Rect> {
        let geometry = self.conn.get_geometry(window).ok()?.reply().ok()?;
        let origin = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        let side =
            |values: &[u32], index: usize| f64::from(values.get(index).copied().unwrap_or(0));
        let frame = self.cardinals(window, self.atoms._NET_FRAME_EXTENTS, AtomEnum::CARDINAL);
        let shadow = self.cardinals(window, self.atoms._GTK_FRAME_EXTENTS, AtomEnum::CARDINAL);
        let (left, right, top, bottom) = (
            side(&frame, 0) - side(&shadow, 0),
            side(&frame, 1) - side(&shadow, 1),
            side(&frame, 2) - side(&shadow, 2),
            side(&frame, 3) - side(&shadow, 3),
        );
        Some(Rect::new(
            f64::from(origin.dst_x) - left,
            f64::from(origin.dst_y) - top,
            f64::from(geometry.width) + left + right,
            f64::from(geometry.height) + top + bottom,
        ))
    }

    pub fn pid(&self, window: Window) -> Option<u32> {
        self.cardinals(window, self.atoms._NET_WM_PID, AtomEnum::CARDINAL)
            .first()
            .copied()
    }

    pub fn active_window(&self) -> Option<Window> {
        self.cardinals(self.root, self.atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW)
            .first()
            .copied()
            .filter(|window| *window != 0)
    }
}

/// Every RandR monitor, the primary first, with the work area and the Xft scale.
pub fn screens() -> Result<Screens> {
    let x = connect()?;
    let scale = x11rb::resource_manager::new_from_default(&x.conn)
        .ok()
        .and_then(|database| database.get_value::<f64>("Xft.dpi", "").ok().flatten())
        .map_or(1.0, |dpi| dpi / 96.0);
    let desktop = x
        .cardinals(x.root, x.atoms._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL)
        .first()
        .copied()
        .unwrap_or(0) as usize;
    let areas = x.cardinals(x.root, x.atoms._NET_WORKAREA, AtomEnum::CARDINAL);
    let work = areas.get(desktop * 4..desktop * 4 + 4).map(|area| {
        Rect::new(
            f64::from(area[0]),
            f64::from(area[1]),
            f64::from(area[2]),
            f64::from(area[3]),
        )
    });
    let monitors = x
        .conn
        .randr_get_monitors(x.root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.monitors)
        .unwrap_or_default();
    let mut found: Vec<(bool, Display)> = monitors
        .iter()
        .map(|monitor| {
            let frame = Rect::new(
                monitor.x.into(),
                monitor.y.into(),
                monitor.width.into(),
                monitor.height.into(),
            );
            let visible = work
                .and_then(|work| work.intersection(&frame))
                .filter(|part| part.area() > 0.0)
                .unwrap_or(frame);
            (
                monitor.primary,
                Display {
                    index: 0,
                    id: monitor.name,
                    frame,
                    visible,
                    scale,
                },
            )
        })
        .collect();
    if found.is_empty() {
        let screen = &x.conn.setup().roots[x.screen];
        let frame = Rect::new(
            0.0,
            0.0,
            screen.width_in_pixels.into(),
            screen.height_in_pixels.into(),
        );
        found.push((
            true,
            Display {
                index: 0,
                id: 0,
                frame,
                visible: work.unwrap_or(frame),
                scale,
            },
        ));
    }
    found.sort_by_key(|(primary, _)| !primary);
    Ok(Screens {
        displays: found
            .into_iter()
            .enumerate()
            .map(|(index, (_, display))| Display { index, ..display })
            .collect(),
    })
}

pub fn pointer() -> Result<Point> {
    let x = connect()?;
    let reply = x
        .conn
        .query_pointer(x.root)
        .map_err(|_| Error::bad_input("the X server stopped answering"))?
        .reply();
    let reply = reply.map_err(|_| Error::bad_input("the X server stopped answering"))?;
    Ok(Point::new(reply.root_x.into(), reply.root_y.into()))
}

/// Client windows front to back. Minimized windows are left out, but their apps still count.
pub fn window_list() -> Result<WindowList> {
    if super::wayland() {
        return Err(super::needs_x11(
            "Finding windows (--window, --app, --element)",
        ));
    }
    let x = connect()?;
    let own = std::process::id();
    let desktop = x
        .cardinals(x.root, x.atoms._NET_CURRENT_DESKTOP, AtomEnum::CARDINAL)
        .first()
        .copied();
    let mut list = WindowList {
        active: x.active_window().and_then(|window| x.pid(window)),
        ..WindowList::default()
    };
    let mut clients = x.cardinals(x.root, x.atoms._NET_CLIENT_LIST_STACKING, AtomEnum::WINDOW);
    clients.reverse();
    for window in clients {
        let types = x.cardinals(window, x.atoms._NET_WM_WINDOW_TYPE, AtomEnum::ATOM);
        if !types.is_empty()
            && !types.iter().any(|kind| {
                *kind == x.atoms._NET_WM_WINDOW_TYPE_NORMAL
                    || *kind == x.atoms._NET_WM_WINDOW_TYPE_DIALOG
            })
        {
            continue;
        }
        let states = x.cardinals(window, x.atoms._NET_WM_STATE, AtomEnum::ATOM);
        let on_desktop = x
            .cardinals(window, x.atoms._NET_WM_DESKTOP, AtomEnum::CARDINAL)
            .first()
            .copied();
        if states.contains(&x.atoms._NET_WM_STATE_ABOVE)
            || on_desktop
                .zip(desktop)
                .is_some_and(|(on, current)| on != current && on != u32::MAX)
        {
            continue;
        }
        let Some(pid) = x.pid(window).filter(|pid| *pid != own) else {
            continue;
        };
        if !list.apps.iter().any(|app| app.pid == pid) {
            list.apps.push(AppInfo {
                pid,
                names: app_names(&x, window, pid),
            });
        }
        let viewable = x
            .conn
            .get_window_attributes(window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|attributes| attributes.map_state == MapState::VIEWABLE);
        if states.contains(&x.atoms._NET_WM_STATE_HIDDEN) || !viewable {
            continue;
        }
        if let Some(bounds) = x.bounds(window) {
            list.windows.push(WindowInfo {
                pid,
                bounds,
                title: x.title(window),
                handle: u64::from(window),
            });
        }
    }
    Ok(list)
}

/// The app's name from its `.desktop` file ("Google Chrome"), then its window class and
/// executable ("google-chrome", "chrome").
fn app_names(x: &X, window: Window, pid: u32) -> Vec<String> {
    let class = x
        .text(window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING)
        .unwrap_or_default();
    let parts: Vec<&str> = class.split('\0').filter(|part| !part.is_empty()).collect();
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|comm| comm.trim().to_owned())
        .unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    let desktop = parts
        .iter()
        .rev()
        .find_map(|part| desktop_names().get(&part.to_lowercase()).cloned());
    for name in desktop
        .into_iter()
        .chain(parts.iter().rev().map(|part| (*part).to_owned()))
        .chain(std::iter::once(comm))
    {
        if !name.is_empty() && !names.iter().any(|known| known.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    names
}

/// `Name=` of every installed `.desktop` file, by lowercase `StartupWMClass` and file stem.
fn desktop_names() -> &'static HashMap<String, String> {
    static NAMES: OnceLock<HashMap<String, String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let home = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| format!("{home}/.local/share"))
        });
        let system = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
        let mut names = HashMap::new();
        for dir in home.into_iter().chain(system.split(':').map(str::to_owned)) {
            let Ok(entries) = std::fs::read_dir(format!("{dir}/applications")) else {
                continue;
            };
            for path in entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "desktop"))
            {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let field = |key: &str| {
                    text.lines()
                        .find_map(|line| line.strip_prefix(key))
                        .map(|value| value.trim().to_owned())
                };
                let Some(name) = field("Name=") else { continue };
                if let Some(class) = field("StartupWMClass=") {
                    names
                        .entry(class.to_lowercase())
                        .or_insert_with(|| name.clone());
                }
                if let Some(stem) = path.file_stem() {
                    names
                        .entry(stem.to_string_lossy().to_lowercase())
                        .or_insert(name);
                }
            }
        }
        names
    })
}

/// Asks the window manager to activate the window (or the app's first window), restoring it
/// if it is minimized, then waits until it is the active window.
pub fn activate(pid: u32, window: Option<u64>) -> (bool, u64) {
    let Ok(x) = connect() else { return (false, 0) };
    let target = window.map(|handle| handle as Window).or_else(|| {
        x.cardinals(x.root, x.atoms._NET_CLIENT_LIST_STACKING, AtomEnum::WINDOW)
            .into_iter()
            .rev()
            .find(|window| x.pid(*window) == Some(pid))
    });
    let Some(target) = target else {
        return (false, 0);
    };
    // Source 2: a pager acting for the user, which window managers let through.
    let message = ClientMessageEvent::new(32, target, x.atoms._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0]);
    let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
    let _ = x.conn.send_event(false, x.root, mask, message);
    let _ = x.conn.flush();
    let start = Instant::now();
    while start.elapsed() < RAISE_LIMIT {
        if x.active_window() == Some(target) {
            std::thread::sleep(Duration::from_millis(100));
            return (true, u64::from(target));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    (false, u64::from(target))
}

pub fn demand_attention(handle: u64) {
    let Ok(x) = connect() else { return };
    let message = ClientMessageEvent::new(
        32,
        handle as Window,
        x.atoms._NET_WM_STATE,
        [1, x.atoms._NET_WM_STATE_DEMANDS_ATTENTION, 0, 2, 0],
    );
    let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
    let _ = x.conn.send_event(false, x.root, mask, message);
    let _ = x.conn.flush();
}

pub fn window_pid(handle: u64) -> Option<u32> {
    connect().ok()?.pid(handle as Window)
}

/// A compositing manager owns `_NET_WM_CM_S<screen>`; without one there is no translucency.
pub fn composited() -> bool {
    let Ok(x) = connect() else { return false };
    let name = format!("_NET_WM_CM_S{}", x.screen);
    x.conn
        .intern_atom(true, name.as_bytes())
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .filter(|reply| reply.atom != 0)
        .and_then(|reply| x.conn.get_selection_owner(reply.atom).ok()?.reply().ok())
        .is_some_and(|reply| reply.owner != 0)
}
