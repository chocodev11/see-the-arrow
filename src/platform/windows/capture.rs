//! `screenshot`: copies screen pixels with GDI. The process is per-monitor DPI aware, so the
//! screen DC hands out the same physical pixels `--at` and `--rect` use.

use tiny_skia::{IntSize, Pixmap};
use windows::Win32::Graphics::Gdi::{BitBlt, CAPTUREBLT, GetDC, ReleaseDC, SRCCOPY};

use super::surface::Surface;
use crate::model::error::{Error, Result};
use crate::platform::PixelRect;

/// What the screen shows in `rect`, global pixels, arrows included (`CAPTUREBLT` copies layered
/// windows too). Opaque RGBA.
pub fn capture(rect: PixelRect) -> Result<Pixmap> {
    let mut surface = Surface::new(rect.width, rect.height)?;
    let screen = unsafe { GetDC(None) };
    let copied = unsafe {
        BitBlt(
            surface.dc,
            0,
            0,
            rect.width as i32,
            rect.height as i32,
            Some(screen),
            rect.x,
            rect.y,
            SRCCOPY | CAPTUREBLT,
        )
    };
    unsafe { ReleaseDC(None, screen) };
    copied.map_err(|_| {
        Error::permission(
            "cannot read the screen: Windows refuses while it is locked or shows a secure desktop (UAC, sign-in)",
        )
    })?;
    let mut data = surface.pixels().to_vec();
    for pixel in data.as_chunks_mut::<4>().0 {
        *pixel = [pixel[2], pixel[1], pixel[0], 255];
    }
    IntSize::from_wh(rect.width, rect.height)
        .and_then(|size| Pixmap::from_vec(data, size))
        .ok_or_else(|| Error::bad_input("the area is too large to capture"))
}
