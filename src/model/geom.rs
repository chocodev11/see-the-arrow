use std::ops::{Add, AddAssign, Mul, Sub};

use serde::{Deserialize, Serialize};

/// A point or vector in a y-down space.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn normalized(self) -> Self {
        let length = self.length();
        if length > 0.0 {
            Self::new(self.x / length, self.y / length)
        } else {
            Self::default()
        }
    }

    /// Rotated 90 degrees counter-clockwise in a y-down space.
    pub fn perpendicular(self) -> Self {
        Self::new(-self.y, self.x)
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub fn distance(self, other: Self) -> f64 {
        (other - self).length()
    }

    /// `f64::clamp` panics on an inverted range; a tiny display can produce one.
    pub fn clamped(self, bounds: Rect) -> Self {
        Self::new(
            self.x.max(bounds.min_x()).min(bounds.max_x()),
            self.y.max(bounds.min_y()).min(bounds.max_y()),
        )
    }
}

impl Add for Point {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Point {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Point {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f64> for Point {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

/// An axis-aligned rect, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_points(min: Point, max: Point) -> Self {
        Self::new(min.x, min.y, max.x - min.x, max.y - min.y)
    }

    pub fn min_x(&self) -> f64 {
        self.x
    }

    pub fn min_y(&self) -> f64 {
        self.y
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn mid_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn center(&self) -> Point {
        Point::new(self.mid_x(), self.mid_y())
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn area(&self) -> f64 {
        self.width * self.height
    }

    /// Half-open containment: a point on a shared edge belongs to exactly one rect.
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.min_x()
            && point.x < self.max_x()
            && point.y >= self.min_y()
            && point.y < self.max_y()
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.min_x() >= self.min_x()
            && other.max_x() <= self.max_x()
            && other.min_y() >= self.min_y()
            && other.max_y() <= self.max_y()
    }

    /// True when the two rects share a positive area.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.intersection(other)
            .is_some_and(|shared| shared.width > 0.0 && shared.height > 0.0)
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let min_x = self.min_x().max(other.min_x());
        let min_y = self.min_y().max(other.min_y());
        let max_x = self.max_x().min(other.max_x());
        let max_y = self.max_y().min(other.max_y());
        (max_x >= min_x && max_y >= min_y)
            .then(|| Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
    }

    pub fn overlap_area(&self, other: &Rect) -> f64 {
        self.intersection(other).map_or(0.0, |shared| shared.area())
    }

    pub fn union(&self, other: &Rect) -> Rect {
        Rect::from_points(
            Point::new(
                self.min_x().min(other.min_x()),
                self.min_y().min(other.min_y()),
            ),
            Point::new(
                self.max_x().max(other.max_x()),
                self.max_y().max(other.max_y()),
            ),
        )
    }

    /// Positive values shrink the rect, negative values grow it.
    pub fn inset(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(
            self.x + dx,
            self.y + dy,
            self.width - dx * 2.0,
            self.height - dy * 2.0,
        )
    }

    pub fn offset(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Moves the rect the minimum distance needed to lie inside `bounds`.
    pub fn clamped(&self, bounds: &Rect) -> Rect {
        let x = self.x.max(bounds.min_x()).min(bounds.max_x() - self.width);
        let y = self.y.max(bounds.min_y()).min(bounds.max_y() - self.height);
        Rect::new(x, y, self.width, self.height)
    }

    /// The rect scaled by `factor` around `anchor`.
    pub fn scaled(&self, factor: f64, anchor: Point) -> Rect {
        let origin = anchor + (self.origin() - anchor) * factor;
        Rect::new(
            origin.x,
            origin.y,
            self.width * factor,
            self.height * factor,
        )
    }

    pub fn bounding(points: impl IntoIterator<Item = Point>) -> Option<Rect> {
        points.into_iter().fold(None, |acc: Option<Rect>, point| {
            let single = Rect::new(point.x, point.y, 0.0, 0.0);
            Some(acc.map_or(single, |rect| rect.union(&single)))
        })
    }

    pub fn to_array(self) -> [f64; 4] {
        [self.x, self.y, self.width, self.height]
    }
}

/// A quadratic Bezier curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadCurve {
    pub start: Point,
    pub control: Point,
    pub end: Point,
}

impl QuadCurve {
    pub fn point(&self, t: f64) -> Point {
        self.start * ((1.0 - t) * (1.0 - t))
            + self.control * (2.0 * (1.0 - t) * t)
            + self.end * (t * t)
    }

    pub fn tangent(&self, t: f64) -> Point {
        ((self.control - self.start) * (2.0 * (1.0 - t)) + (self.end - self.control) * (2.0 * t))
            .normalized()
    }
}
