//! Drawing with tiny-skia, everywhere the same: the overlay, `--png` and the tests.

pub mod animation;
pub mod click;
pub mod png;
pub mod scale;
pub mod scene;
pub mod sign;
pub mod text;

use tiny_skia::{Color, Paint, Path, PathBuilder};

use crate::model::appearance::ArrowColor;
use crate::model::geom::Rect;

pub fn color(color: ArrowColor, alpha: f64) -> Color {
    Color::from_rgba(
        color.red as f32,
        color.green as f32,
        color.blue as f32,
        alpha.clamp(0.0, 1.0) as f32,
    )
    .unwrap_or(Color::BLACK)
}

pub fn paint(fill: ArrowColor, alpha: f64) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(color(fill, alpha));
    paint.anti_alias = true;
    paint
}

/// A rounded rect with circular corners, the radius clamped to half the shorter side.
pub fn rounded_rect(rect: Rect, radius: f64) -> Option<Path> {
    let radius = radius.min(rect.width / 2.0).min(rect.height / 2.0).max(0.0) as f32;
    let (x, y, max_x, max_y) = (
        rect.x as f32,
        rect.y as f32,
        rect.max_x() as f32,
        rect.max_y() as f32,
    );
    if radius <= 0.0 {
        return tiny_skia::Rect::from_ltrb(x, y, max_x, max_y).map(PathBuilder::from_rect);
    }
    // Cubic approximation of a quarter circle.
    let k = 0.552_284_8 * radius;
    let mut path = PathBuilder::new();
    path.move_to(x + radius, y);
    path.line_to(max_x - radius, y);
    path.cubic_to(
        max_x - radius + k,
        y,
        max_x,
        y + radius - k,
        max_x,
        y + radius,
    );
    path.line_to(max_x, max_y - radius);
    path.cubic_to(
        max_x,
        max_y - radius + k,
        max_x - radius + k,
        max_y,
        max_x - radius,
        max_y,
    );
    path.line_to(x + radius, max_y);
    path.cubic_to(
        x + radius - k,
        max_y,
        x,
        max_y - radius + k,
        x,
        max_y - radius,
    );
    path.line_to(x, y + radius);
    path.cubic_to(x, y + radius - k, x + radius - k, y, x + radius, y);
    path.close();
    path.finish()
}

/// Blurs an alpha channel in place, close to a Gaussian of `sigma` pixels: three box blurs.
pub fn blur_alpha(alpha: &mut [u8], width: usize, height: usize, sigma: f64) {
    if sigma < 0.5 || width == 0 || height == 0 {
        return;
    }
    let mut scratch = vec![0u8; alpha.len()];
    for radius in box_radii(sigma) {
        box_pass(alpha, &mut scratch, width, height, radius, true);
        box_pass(&scratch, alpha, width, height, radius, false);
    }
}

/// Box radii whose three passes approximate a Gaussian (Kovesi's "boxes for Gauss").
fn box_radii(sigma: f64) -> [usize; 3] {
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let upper = lower + 2;
    let lower_f = lower as f64;
    let count = ((12.0 * sigma * sigma - 3.0 * lower_f * lower_f - 12.0 * lower_f - 9.0)
        / (-4.0 * lower_f - 4.0))
        .round() as i64;
    std::array::from_fn(|pass| {
        let size = if (pass as i64) < count { lower } else { upper };
        ((size.max(1) - 1) / 2) as usize
    })
}

/// One sliding-window box blur along rows (`horizontal`) or columns, edges treated as transparent.
fn box_pass(
    source: &[u8],
    target: &mut [u8],
    width: usize,
    height: usize,
    radius: usize,
    horizontal: bool,
) {
    let (lines, length) = if horizontal {
        (height, width)
    } else {
        (width, height)
    };
    let index = |line: usize, at: usize| {
        if horizontal {
            line * width + at
        } else {
            at * width + line
        }
    };
    let span = (radius * 2 + 1) as u32;
    for line in 0..lines {
        let mut sum: u32 = (0..=radius.min(length - 1))
            .map(|at| u32::from(source[index(line, at)]))
            .sum();
        for at in 0..length {
            target[index(line, at)] = ((sum + span / 2) / span) as u8;
            if at + radius + 1 < length {
                sum += u32::from(source[index(line, at + radius + 1)]);
            }
            if at >= radius {
                sum -= u32::from(source[index(line, at - radius)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_spreads_and_keeps_mass() {
        let (width, height) = (64, 64);
        let mut alpha = vec![0u8; width * height];
        for y in 28..36 {
            for x in 28..36 {
                alpha[y * width + x] = 255;
            }
        }
        let before: u32 = alpha.iter().map(|&a| u32::from(a)).sum();
        blur_alpha(&mut alpha, width, height, 4.0);
        let after: u32 = alpha.iter().map(|&a| u32::from(a)).sum();
        assert!(alpha[32 * width + 20] > 0, "the blur reaches outwards");
        assert!(alpha[32 * width + 32] < 255);
        assert!((f64::from(after) / f64::from(before) - 1.0).abs() < 0.05);
        assert_eq!(alpha[0], 0);
    }
}
