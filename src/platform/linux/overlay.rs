//! The arrow on screen: one override-redirect ARGB window. Its XShape input region is exactly
//! where a click counts, so every other click goes through. Override-redirect windows are never
//! managed, so the window manager never gives them the focus.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::os::fd::{AsFd, OwnedFd};
use std::time::Instant;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::pipe::{PipeFlags, pipe_with};
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::image::{BitsPerPixel, Image, ImageOrder, ScanlinePad};
use x11rb::protocol::Event as XEvent;
use x11rb::protocol::randr::{ConnectionExt as _, NotifyMask};
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xinput::{self, ConnectionExt as _, XIEventMask};
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClipOrdering, ColormapAlloc, ConfigureWindowAux,
    ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, PropMode, Rectangle, StackMode,
    VisualClass, Window, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use super::x11::{X, connect};
use crate::model::error::{Error, Result};
use crate::model::geom::Point;
use crate::platform::{Event, PixelRect};

/// Wakes the overlay's loop from another thread.
pub struct Waker(OwnedFd);

impl Waker {
    pub fn wake(&self) {
        let _ = rustix::io::write(&self.0, &[1]);
    }
}

pub struct Overlay {
    x: X,
    window: Window,
    gc: u32,
    rect: PixelRect,
    alpha: f64,
    composited: bool,
    events: VecDeque<Event>,
    signals: OwnedFd,
    wake: (OwnedFd, OwnedFd),
    owner: Option<OwnedFd>,
    active: Option<Window>,
}

