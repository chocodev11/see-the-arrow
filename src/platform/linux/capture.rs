//! `screenshot`: reads the root window, whose pixels are the coordinates `--at` and `--rect` use.
//! With a compositor the root shows what the screen shows, arrows included.

use tiny_skia::{IntSize, Pixmap};
use x11rb::connection::Connection;
use x11rb::image::{Image, PixelLayout};

use super::{wayland, x11};
use crate::model::error::{Error, Result};
use crate::platform::PixelRect;

/// What the screen shows in `rect`, root pixels. Opaque RGBA.
pub fn capture(rect: PixelRect) -> Result<Pixmap> {
    if wayland() {
        return Err(Error::permission(
            "Capturing the screen needs an X11 session, because Wayland does not let apps read the screen: log in to \
             an X11 session (e.g. 'GNOME on Xorg', 'Plasma (X11)'). Run 'bigarrow doctor' to check.",
        ));
    }
    let x = x11::connect()?;
    let screen = &x.conn.setup().roots[x.screen];
    let root = PixelRect {
        x: 0,
        y: 0,
        width: u32::from(screen.width_in_pixels),
        height: u32::from(screen.height_in_pixels),
    };
    if rect.intersect(&root) != Some(rect) {
        return Err(Error::bad_input("the area reaches outside the X screen"));
    }
    let (image, visual) = Image::get(
        &x.conn,
        x.root,
        rect.x as i16,
        rect.y as i16,
        rect.width as u16,
        rect.height as u16,
    )
    .map_err(|_| {
        Error::permission(
            "cannot read the screen: the X server refused. XWayland (Wayland sessions, WSLg) keeps the screen from \
             apps; an Xorg session or Xvfb lets them read it.",
        )
    })?;
    let layout = screen
        .allowed_depths
        .iter()
        .flat_map(|depth| &depth.visuals)
        .find(|candidate| candidate.visual_id == visual)
        .cloned()
        .and_then(|visual| PixelLayout::from_visual_type(visual).ok())
        .ok_or_else(|| Error::bad_input("the X screen uses a pixel format bigarrow cannot read"))?;
    let mut data = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);
    for y in 0..rect.height as u16 {
        for x in 0..rect.width as u16 {
            let (red, green, blue) = layout.decode(image.get_pixel(x, y));
            data.extend_from_slice(&[(red >> 8) as u8, (green >> 8) as u8, (blue >> 8) as u8, 255]);
        }
    }
    IntSize::from_wh(rect.width, rect.height)
        .and_then(|size| Pixmap::from_vec(data, size))
        .ok_or_else(|| Error::bad_input("the area is too large to capture"))
}
