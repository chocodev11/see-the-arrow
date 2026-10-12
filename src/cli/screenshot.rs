//! `screenshot`: saves what the screen shows to a PNG in the pixels `--at` and `--rect` use, so an
//! agent can find a spot no accessibility API reaches (Blender, canvases, games) and point at it.

use std::path::Path;

use super::options::ScreenshotArgs;
use super::output::{self, number};
use super::targeting;
use crate::model::error::{Error, Result};
use crate::model::geom::Rect;
use crate::model::matcher::WindowQuery;
use crate::model::result::ScreenshotResult;
use crate::model::target::parse_rect;
use crate::platform::{self, PixelRect};
use crate::render::{png, scale};

pub fn run(args: &ScreenshotArgs) -> Result<()> {
    let window = args.window.as_deref().map(WindowQuery::parse).transpose()?;
    if window.is_some() && (args.rect.is_some() || args.display.is_some()) {
        return Err(Error::bad_input(
            "give one area: --display, --window or --rect (--display N with --rect makes the rect relative to it)",
        ));
    }
    if let Some(home) = &window
        && !args.no_raise
    {
        targeting::raise_or_warn(home, "capturing anyway")?;
    }
    // Read after the raise: a window that was minimized has only now got its place.
    let screens = platform::screens()?;
    let area = match (&window, &args.rect) {
        (Some(query), _) => targeting::windows_named(query, &screens)?.1[0].bounds,
        (None, Some(rect)) => {
            let rect = parse_rect(rect)?;
            let origin = screens.global(rect.origin(), args.display)?;
            Rect::new(origin.x, origin.y, rect.width, rect.height)
        }
        (None, None) => screens.number(args.display.unwrap_or(1))?.frame,
    };
    let rect = PixelRect::enclosing(area)
        .intersect(&PixelRect::enclosing(screens.bounds()))
        .ok_or_else(|| {
            Error::bad_input(format!(
                "rect {} is outside every display (displays: {})",
                targeting::rect_text(area),
                screens.summary()
            ))
        })?;
    let captured = platform::capture(rect)?;
    let image = match args.max_size {
        Some(max) => scale::fit(captured, max),
        None => captured,
    };
    let path = std::path::absolute(Path::new(&args.png))
        .map_err(|_| Error::bad_input(format!("cannot write a PNG to {}", args.png)))?;
    png::save(&image, &path)?;
    let (x, y) = (i64::from(rect.x), i64::from(rect.y));
    let (width, height) = (i64::from(rect.width), i64::from(rect.height));
    let size = [i64::from(image.width()), i64::from(image.height())];
    if args.json {
        output::json(&ScreenshotResult {
            ok: true,
            path: path.display().to_string(),
            rect: [x, y, width, height],
            size,
        });
    } else if size == [width, height] {
        output::print(&format!(
            "bigarrow: wrote {}, {width}x{height} at {x},{y}",
            path.display()
        ));
    } else {
        output::print(&format!(
            "bigarrow: wrote {}, {}x{} showing {width}x{height} at {x},{y} (one image pixel = {} screen pixels)",
            path.display(),
            size[0],
            size[1],
            number(width as f64 / size[0] as f64)
        ));
    }
    Ok(())
}