impl Overlay {
    pub fn new(rect: PixelRect, owner: Option<u32>) -> Result<Self> {
        let x = connect()?;
        fn failed<E>(_: E) -> Error {
            Error::bad_input("cannot create the overlay window")
        }
        let screen = &x.conn.setup().roots[x.screen];
        let visual = screen
            .allowed_depths
            .iter()
            .filter(|depth| depth.depth == 32)
            .flat_map(|depth| &depth.visuals)
            .find(|visual| visual.class == VisualClass::TRUE_COLOR)
            .map(|visual| visual.visual_id)
            .ok_or_else(|| {
                Error::bad_input("the X server has no 32-bit visual for a translucent window")
            })?;
        let colormap = x.conn.generate_id().map_err(failed)?;
        x.conn
            .create_colormap(ColormapAlloc::NONE, colormap, x.root, visual)
            .map_err(failed)?;
        let window = x.conn.generate_id().map_err(failed)?;
        let events = EventMask::ENTER_WINDOW
            | EventMask::LEAVE_WINDOW
            | EventMask::POINTER_MOTION
            | EventMask::BUTTON_PRESS;
        let aux = CreateWindowAux::new()
            .background_pixel(0)
            .border_pixel(0)
            .colormap(colormap)
            .override_redirect(1u32)
            .event_mask(events);
        let (width, height) = (rect.width as u16, rect.height as u16);
        x.conn
            .create_window(
                32,
                window,
                x.root,
                rect.x as i16,
                rect.y as i16,
                width,
                height,
                0,
                WindowClass::INPUT_OUTPUT,
                visual,
                &aux,
            )
            .map_err(failed)?;
        let _ = x.conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            b"bigarrow\0bigarrow\0",
        );
        let _ = x.conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"bigarrow",
        );
        let _ = x.conn.change_property32(
            PropMode::REPLACE,
            window,
            x.atoms._NET_WM_PID,
            AtomEnum::CARDINAL,
            &[std::process::id()],
        );
        let notification = x.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION;
        let _ = x.conn.change_property32(
            PropMode::REPLACE,
            window,
            x.atoms._NET_WM_WINDOW_TYPE,
            AtomEnum::ATOM,
            &[notification],
        );
        // Click-through everywhere until the hit mask arrives.
        let _ = x.conn.shape_rectangles(
            SO::SET,
            SK::INPUT,
            ClipOrdering::UNSORTED,
            window,
            0,
            0,
            &[],
        );
        let gc = x.conn.generate_id().map_err(failed)?;
        x.conn
            .create_gc(gc, window, &CreateGCAux::new())
            .map_err(failed)?;
        let watch = ChangeWindowAttributesAux::new()
            .event_mask(EventMask::SUBSTRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE);
        let _ = x.conn.change_window_attributes(x.root, &watch);
        let _ = x.conn.randr_select_input(
            x.root,
            NotifyMask::SCREEN_CHANGE | NotifyMask::OUTPUT_CHANGE,
        );
        let _ = x.conn.flush();
        let (signals, alarm) = pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)
            .map_err(|_| Error::bad_input("cannot create a pipe"))?;
        for signal in [
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
        ] {
            if let Ok(copy) = alarm.try_clone() {
                let _ = signal_hook::low_level::pipe::register(signal, copy);
            }
        }
        let wake = pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)
            .map_err(|_| Error::bad_input("cannot create a pipe"))?;
        let mut events = VecDeque::new();
        let owner = owner.and_then(|pid| {
            let pid = rustix::process::Pid::from_raw(i32::try_from(pid).ok()?)?;
            match rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()) {
                Ok(fd) => Some(fd),
                Err(rustix::io::Errno::SRCH) => {
                    events.push_back(Event::OwnerGone);
                    None
                }
                Err(_) => None,
            }
        });
        let composited = super::x11::composited();
        let active = x.active_window();
        Ok(Self {
            x,
            window,
            gc,
            rect,
            alpha: 1.0,
            composited,
            events,
            signals,
            wake,
            owner,
            active,
        })
    }

    /// Translucent pixels need a compositing manager.
    pub fn translucent(&self) -> bool {
        self.composited
    }

    pub fn waker(&self) -> Waker {
        Waker(
            self.wake
                .1
                .try_clone()
                .expect("the wake pipe can be shared"),
        )
    }

    /// XInput2 raw button events reach this client wherever the click lands. Not on Wayland.
    pub fn watch_clicks(&mut self) -> bool {
        if super::wayland()
            || self
                .x
                .conn
                .extension_information(xinput::X11_EXTENSION_NAME)
                .ok()
                .flatten()
                .is_none()
        {
            return false;
        }
        let version = self
            .x
            .conn
            .xinput_xi_query_version(2, 2)
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        if version.is_none_or(|version| version.major_version < 2) {
            return false;
        }
        let mask = xinput::EventMask {
            deviceid: 1,
            mask: vec![XIEventMask::RAW_BUTTON_PRESS],
        };
        let selected = self
            .x
            .conn
            .xinput_xi_select_events(self.x.root, &[mask])
            .is_ok();
        let _ = self.x.conn.flush();
        selected
    }

    pub fn set_rect(&mut self, rect: PixelRect) {
        self.rect = rect;
        let aux = ConfigureWindowAux::new()
            .x(rect.x)
            .y(rect.y)
            .width(rect.width)
            .height(rect.height);
        let _ = self.x.conn.configure_window(self.window, &aux);
        let _ = self.x.conn.flush();
    }

    /// Shows `pixmap` (premultiplied RGBA, the window's size); `dirty` limits the upload.
    pub fn present(&mut self, pixmap: &Pixmap, dirty: Option<PixelRect>) {
        let area = dirty.unwrap_or(self.rect);
        let (left, top) = (
            (area.x - self.rect.x) as usize,
            (area.y - self.rect.y) as usize,
        );
        let (width, height) = (area.width as usize, area.height as usize);
        let stride = self.rect.width as usize * 4;
        let source = pixmap.data();
        let mut data = Vec::with_capacity(width * height * 4);
        for y in top..top + height {
            for pixel in
                source[y * stride + left * 4..y * stride + (left + width) * 4].chunks_exact(4)
            {
                data.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        }
        if let Ok(image) = Image::new(
            width as u16,
            height as u16,
            ScanlinePad::Pad32,
            32,
            BitsPerPixel::B32,
            ImageOrder::LsbFirst,
            Cow::Owned(data),
        ) {
            let _ = image.put(&self.x.conn, self.window, self.gc, left as i16, top as i16);
        }
        if !self.composited && dirty.is_none() {
            // No translucency: cut the window to the drawn pixels instead.
            let alpha: Vec<u8> = source.chunks_exact(4).map(|pixel| pixel[3]).collect();
            let rects = rectangles(
                &alpha,
                self.rect.width as usize,
                self.rect.height as usize,
                128,
            );
            let _ = self.x.conn.shape_rectangles(
                SO::SET,
                SK::BOUNDING,
                ClipOrdering::UNSORTED,
                self.window,
                0,
                0,
                &rects,
            );
        }
        let _ = self.x.conn.flush();
    }

    /// Bytes above zero become clickable, everything else clicks through.
    pub fn set_hit_mask(&mut self, mask: &[u8]) {
        let rects = rectangles(mask, self.rect.width as usize, self.rect.height as usize, 1);
        let _ = self.x.conn.shape_rectangles(
            SO::SET,
            SK::INPUT,
            ClipOrdering::UNSORTED,
            self.window,
            0,
            0,
            &rects,
        );
        let _ = self.x.conn.flush();
    }

    /// The whole window's opacity, applied by the compositor.
    pub fn set_alpha(&mut self, alpha: f64) {
        let alpha = alpha.clamp(0.0, 1.0);
        if (alpha - self.alpha).abs() < 1.0 / 255.0 {
            return;
        }
        self.alpha = alpha;
        let value = (alpha * f64::from(u32::MAX)).round() as u32;
        let _ = self.x.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            self.x.atoms._NET_WM_WINDOW_OPACITY,
            AtomEnum::CARDINAL,
            &[value],
        );
        let _ = self.x.conn.flush();
    }

    pub fn set_visible(&mut self, visible: bool) {
        if visible {
            let _ = self.x.conn.map_window(self.window);
            self.stay_on_top();
        } else {
            let _ = self.x.conn.unmap_window(self.window);
        }
        let _ = self.x.conn.flush();
    }

    fn stay_on_top(&self) {
        let _ = self.x.conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        );
        let _ = self.x.conn.flush();
    }

    /// Waits for X events, signals, the waker or the owner until an event arrives or `deadline`
    /// passes. Everything ready is looked at before a deadline counts, so nothing starves.
    pub fn wait(&mut self, deadline: Option<Instant>) -> Event {
        loop {
            while let Ok(Some(event)) = self.x.conn.poll_for_event() {
                self.translate(event);
            }
            if let Some(event) = self.events.pop_front() {
                return event;
            }
            let timeout = deadline.map(|deadline| {
                let left = deadline.saturating_duration_since(Instant::now());
                Timespec {
                    tv_sec: left.as_secs() as i64,
                    tv_nsec: i64::from(left.subsec_nanos()),
                }
            });
            let x_fd = self.x.conn.stream().as_fd();
            let mut fds = vec![
                PollFd::new(&x_fd, PollFlags::IN),
                PollFd::new(&self.signals, PollFlags::IN),
                PollFd::new(&self.wake.0, PollFlags::IN),
            ];
            if let Some(owner) = &self.owner {
                fds.push(PollFd::new(owner, PollFlags::IN));
            }
            let ready = poll(&mut fds, timeout.as_ref()).unwrap_or(0);
            let fired: Vec<bool> = fds.iter().map(|fd| !fd.revents().is_empty()).collect();
            drop(fds);
            if fired[1] {
                drain(&self.signals);
                self.events.push_back(Event::Stop);
            }
            if fired[2] {
                drain(&self.wake.0);
                self.events.push_back(Event::Wake);
            }
            if fired.get(3).copied().unwrap_or(false) {
                self.owner = None;
                self.events.push_back(Event::OwnerGone);
            }
            if ready == 0
                && self.events.is_empty()
                && deadline.is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Event::Timeout;
            }
        }
    }

    fn translate(&mut self, event: XEvent) {
        let root_point = |x: i16, y: i16| Point::new(x.into(), y.into());
        match event {
            XEvent::EnterNotify(e) if e.event == self.window => self
                .events
                .push_back(Event::Pointer(Some(root_point(e.root_x, e.root_y)))),
            XEvent::MotionNotify(e) if e.event == self.window => self
                .events
                .push_back(Event::Pointer(Some(root_point(e.root_x, e.root_y)))),
            XEvent::LeaveNotify(e) if e.event == self.window => {
                self.events.push_back(Event::Pointer(None))
            }
            XEvent::ButtonPress(e) if e.event == self.window && e.detail == 1 => self
                .events
                .push_back(Event::Click(root_point(e.root_x, e.root_y))),
            XEvent::XinputRawButtonPress(e) if e.detail == 1 => {
                if let Ok(Ok(pointer)) = self
                    .x
                    .conn
                    .query_pointer(self.x.root)
                    .map(|cookie| cookie.reply())
                {
                    self.events.push_back(Event::GlobalClick(root_point(
                        pointer.root_x,
                        pointer.root_y,
                    )));
                }
            }
            XEvent::RandrScreenChangeNotify(_) | XEvent::RandrNotify(_) => {
                self.events.push_back(Event::DisplaysChanged)
            }
            XEvent::PropertyNotify(e) if e.window == self.x.root => {
                if e.atom == self.x.atoms._NET_ACTIVE_WINDOW {
                    let active = self.x.active_window();
                    if active != self.active {
                        self.active = active;
                        self.stay_on_top();
                    }
                }
                if e.atom == self.x.atoms._NET_ACTIVE_WINDOW
                    || e.atom == self.x.atoms._NET_CLIENT_LIST_STACKING
                {
                    self.events.push_back(Event::WindowsChanged);
                }
            }
            XEvent::ConfigureNotify(e) if e.event == self.x.root && e.window != self.window => {
                self.events.push_back(Event::WindowsChanged)
            }
            XEvent::MapNotify(e) if e.event == self.x.root && e.window != self.window => {
                self.events.push_back(Event::WindowsChanged)
            }
            XEvent::UnmapNotify(e) if e.event == self.x.root && e.window != self.window => {
                self.events.push_back(Event::WindowsChanged)
            }
            _ => {}
        }
    }

    pub fn close(&mut self) {
        let _ = self.x.conn.destroy_window(self.window);
        let _ = self.x.conn.flush();
    }
}

