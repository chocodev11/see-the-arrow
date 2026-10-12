use super::appearance::{ApproachDirection, ArrowShape, ArrowSize, ArrowStyle, SignCorners};
use super::arrow::{ArrowGeometry, SignOutline};
use super::display::Display;
use super::geom::{Point, Rect};
use super::placement::{self, OtherArrow};
use super::spiral;
use super::target::TargetShape;

/// Padding between a highlighted rect and its box.
const BOX_PADDING: f64 = 8.0;
/// Gap between a box or ring and the arrow tip.
const MARK_GAP: f64 = 6.0;

/// The mark drawn on the target itself, besides the arrow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetMark {
    None,
    Box(Rect),
    Ring { center: Point, radius: f64 },
}

impl TargetMark {
    /// The area the mark occupies, used as the zone the sign keeps clear of.
    pub fn area(&self) -> Option<Rect> {
        match *self {
            Self::None => None,
            Self::Box(rect) => Some(rect),
            Self::Ring { center, radius } => Some(Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            )),
        }
    }
}

/// Everything the overlay draws on one display, in display-local top-left points.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayLayout {
    pub display: Display,
    pub mark: TargetMark,
    pub sign_rect: Rect,
    pub direction: ApproachDirection,
    pub arrow: ArrowGeometry,
    pub size: ArrowSize,
    pub corners: SignCorners,
}

#[derive(Debug, Clone)]
pub struct LayoutRequest {
    /// Global input units.
    pub target: TargetShape,
    pub display: Display,
    /// Sign size in points.
    pub sign_size: (f64, f64),
    pub style: ArrowStyle,
    pub size: ArrowSize,
    pub forced: Option<ApproachDirection>,
    pub corners: SignCorners,
    pub shape: ArrowShape,
    /// Other live arrows on this display, display-local points.
    pub others: Vec<OtherArrow>,
}

pub fn plan_layout(request: &LayoutRequest) -> OverlayLayout {
    let display = &request.display;
    let local = match request.target {
        TargetShape::Point(point) => TargetShape::Point(display.local(point)),
        TargetShape::Rect(rect) => TargetShape::Rect(display.local_rect(&rect)),
    };
    let mark = mark_for(local, request.style);
    let anchor = local.anchor();
    let marked = mark
        .area()
        .unwrap_or(Rect::new(anchor.x, anchor.y, 0.0, 0.0));
    let metrics = request.size.metrics();
    // A spiral's loop is placed as part of the sign, so it stays on screen and clear of the target.
    let pad = if request.shape == ArrowShape::Spiral {
        spiral::padding(request.sign_size, metrics)
    } else {
        (0.0, 0.0)
    };
    let padded = (
        request.sign_size.0 + pad.0 * 2.0,
        request.sign_size.1 + pad.1 * 2.0,
    );
    let placed = placement::place(
        padded,
        marked,
        display.local_visible(),
        metrics.reach,
        request.forced,
        &request.others,
    );
    let sign_rect = placed.sign_rect.inset(pad.0, pad.1);
    let tip = tip_point(local, mark, sign_rect.center());
    let outline = SignOutline {
        rect: sign_rect,
        corner_radius: request.corners.radius(sign_rect.height),
    };
    let arrow = ArrowGeometry::new(
        tip,
        &outline,
        display.local_bounds(),
        metrics,
        request.shape,
    );
    OverlayLayout {
        display: display.clone(),
        mark,
        sign_rect,
        direction: placed.direction,
        arrow,
        size: request.size,
        corners: request.corners,
    }
}

fn mark_for(target: TargetShape, style: ArrowStyle) -> TargetMark {
    match (style, target) {
        (ArrowStyle::Ring, _) => {
            let radius = (target
                .rect()
                .map_or(0.0, |rect| rect.width.hypot(rect.height) / 2.0)
                + 12.0)
                .max(36.0);
            TargetMark::Ring {
                center: target.anchor(),
                radius,
            }
        }
        (_, TargetShape::Rect(rect)) => TargetMark::Box(rect.inset(-BOX_PADDING, -BOX_PADDING)),
        (ArrowStyle::Box, TargetShape::Point(point)) => {
            TargetMark::Box(Rect::new(point.x - 36.0, point.y - 36.0, 72.0, 72.0))
        }
        (ArrowStyle::Arrow, TargetShape::Point(_)) => TargetMark::None,
    }
}

/// Point targets are hit exactly. Boxes are hit at the edge midpoint nearest the sign, rings at
/// the point facing the sign.
fn tip_point(target: TargetShape, mark: TargetMark, sign: Point) -> Point {
    match mark {
        TargetMark::None => target.anchor(),
        TargetMark::Ring { center, radius } => {
            center + (sign - center).normalized() * (radius + MARK_GAP)
        }
        TargetMark::Box(rect) => {
            let midpoints = [
                (
                    Point::new(rect.mid_x(), rect.min_y()),
                    Point::new(0.0, -1.0),
                ),
                (Point::new(rect.mid_x(), rect.max_y()), Point::new(0.0, 1.0)),
                (
                    Point::new(rect.min_x(), rect.mid_y()),
                    Point::new(-1.0, 0.0),
                ),
                (Point::new(rect.max_x(), rect.mid_y()), Point::new(1.0, 0.0)),
            ];
            let mut nearest = midpoints[0];
            for candidate in &midpoints[1..] {
                if candidate.0.distance(sign) < nearest.0.distance(sign) {
                    nearest = *candidate;
                }
            }
            nearest.0 + nearest.1 * MARK_GAP
        }
    }
}

impl OverlayLayout {
    /// Union of sign, arrow and mark in display-local points.
    pub fn content_bounds(&self) -> Rect {
        let mut rect = self
            .sign_rect
            .union(&self.arrow.shaft_bounds())
            .union(&self.arrow.head_bounds());
        if let Some(area) = self.mark.area() {
            rect = rect.union(&area);
        }
        rect
    }

    /// The sign in global input units.
    pub fn global_sign(&self) -> Rect {
        self.display.global_rect(&self.sign_rect)
    }

    /// What the arrow marks, or its bare tip, in global input units.
    pub fn global_target(&self) -> Rect {
        let tip = self.arrow.tip;
        let marked = self
            .mark
            .area()
            .unwrap_or(Rect::new(tip.x, tip.y, 0.0, 0.0));
        self.display.global_rect(&marked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display() -> Display {
        Display {
            index: 0,
            id: 1,
            frame: Rect::new(0.0, 0.0, 1512.0, 982.0),
            visible: Rect::new(0.0, 0.0, 1512.0, 982.0),
            scale: 1.0,
        }
    }

    #[test]
    fn rect_targets_get_a_box_and_the_tip_sits_at_its_edge() {
        let layout = plan_layout(&LayoutRequest {
            target: TargetShape::Rect(Rect::new(600.0, 400.0, 200.0, 80.0)),
            display: display(),
            sign_size: (220.0, 64.0),
            style: ArrowStyle::Arrow,
            size: ArrowSize::Medium,
            forced: Some(ApproachDirection::Bottom),
            corners: SignCorners::Round,
            shape: ArrowShape::Bend,
            others: Vec::new(),
        });
        let TargetMark::Box(rect) = layout.mark else {
            panic!("a rect target gets a box")
        };
        assert_eq!(rect, Rect::new(592.0, 392.0, 216.0, 96.0));
        assert!(
            layout
                .arrow
                .tip
                .distance(Point::new(rect.mid_x(), rect.max_y() + 6.0))
                < 0.5
        );
    }
}
