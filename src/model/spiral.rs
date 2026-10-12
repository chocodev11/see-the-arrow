//! `--shape spiral`: the shaft leaves the sign, runs once around it on a loop that widens as it
//! goes, so the loop never touches itself, and then curves to the target. The loop turns the way
//! that ends heading towards the target, so the last leg sweeps in instead of doubling back.

use std::f64::consts::{PI, SQRT_2};

use super::appearance::ArrowMetrics;
use super::arrow::SignJunction;
use super::geom::{Point, QuadCurve, Rect};

const LOOP_SAMPLES: usize = 96;
const LEG_SAMPLES: usize = 24;
/// Distance from the sign to the loop where it starts, in strokes.
const GAP_FACTOR: f64 = 2.2;
/// How much wider the loop is when it comes back round, in strokes.
const GROWTH_FACTOR: f64 = 2.8;
/// The last stretch runs straight into the head, in head lengths.
const RUN_IN: f64 = 1.2;
/// The leg to the target starts along the loop's tangent for this share of its length.
const LEG_TANGENT: f64 = 0.45;
/// The shaft curves into the loop over this many loop samples.
const ENTRY_SAMPLES: usize = 8;
/// The straight stub out of the sign before that curve, in strokes.
const ENTRY_STUB: f64 = 1.0;
/// The widest border drawn around the shaft, in points.
const BORDER_ALLOWANCE: f64 = 6.0;

/// The loop and the leg to the target as polyline joints, and the point the head points from.
pub fn route(
    sign: Rect,
    junction: &SignJunction,
    tip: Point,
    metrics: ArrowMetrics,
    bounds: Rect,
) -> (Point, Vec<Point>) {
    let clockwise = loop_points(sign, junction, metrics, 1.0);
    let anticlockwise = loop_points(sign, junction, metrics, -1.0);
    // Ties keep the clockwise loop.
    let ring = if heading(&anticlockwise, tip) > heading(&clockwise, tip) {
        anticlockwise
    } else {
        clockwise
    };
    let exit = *ring.last().unwrap_or(&tip);
    let end = tip - (tip - exit).normalized() * (metrics.head_length * RUN_IN);
    let leg = leg_points(&ring, end);
    let joints: Vec<Point> = ring
        .into_iter()
        .chain(leg)
        .map(|point| point.clamped(bounds))
        .collect();
    (*joints.last().unwrap_or(&end), joints)
}

/// Room the loop needs on each side of a sign of `size`, so placement keeps the whole loop on
/// screen and clear of the target: the widest loop plus half a shaft and its border.
pub fn padding(size: (f64, f64), metrics: ArrowMetrics) -> (f64, f64) {
    let extra =
        metrics.stroke * (GAP_FACTOR + GROWTH_FACTOR) + metrics.stroke / 2.0 + BORDER_ALLOWANCE;
    (
        size.0 / 2.0 * (SQRT_2 - 1.0) + extra,
        size.1 / 2.0 * (SQRT_2 - 1.0) + extra,
    )
}

/// One turn around the sign's centre (1: clockwise on screen, -1: anticlockwise), starting where
/// the shaft leaves the sign.
fn loop_points(
    sign: Rect,
    junction: &SignJunction,
    metrics: ArrowMetrics,
    turn: f64,
) -> Vec<Point> {
    let center = sign.center();
    let gap = metrics.stroke * GAP_FACTOR;
    let growth = metrics.stroke * GROWTH_FACTOR;
    // The smallest ellipse around a rectangle has semi-axes of √2 times its half sides.
    let radii = (
        sign.width / 2.0 * SQRT_2 + gap,
        sign.height / 2.0 * SQRT_2 + gap,
    );
    let offset = junction.point - center;
    let start = (offset.y / radii.1).atan2(offset.x / radii.0);
    let ring: Vec<Point> = (0..=LOOP_SAMPLES)
        .map(|index| {
            let share = index as f64 / LOOP_SAMPLES as f64;
            let angle = start + turn * 2.0 * PI * share;
            let grown = growth * share;
            center
                + Point::new(
                    (radii.0 + grown) * angle.cos(),
                    (radii.1 + grown) * angle.sin(),
                )
        })
        .collect();
    let stub = junction.point + junction.normal * (metrics.stroke * ENTRY_STUB);
    let entry = QuadCurve {
        start: stub,
        control: ring[0],
        end: ring[ENTRY_SAMPLES],
    };
    (0..ENTRY_SAMPLES)
        .map(|index| entry.point(index as f64 / ENTRY_SAMPLES as f64))
        .chain(ring.into_iter().skip(ENTRY_SAMPLES))
        .collect()
}

/// How well the loop's last step points at the target: 1 straight at it, -1 away from it.
fn heading(ring: &[Point], tip: Point) -> f64 {
    match ring {
        [.., previous, exit] => (*exit - *previous)
            .normalized()
            .dot((tip - *exit).normalized()),
        _ => -1.0,
    }
}

/// From the loop's end, on along its tangent, then bending into the straight run to the head.
fn leg_points(ring: &[Point], end: Point) -> Vec<Point> {
    let [.., previous, exit] = ring else {
        return vec![end];
    };
    let tangent = (*exit - *previous).normalized();
    let control = *exit + tangent * (exit.distance(end) * LEG_TANGENT);
    let curve = QuadCurve {
        start: *exit,
        control,
        end,
    };
    (1..=LEG_SAMPLES)
        .map(|index| curve.point(index as f64 / LEG_SAMPLES as f64))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::model::appearance::{
        ApproachDirection, ArrowShape, ArrowSize, ArrowStyle, SignCorners,
    };
    use crate::model::arrow::ShaftLine;
    use crate::model::display::Display;
    use crate::model::geom::{Point, Rect};
    use crate::model::layout::{LayoutRequest, plan_layout};
    use crate::model::target::TargetShape;

    #[test]
    fn spiral_loops_once_around_the_sign_without_touching_it() {
        let display = Display {
            index: 0,
            id: 1,
            frame: Rect::new(0.0, 0.0, 1512.0, 982.0),
            visible: Rect::new(0.0, 33.0, 1512.0, 949.0),
            scale: 1.0,
        };
        for direction in ApproachDirection::ALL {
            let layout = plan_layout(&LayoutRequest {
                target: TargetShape::Point(Point::new(756.0, 500.0)),
                display: display.clone(),
                sign_size: (300.0, 70.0),
                style: ArrowStyle::Arrow,
                size: ArrowSize::Medium,
                forced: Some(direction),
                corners: SignCorners::Round,
                shape: ArrowShape::Spiral,
                others: Vec::new(),
            });
            let ShaftLine::Polyline(points) = &layout.arrow.shaft else {
                panic!("spiral is a polyline")
            };
            let stroke = ArrowSize::Medium.metrics().stroke;
            let ring = &points[1..=97];
            let grown = layout.sign_rect.inset(-stroke, -stroke);
            assert!(
                ring[8..].iter().all(|point| !grown.contains(*point)),
                "{direction:?}"
            );
            let center = layout.sign_rect.center();
            let mut turned = 0.0;
            for pair in ring.windows(2) {
                let (a, b) = (pair[0] - center, pair[1] - center);
                turned += (a.x * b.y - a.y * b.x).atan2(a.dot(b));
            }
            assert!(
                turned.abs() > 0.85 * 2.0 * std::f64::consts::PI,
                "{direction:?} turned {turned}"
            );
            assert!(ring[0].distance(ring[ring.len() - 1]) >= 2.5 * stroke);
            assert!(layout.arrow.tip.distance(Point::new(756.0, 500.0)) < 0.5);
        }
    }
}
