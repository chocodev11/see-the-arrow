//! Shrinks screen captures for `screenshot --max-size`, keeping small text readable.

use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

/// `pixmap` with its longer side at most `max` pixels, the aspect ratio kept. Halves with a 2x2
/// box average while that stays at or above the target, so text keeps its strokes, then finishes
/// with one bicubic step.
pub fn fit(pixmap: Pixmap, max: u32) -> Pixmap {
    let (width, height) = (pixmap.width(), pixmap.height());
    let longer = width.max(height);
    if longer <= max {
        return pixmap;
    }
    let factor = f64::from(max) / f64::from(longer);
    let target = (
        ((f64::from(width) * factor).round() as u32).max(1),
        ((f64::from(height) * factor).round() as u32).max(1),
    );
    let mut current = pixmap;
    while current.width() / 2 >= target.0 && current.height() / 2 >= target.1 {
        current = halve(&current);
    }
    if (current.width(), current.height()) == target {
        return current;
    }
    let Some(mut scaled) = Pixmap::new(target.0, target.1) else {
        return current;
    };
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let transform = Transform::from_scale(
        target.0 as f32 / current.width() as f32,
        target.1 as f32 / current.height() as f32,
    );
    scaled.draw_pixmap(0, 0, current.as_ref(), &paint, transform, None);
    scaled
}

/// Half the size, each pixel the average of the 2x2 block it covers; an odd last row or column
/// is dropped.
fn halve(pixmap: &Pixmap) -> Pixmap {
    let (width, height) = (pixmap.width() / 2, pixmap.height() / 2);
    let stride = pixmap.width() as usize * 4;
    let source = pixmap.data();
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        let top = y * 2 * stride;
        let bottom = top + stride;
        for x in 0..width as usize {
            let left = x * 8;
            for channel in 0..4 {
                let sum = u16::from(source[top + left + channel])
                    + u16::from(source[top + left + 4 + channel])
                    + u16::from(source[bottom + left + channel])
                    + u16::from(source[bottom + left + 4 + channel]);
                data.push(((sum + 2) / 4) as u8);
            }
        }
    }
    tiny_skia::IntSize::from_wh(width, height)
        .and_then(|size| Pixmap::from_vec(data, size))
        .expect("half of a valid pixmap is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Color;

    fn solid(width: u32, height: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap();
        pixmap.fill(Color::from_rgba8(33, 178, 75, 255));
        pixmap
    }

    #[test]
    fn fits_the_longer_side_and_keeps_the_aspect_ratio() {
        let fitted = fit(solid(1920, 1020), 1280);
        assert_eq!((fitted.width(), fitted.height()), (1280, 680));
        let tall = fit(solid(300, 1200), 400);
        assert_eq!((tall.width(), tall.height()), (100, 400));
        let small = fit(solid(64, 48), 1280);
        assert_eq!((small.width(), small.height()), (64, 48));
        let halved = fit(solid(64, 48), 32);
        assert_eq!((halved.width(), halved.height()), (32, 24));
    }

    #[test]
    fn a_solid_colour_stays_the_same_colour() {
        for fitted in [fit(solid(1920, 1020), 1280), fit(solid(64, 48), 32)] {
            for pixel in fitted.pixels() {
                let pixel = pixel.demultiply();
                let channels = [pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()];
                for (got, want) in channels.into_iter().zip([33, 178, 75, 255]) {
                    assert!(got.abs_diff(want) <= 1, "{channels:?}");
                }
            }
        }
    }
}
