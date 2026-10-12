use super::appearance::{ArrowMetrics, ArrowShape};
use super::geom::{Point, QuadCurve, Rect};
use super::spiral;

/// The sign's outline as the arrow sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignOutline {
    pub rect: Rect,
    pub corner_radius: f64,
}

/// Where the shaft leaves the sign: a point on a straight part of the sign's edge, the edge's
/// outward normal, and how wide the flare may be there. The flare never reaches into a rounded
/// corner, because a flare corner outside the rounded outline shows as a notch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignJunction {
    pub point: Point,
    pub normal: Point,
    pub half_width: f64,
}

/// A straight part shorter than this many strokes (half length) is too short for a flare,
/// e.g. the round end of a pill; the junction moves to the long top or bottom edge.
const MINIMUM_HALF_FACTOR: f64 = 0.6;
/// Keeps the flare this far from where the rounded corner starts.
const CORNER_CLEARANCE: f64 = 2.0;

struct Edge {
    normal: Point,
    /// Fixed coordinate: y for top and bottom, x for left and right.
    line: f64,
    horizontal: bool,
    straight: (f64, f64),
    exit: f64,
}

impl Edge {
    fn point(&self, along: f64) -> Point {
        if self.horizontal {
            Point::new(along, self.line)
        } else {
            Point::new(self.line, along)
        }
    }
}

impl SignJunction {
    pub fn make(sign: &SignOutline, tip: Point, stroke: f64, preferred_half: f64) -> Self {
        let edges = Self::edges(sign, tip);
        let half_length = |edge: &Edge| (edge.straight.1 - edge.straight.0) / 2.0;
        let edge = edges
            .iter()
            .find(|edge| half_length(edge) >= stroke * MINIMUM_HALF_FACTOR)
            .unwrap_or(&edges[1]);
        let half = preferred_half.min(half_length(edge)).max(0.0);
        let along = edge
            .exit
            .max(edge.straight.0 + half)
            .min(edge.straight.1 - half);
        Self {
            point: edge.point(along),
            normal: edge.normal,
            half_width: half,
        }
    }

    /// The edge the ray from the centre to the tip leaves through, then the long edge facing the tip.
    fn edges(sign: &SignOutline, tip: Point) -> [Edge; 2] {
        let rect = sign.rect;
        let inset = sign.corner_radius + CORNER_CLEARANCE;
        let horizontal = (
            rect.min_x() + inset,
            (rect.max_x() - inset).max(rect.min_x() + inset),
        );
        let vertical = (
            rect.min_y() + inset,
            (rect.max_y() - inset).max(rect.min_y() + inset),
        );
        let exit = exit_point(&rect, tip);
        let top = || Edge {
            normal: Point::new(0.0, -1.0),
            line: rect.min_y(),
            horizontal: true,
            straight: horizontal,
            exit: exit.x,
        };
        let bottom = || Edge {
            normal: Point::new(0.0, 1.0),
            line: rect.max_y(),
            horizontal: true,
            straight: horizontal,
            exit: exit.x,
        };
        let left = || Edge {
            normal: Point::new(-1.0, 0.0),
            line: rect.min_x(),
            horizontal: false,
            straight: vertical,
            exit: exit.y,
        };
        let right = || Edge {
            normal: Point::new(1.0, 0.0),
            line: rect.max_x(),
            horizontal: false,
            straight: vertical,
            exit: exit.y,
        };
        let distances = [
            (exit.y - rect.min_y()).abs(),
            (exit.y - rect.max_y()).abs(),
            (exit.x - rect.min_x()).abs(),
            (exit.x - rect.max_x()).abs(),
        ];
        let mut crossed = 0;
        for (index, distance) in distances.iter().enumerate().skip(1) {
            if *distance < distances[crossed] {
                crossed = index;
            }
        }
        let crossed = match crossed {
            0 => top(),
            1 => bottom(),
            2 => left(),
            _ => right(),
        };
        let long = if tip.y < rect.mid_y() {
            top()
        } else {
            bottom()
        };
        [crossed, long]
    }
}

