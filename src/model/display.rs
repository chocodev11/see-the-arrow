use serde::{Deserialize, Serialize};

use super::error::{Error, Result};
use super::geom::{Point, Rect};

/// One display. `frame` and `visible` are global top-left rects in the platform's input units
/// (Windows: physical pixels of the virtual screen, X11: root window pixels); `scale` is input
/// units per layout point. Layout runs in display-local points, so every look constant stays
/// the same on every scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Display {
    pub index: usize,
    pub id: u32,
    pub frame: Rect,
    /// The part outside taskbars and panels.
    pub visible: Rect,
    pub scale: f64,
}

impl Display {
    pub fn contains(&self, point: Point) -> bool {
        self.frame.contains(point)
    }

    /// Global input units to display-local points.
    pub fn local(&self, point: Point) -> Point {
        (point - self.frame.origin()) * (1.0 / self.scale)
    }

    pub fn local_rect(&self, rect: &Rect) -> Rect {
        let origin = self.local(rect.origin());
        Rect::new(
            origin.x,
            origin.y,
            rect.width / self.scale,
            rect.height / self.scale,
        )
    }

    /// Display-local points to global input units.
    pub fn global(&self, point: Point) -> Point {
        self.frame.origin() + point * self.scale
    }

    pub fn global_rect(&self, rect: &Rect) -> Rect {
        let origin = self.global(rect.origin());
        Rect::new(
            origin.x,
            origin.y,
            rect.width * self.scale,
            rect.height * self.scale,
        )
    }

    pub fn local_bounds(&self) -> Rect {
        Rect::new(
            0.0,
            0.0,
            self.frame.width / self.scale,
            self.frame.height / self.scale,
        )
    }

    pub fn local_visible(&self) -> Rect {
        self.local_rect(&self.visible)
    }

    pub fn number(&self) -> usize {
        self.index + 1
    }
}

/// Every display, the primary first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Screens {
    pub displays: Vec<Display>,
}

impl Screens {
    /// The display that contains `point`; a point on a shared edge belongs to the right or lower one.
    pub fn containing(&self, point: Point) -> Result<&Display> {
        self.displays
            .iter()
            .find(|display| display.contains(point))
            .ok_or_else(|| {
                Error::bad_input(format!(
                    "point is outside every display: {},{} (displays: {})",
                    point.x as i64,
                    point.y as i64,
                    self.summary()
                ))
            })
    }

    /// Display by its 1-based number, as `--display N` and `doctor` count them.
    pub fn number(&self, number: i64) -> Result<&Display> {
        usize::try_from(number)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|index| self.displays.get(index))
            .ok_or_else(|| {
                Error::bad_input(format!(
                    "display {number} does not exist, there are {} ({})",
                    self.displays.len(),
                    self.summary()
                ))
            })
    }

    /// With `--display N`, coordinates are relative to that display's top-left corner.
    pub fn global(&self, point: Point, display: Option<i64>) -> Result<Point> {
        let Some(number) = display else {
            return Ok(point);
        };
        let frame = self.number(number)?.frame;
        let global = frame.origin() + point;
        if !frame.contains(global) {
            return Err(Error::bad_input(format!(
                "point {},{} is outside display {number} ({}x{})",
                point.x as i64, point.y as i64, frame.width as i64, frame.height as i64
            )));
        }
        Ok(global)
    }

    /// The rect around every display, the area a screen capture can read.
    pub fn bounds(&self) -> Rect {
        self.displays
            .iter()
            .map(|display| display.frame)
            .reduce(|all, frame| all.union(&frame))
            .unwrap_or_default()
    }

    /// One line describing every display, for error messages.
    pub fn summary(&self) -> String {
        self.displays
            .iter()
            .map(|display| {
                let frame = display.frame;
                format!(
                    "{}: {},{} {}x{}",
                    display.number(),
                    frame.x as i64,
                    frame.y as i64,
                    frame.width as i64,
                    frame.height as i64
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screens() -> Screens {
        let primary = Display {
            index: 0,
            id: 1,
            frame: Rect::new(0.0, 0.0, 2880.0, 1800.0),
            visible: Rect::new(0.0, 0.0, 2880.0, 1752.0),
            scale: 1.5,
        };
        let left = Display {
            index: 1,
            id: 2,
            frame: Rect::new(-1920.0, -300.0, 1920.0, 1080.0),
            visible: Rect::new(-1920.0, -300.0, 1920.0, 1040.0),
            scale: 1.0,
        };
        Screens {
            displays: vec![primary, left],
        }
    }

    #[test]
    fn local_and_global_round_trip_with_fractional_scale_and_negative_origin() {
        let screens = screens();
        for (display, point) in [
            (0, Point::new(1234.5, 777.0)),
            (1, Point::new(-1000.0, -120.0)),
        ] {
            let display = &screens.displays[display];
            let back = display.global(display.local(point));
            assert!(back.distance(point) < 1e-9, "{back:?} != {point:?}");
        }
        assert_eq!(
            screens.displays[0].local(Point::new(300.0, 150.0)),
            Point::new(200.0, 100.0)
        );
        assert_eq!(
            screens.displays[0].local_bounds(),
            Rect::new(0.0, 0.0, 1920.0, 1200.0)
        );
    }

    #[test]
    fn display_lookup_and_relative_points() {
        let screens = screens();
        assert_eq!(screens.containing(Point::new(-5.0, 0.0)).unwrap().index, 1);
        assert_eq!(screens.containing(Point::new(0.0, 0.0)).unwrap().index, 0);
        let outside = screens.containing(Point::new(99999.0, 0.0)).unwrap_err();
        assert_eq!(outside.code, 2);
        assert!(
            outside
                .message
                .starts_with("point is outside every display: 99999,0 (displays: 1: 0,0 2880x1800")
        );
        assert_eq!(
            screens.global(Point::new(100.0, 100.0), Some(2)).unwrap(),
            Point::new(-1820.0, -200.0)
        );
        assert!(screens.global(Point::new(1.0, 1.0), Some(99)).is_err());
        assert!(screens.global(Point::new(5000.0, 1.0), Some(1)).is_err());
        assert_eq!(screens.bounds(), Rect::new(-1920.0, -300.0, 4800.0, 2100.0));
    }
}