fn drain(fd: &OwnedFd) {
    let mut buffer = [0u8; 64];
    while rustix::io::read(fd, &mut buffer).is_ok_and(|read| read > 0) {}
}

/// The pixels at or above `threshold` as rectangles: runs per row, merged down while rows repeat.
fn rectangles(alpha: &[u8], width: usize, height: usize, threshold: u8) -> Vec<Rectangle> {
    let mut done = Vec::new();
    let mut open: Vec<(usize, usize, usize)> = Vec::new();
    let mut previous: Vec<(usize, usize)> = Vec::new();
    for y in 0..=height {
        let mut runs = Vec::new();
        if y < height {
            let row = &alpha[y * width..(y + 1) * width];
            let mut x = 0;
            while x < width {
                if row[x] >= threshold {
                    let start = x;
                    while x < width && row[x] >= threshold {
                        x += 1;
                    }
                    runs.push((start, x));
                } else {
                    x += 1;
                }
            }
        }
        if runs != previous {
            for (start, end, top) in open.drain(..) {
                done.push(Rectangle {
                    x: start as i16,
                    y: top as i16,
                    width: (end - start) as u16,
                    height: (y - top) as u16,
                });
            }
            open = runs.iter().map(|&(start, end)| (start, end, y)).collect();
            previous = runs;
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::rectangles;

    #[test]
    fn mask_rows_merge_into_rectangles() {
        let (width, height) = (6, 4);
        let mut mask = vec![0u8; width * height];
        for y in 0..3 {
            for x in 1..4 {
                mask[y * width + x] = 255;
            }
        }
        mask[3 * width + 5] = 255;
        let rects = rectangles(&mask, width, height, 1);
        assert_eq!(rects.len(), 2);
        assert_eq!(
            (rects[0].x, rects[0].y, rects[0].width, rects[0].height),
            (1, 0, 3, 3)
        );
        assert_eq!(
            (rects[1].x, rects[1].y, rects[1].width, rects[1].height),
            (5, 3, 1, 1)
        );
    }
}