/// Where the ray from the centre to the tip leaves the rect.
fn exit_point(rect: &Rect, tip: Point) -> Point {
    let center = rect.center();
    let delta = tip - center;
    let scale_x = if delta.x == 0.0 {
        f64::INFINITY
    } else {
        (rect.width / 2.0) / delta.x.abs()
    };
    let scale_y = if delta.y == 0.0 {
        f64::INFINITY
    } else {
        (rect.height / 2.0) / delta.y.abs()
    };
    center + delta * scale_x.min(scale_y).min(1.0)
}

/// How the shaft is drawn: one quadratic curve (bend, straight) or a polyline (zigzag, spiral).
#[derive(Debug, Clone, PartialEq)]
pub enum ShaftLine {
    Curve(QuadCurve),
    Polyline(Vec<Point>),
}

impl ShaftLine {
    /// The points the path runs through, control point included (a superset of the tight bounds).
    pub fn points(&self) -> Vec<Point> {
        match self {
            Self::Curve(curve) => vec![curve.start, curve.control, curve.end],
            Self::Polyline(points) => points.clone(),
        }
    }
}

/// The arrow from the sign to the target: a flared root, a curved shaft and a notched head.
/// Display-local top-left points.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowGeometry {
    pub tip: Point,
    pub tail: Point,
    /// The point the shaft's last stretch comes from.
    pub control: Point,
    pub shaft: ShaftLine,
    /// Where the shaft stops, hidden inside the head.
    pub shaft_end: Point,
    /// Unit tangent of the shaft at its end, the direction the head points.
    pub direction: Point,
    pub head_points: [Point; 4],
    pub root: ArrowRoot,
}

/// How far the shaft starts inside the sign's edge, so it grows out of the sign.
const TAIL_INSET: f64 = 12.0;
/// Bow of the shaft as a share of its length.
const BEND: f64 = 0.22;
/// The shaft leaves the sign at least this share of its length outwards before it bends.
const MINIMUM_LEAVE: f64 = 0.25;
/// The zigzag's straight run into the head, in head lengths.
const ZIGZAG_RUN_IN: f64 = 1.2;
/// The zigzag's straight lead-out from the sign, in strokes.
const ZIGZAG_LEAD_OUT: f64 = 3.5;

impl ArrowGeometry {
    pub fn new(
        tip: Point,
        sign: &SignOutline,
        bounds: Rect,
        metrics: ArrowMetrics,
        shape: ArrowShape,
    ) -> Self {
        let junction = SignJunction::make(
            sign,
            tip,
            metrics.stroke,
            metrics.stroke * ArrowRoot::BASE_FACTOR,
        );
        let tail = junction.point - junction.normal * TAIL_INSET;
        let (control, joints) = match shape {
            ArrowShape::Spiral => spiral::route(sign.rect, &junction, tip, metrics, bounds),
            _ => route(shape, metrics, &junction, tail, tip, bounds),
        };
        let direction = (tip - control).normalized();
        let base = tip - direction * metrics.head_length;
        let side = direction.perpendicular() * (metrics.head_width / 2.0);
        let shaft_end = tip - direction * (metrics.head_length * 0.7);
        let curve = QuadCurve {
            start: tail,
            control,
            end: shaft_end,
        };
        let first_leg = joints.first().map_or(curve, |&joint| QuadCurve {
            start: tail,
            control: tail + (joint - tail) * 0.5,
            end: joint,
        });
        let shaft = if joints.is_empty() {
            ShaftLine::Curve(curve)
        } else {
            ShaftLine::Polyline(
                std::iter::once(tail)
                    .chain(joints)
                    .chain(std::iter::once(shaft_end))
                    .collect(),
            )
        };
        Self {
            tip,
            tail,
            control,
            shaft,
            shaft_end,
            direction,
            head_points: [
                tip,
                base + side,
                tip - direction * (metrics.head_length * 0.78),
                base - side,
            ],
            root: ArrowRoot {
                junction,
                shaft: first_leg,
                stroke: metrics.stroke,
            },
        }
    }

    pub fn head_bounds(&self) -> Rect {
        Rect::bounding(self.head_points).unwrap_or_default()
    }

