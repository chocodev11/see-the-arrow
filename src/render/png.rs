use std::path::Path;

use tiny_skia::Pixmap;

use super::animation::FrameState;
use super::scene::Scene;
use crate::model::error::{Error, Result};
use crate::model::geom::Point;

/// The rendered region: the arrow's bounding box plus this margin, on a transparent background.
/// Wider than the shadow reaches, so the shadow is never cut off.
const MARGIN: f64 = 40.0;

/// Renders the final frame into a PNG. Returns the image's top-left corner in global input units
/// and its pixels per input unit.
pub fn write(scene: &Scene, file: &Path) -> Result<[i64; 3]> {
    let display = &scene.layout.display;
    let grown = scene.layout.content_bounds().inset(-MARGIN, -MARGIN);
    let crop = grown.intersection(&display.local_bounds()).unwrap_or(grown);
    let scale = display.scale;
    let (left, top) = ((crop.x * scale).floor(), (crop.y * scale).floor());
    let (right, bottom) = ((crop.max_x() * scale).ceil(), (crop.max_y() * scale).ceil());
    let mut pixmap = Pixmap::new(
        (right - left).max(1.0) as u32,
        (bottom - top).max(1.0) as u32,
    )
    .ok_or_else(|| Error::bad_input("the arrow is too large to render"))?;
    scene.draw(
        &mut pixmap,
        Point::new(left / scale, top / scale),
        scale,
        &FrameState::FINAL,
        None,
    );
    save(&pixmap, file)?;
    Ok([
        (display.frame.x + left) as i64,
        (display.frame.y + top) as i64,
        1,
    ])
}

/// Writes `pixmap` to `file` as a PNG, with no metadata.
pub fn save(pixmap: &Pixmap, file: &Path) -> Result<()> {
    let failed = |_| Error::bad_input(format!("cannot write a PNG to {}", file.display()));
    let data = pixmap
        .encode_png()
        .map_err(|_| Error::bad_input("cannot encode the PNG"))?;
    std::fs::write(file, data).map_err(failed)
}