    pub fn shaft_bounds(&self) -> Rect {
        Rect::bounding(self.shaft.points()).unwrap_or_default()
    }
}

/// The shaft's last control point and, for a zigzag, its joints.
fn route(
    shape: ArrowShape,
    metrics: ArrowMetrics,
    junction: &SignJunction,
    tail: Point,
    tip: Point,
    bounds: Rect,
) -> (Point, Vec<Point>) {
    match shape {
        ArrowShape::Bend => {
            let inside = bounds.inset(metrics.stroke, metrics.stroke);
            (
                control_point(tail, tip, junction.normal, inside),
                Vec::new(),
            )
        }
        ArrowShape::Zigzag => {
            let lead_out = junction.point + junction.normal * (metrics.stroke * ZIGZAG_LEAD_OUT);
            let end = tip - (tip - lead_out).normalized() * (metrics.head_length * ZIGZAG_RUN_IN);
            let mut joints = vec![lead_out];
            joints.extend(zigzag_joints(
                lead_out,
                end,
                junction.normal,
                metrics.stroke,
            ));
            joints.push(end);
            (end, joints)
        }
        ArrowShape::Straight | ArrowShape::Spiral => (tail + (tip - tail) * 0.5, Vec::new()),
    }
}

/// Bows the shaft to the side that keeps it furthest from the display edges, makes sure it
/// first heads away from the sign, and keeps the control point on the display.
fn control_point(tail: Point, tip: Point, normal: Point, bounds: Rect) -> Point {
    let chord = tip - tail;
    let mid = tail + chord * 0.5;
    let offset = chord.perpendicular().normalized() * (chord.length() * BEND);
    let (first, second) = (mid + offset, mid - offset);
    // Ties keep the first candidate.
    let mut best = if clearance(second, &bounds) > clearance(first, &bounds) {
        second
    } else {
        first
    };
    let outwards = (best - tail).dot(normal);
    let wanted = chord.length() * MINIMUM_LEAVE;
    if outwards < wanted {
        best += normal * (wanted - outwards);
    }
    best.clamped(bounds)
}

fn clearance(point: Point, bounds: &Rect) -> f64 {
    (point.x - bounds.min_x())
        .min(bounds.max_x() - point.x)
        .min(point.y - bounds.min_y())
        .min(bounds.max_y() - point.y)
}

/// Sideways swing of each zigzag joint, as a multiple of the stroke.
const ZIGZAG_AMPLITUDE: f64 = 2.4;
/// Preferred leg length along the chord.
const ZIGZAG_LEG: f64 = 70.0;

/// Joints between `tail` and `end`, both excluded: the chord is cut into an odd number of legs,
/// the joints swing alternately left and right, the first leg heads away from the sign.
fn zigzag_joints(tail: Point, end: Point, normal: Point, stroke: f64) -> Vec<Point> {
    let chord = end - tail;
    let length = chord.length();
    let legs = leg_count(length);
    let unit = chord.normalized();
    let side = unit.perpendicular();
    let amplitude = (stroke * ZIGZAG_AMPLITUDE).min(length / legs as f64 * 0.6);
    let joints: Vec<Point> = (1..legs)
        .map(|index| {
            let along = tail + unit * (length * index as f64 / legs as f64);
            let swing = if index % 2 == 0 { -1.0 } else { 1.0 };
            along + side * (amplitude * swing)
        })
        .collect();
    match joints.first() {
        Some(&first) if (first - tail).dot(normal) < 0.0 => joints
            .into_iter()
            .map(|joint| mirror(joint, tail, unit))
            .collect(),
        _ => joints,
    }
}

fn leg_count(length: f64) -> usize {
    let raw = (length / ZIGZAG_LEG).round() as usize;
    let odd = if raw.is_multiple_of(2) { raw + 1 } else { raw };
    odd.clamp(3, 7)
}

/// Mirrors a joint to the other side of the chord.
fn mirror(point: Point, origin: Point, unit: Point) -> Point {
    let relative = point - origin;
    let along = unit * relative.dot(unit);
    origin + along - (relative - along)
}

/// The flared root where the shaft grows out of the sign, like a speech-bubble tail: two cubic
/// flanks leave the sign along its edge and run into the shaft's sides parallel to the shaft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowRoot {
    pub junction: SignJunction,
    pub shaft: QuadCurve,
    pub stroke: f64,
}

/// One side of the flare: a cubic from the sign edge into the side of the shaft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flank {
    pub on_edge: Point,
    pub near_edge: Point,
    pub near_shaft: Point,
    pub on_shaft: Point,
}

impl ArrowRoot {
    /// Preferred half width of the root at the sign edge, as a multiple of the stroke.
    pub const BASE_FACTOR: f64 = 1.8;
    /// How far outside the sign the flare has narrowed to the shaft, in strokes.
    const FLARE_FACTOR: f64 = 2.6;
    /// How far the root reaches into the sign, under the text.
    pub const DEPTH: f64 = 10.0;
    /// The flank corners sit this far inside the sign, so their outline hides under the sign body.
    const CORNER_INSET: f64 = 4.0;
    /// Shafts shorter than this many strokes shrink the flare.
    const SHORT_SHAFT_FACTOR: f64 = 9.0;

    /// Short arrows get a proportionally smaller flare, so it never runs into the head.
    pub fn scale(&self) -> f64 {
        (self.shaft.start.distance(self.shaft.end) / (self.stroke * Self::SHORT_SHAFT_FACTOR))
            .min(1.0)
    }

    pub fn flare_length(&self) -> f64 {
        self.stroke * Self::FLARE_FACTOR * self.scale()
    }

    /// The shaft parameter where the shaft is `distance` away from the junction (bisection).
    pub fn parameter(&self, distance: f64) -> f64 {
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..24 {
            let mid = (low + high) / 2.0;
            if self.shaft.point(mid).distance(self.junction.point) < distance {
                low = mid;
            } else {
                high = mid;
            }
        }
        (low + high) / 2.0
    }

    /// The two sides of the flare.
    pub fn flanks(&self) -> [Flank; 2] {
        let normal = self.junction.normal;
        let along = normal.perpendicular();
        let flare = self.flare_length();
        let neck_t = self.parameter(flare);
        let neck = self.shaft.point(neck_t);
        let tangent = self.shaft.tangent(neck_t);
        let side = tangent.perpendicular();
        let orientation = if along.dot(side) >= 0.0 { 1.0 } else { -1.0 };
        let half = self.junction.half_width;
        let base = (half * self.scale()).min(half).max(self.stroke / 2.0);
        let edge = self.junction.point - normal * Self::CORNER_INSET;
        [orientation, -orientation].map(|sign| {
            let shaft_side = neck + side * (self.stroke / 2.0 * sign * orientation);
            Flank {
                on_edge: edge + along * (base * sign),
                near_edge: edge + along * (base * 0.35 * sign),
                near_shaft: shaft_side - tangent * (flare * 0.55),
                on_shaft: shaft_side,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::appearance::{ApproachDirection, ArrowSize, SignCorners};
    use crate::model::layout::{LayoutRequest, plan_layout};
    use crate::model::target::TargetShape;
    use crate::model::{display::Display, layout::OverlayLayout};

    pub fn display() -> Display {
        Display {
            index: 0,
            id: 1,
            frame: Rect::new(0.0, 0.0, 1512.0, 982.0),
            visible: Rect::new(0.0, 33.0, 1512.0, 949.0),
            scale: 1.0,
        }
    }

    fn layout(
        direction: ApproachDirection,
        size: ArrowSize,
        shape: ArrowShape,
        corners: SignCorners,
    ) -> OverlayLayout {
        plan_layout(&LayoutRequest {
            target: TargetShape::Point(Point::new(756.0, 500.0)),
            display: display(),
            sign_size: (421.0, 84.0),
            style: crate::model::appearance::ArrowStyle::Arrow,
            size,
            forced: Some(direction),
            corners,
            shape,
            others: Vec::new(),
        })
    }

    fn near(a: Point, b: Point, limit: f64) -> bool {
        a.distance(b) <= limit
    }

    #[test]
    fn arrow_hits_target_and_stays_on_screen() {
        let bounds = display().local_bounds();
        for shape in [
            ArrowShape::Bend,
            ArrowShape::Straight,
            ArrowShape::Zigzag,
            ArrowShape::Spiral,
        ] {
            for direction in ApproachDirection::ALL {
                let layout = layout(direction, ArrowSize::Medium, shape, SignCorners::Round);
                let arrow = &layout.arrow;
                assert!(
                    near(arrow.tip, Point::new(756.0, 500.0), 0.5),
                    "{shape:?} {direction:?}"
                );
                assert!(near(arrow.head_points[0], arrow.tip, 0.5));
                assert!(
                    (arrow.shaft_end - arrow.control)
                        .normalized()
                        .dot(arrow.direction)
                        > 0.9999
                );
                let shaft = arrow.shaft_bounds();
                assert!(bounds.contains_rect(&shaft), "{shape:?} {direction:?}");
                if shape == ArrowShape::Bend {
                    let stroke = ArrowSize::Medium.metrics().stroke;
                    assert!(bounds.contains_rect(&shaft.inset(-stroke / 2.0, -stroke / 2.0)));
                }
                assert!(bounds.contains_rect(&arrow.head_bounds()));
                assert!(bounds.contains_rect(&layout.sign_rect));
                if shape != ArrowShape::Spiral {
                    assert_eq!(layout.direction, direction, "{shape:?}");
                }
            }
        }
    }

    #[test]
    fn root_flanks_meet_the_shaft_and_the_sign() {
        for size in [ArrowSize::Small, ArrowSize::Medium, ArrowSize::Large] {
            for direction in ApproachDirection::ALL {
                let layout = layout(direction, size, ArrowShape::Bend, SignCorners::Round);
                let root = layout.arrow.root;
                let stroke = size.metrics().stroke;
                let neck_t = root.parameter(root.flare_length());
                let neck = root.shaft.point(neck_t);
                let tangent = root.shaft.tangent(neck_t);
                let along = root.junction.normal.perpendicular();
                for flank in root.flanks() {
                    let off = (flank.on_shaft - neck).length();
                    assert!((off - stroke / 2.0).abs() < 0.01);
                    assert!(
                        (flank.on_shaft - flank.near_shaft)
                            .normalized()
                            .dot(tangent)
                            > 0.9999
                    );
                    assert!(
                        (flank.near_edge - flank.on_edge)
                            .normalized()
                            .dot(along)
                            .abs()
                            > 0.9999
                    );
                    assert!(layout.sign_rect.inset(-0.01, -0.01).contains(flank.on_edge));
                }
                assert!(layout.sign_rect.contains(layout.arrow.tail));
                assert!(root.shaft.tangent(0.0).dot(root.junction.normal) > 0.0);
            }
        }
        let sharp = layout(
            ApproachDirection::Left,
            ArrowSize::Small,
            ArrowShape::Bend,
            SignCorners::Sharp,
        );
        assert_eq!(sharp.arrow.root.junction.normal, Point::new(1.0, 0.0));
    }

    #[test]
    fn straight_is_collinear_and_zigzag_runs_into_the_head() {
        for direction in ApproachDirection::ALL {
            let straight = layout(
                direction,
                ArrowSize::Medium,
                ArrowShape::Straight,
                SignCorners::Round,
            );
            let ShaftLine::Curve(curve) = straight.arrow.shaft else {
                panic!("straight is a curve")
            };
            let chord = (curve.end - curve.start).normalized();
            assert!((curve.control - curve.start).normalized().dot(chord) > 0.9999);

            let zigzag = layout(
                direction,
                ArrowSize::Medium,
                ArrowShape::Zigzag,
                SignCorners::Round,
            );
            let ShaftLine::Polyline(points) = &zigzag.arrow.shaft else {
                panic!("zigzag is a polyline")
            };
            assert!(points.len() >= 5);
            let first = (points[1] - points[0]).normalized();
            assert!(first.dot(zigzag.arrow.root.junction.normal) > 0.999);
            let last = (points[points.len() - 1] - points[points.len() - 2]).normalized();
            assert!(last.dot(zigzag.arrow.direction) > 0.9999);
        }
    }
}
