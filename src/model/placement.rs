//! Chooses the sign position with the most free space. Pure and deterministic.

use serde::{Deserialize, Serialize};

use super::appearance::ApproachDirection;
use super::geom::{Point, Rect};

/// Side of the square zone around the target the sign never covers.
pub const KEEP_OUT_SIDE: f64 = 120.0;
/// Gap between the sign and the edge of the usable area, wide enough for the drop shadow to fade.
pub const MARGIN: f64 = 24.0;
/// Space kept between this sign and the signs of other live arrows.
pub const OTHER_GAP: f64 = 12.0;
const REACH_SCALES: [f64; 6] = [1.0, 0.75, 1.4, 0.55, 1.9, 2.5];
/// Score cost per unit of reach change: a cramped or overlong arrow only wins when the preferred
/// length does not fit.
const SHORTCUT_PENALTY: f64 = 400.0;
/// Score cost per square point of overlap with another arrow, when overlap cannot be avoided.
const OVERLAP_PENALTY: f64 = 0.05;
/// Score cost per other arrow this shaft would cross: worth a shorter reach, not a cramped sign.
const CROSSING_PENALTY: f64 = 150.0;

/// Another live arrow on the same display.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OtherArrow {
    pub sign: Rect,
    /// What it marks, or a zero-size rect at its tip.
    pub target: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub direction: ApproachDirection,
    pub sign_rect: Rect,
    pub keep_out: Rect,
}

/// What every candidate is judged against.
struct Situation {
    size: (f64, f64),
    keep_out: Rect,
    usable: Rect,
    reach: f64,
    /// Other arrows' signs and the zones around their targets: a sign covers neither.
    blocked: Vec<Rect>,
    /// Other arrows' shafts, as straight lines from sign centre to target centre.
    shafts: Vec<(Point, Point)>,
}

/// All rects are display-local top-left points; `area` is the usable part of the display. A
/// forced side is a preference: if the sign does not fit there, the roomiest other side wins,
/// because an arrow must never hide what it points at, nor what another arrow points at. Other
/// arrows' signs and targets are avoided while that is possible, and overlapped as little as
/// possible otherwise; crossing their shafts costs score.
pub fn place(
    size: (f64, f64),
    marked: Rect,
    area: Rect,
    reach: f64,
    forced: Option<ApproachDirection>,
    others: &[OtherArrow],
) -> Placement {
    let situation = Situation {
        size,
        keep_out: keep_out_zone(marked),
        usable: area.inset(MARGIN, MARGIN),
        reach,
        blocked: others
            .iter()
            .flat_map(|other| {
                [
                    other.sign.inset(-OTHER_GAP, -OTHER_GAP),
                    keep_out_zone(other.target),
                ]
            })
            .collect(),
        shafts: others
            .iter()
            .map(|other| (other.sign.center(), other.target.center()))
            .collect(),
    };
    let preferred: Vec<ApproachDirection> = forced.into_iter().collect();
    let attempts = [
        (false, &preferred[..]),
        (false, &ApproachDirection::ALL[..]),
        (true, &preferred[..]),
        (true, &ApproachDirection::ALL[..]),
    ];
    for (tolerate, sides) in attempts {
        if let Some(found) = best(&situation, sides, tolerate) {
            return found;
        }
    }
    fallback(&situation)
}

/// The best-scoring sign that fits fully and clears the keep-out zone, if any.
fn best(
    situation: &Situation,
    directions: &[ApproachDirection],
    tolerate: bool,
) -> Option<Placement> {
    let mut best: Option<(f64, Placement)> = None;
    for (rank, &direction) in directions.iter().enumerate() {
        for scale in REACH_SCALES {
            let rect = sign_rect(
                situation.size,
                situation.keep_out,
                direction,
                situation.reach * scale,
            );
            let overlap: f64 = situation
                .blocked
                .iter()
                .map(|zone| rect.overlap_area(zone))
                .sum();
            if !situation.usable.contains_rect(&rect)
                || rect.intersects(&situation.keep_out)
                || (!tolerate && overlap != 0.0)
            {
                continue;
            }
            let score = clearance(&rect, &situation.usable)
                - (scale - 1.0).abs() * SHORTCUT_PENALTY
                - rank as f64 * 4.0
                - overlap * OVERLAP_PENALTY
                - crossings(situation, &rect) as f64 * CROSSING_PENALTY;
            if best.as_ref().is_none_or(|(top, _)| score > *top) {
                best = Some((
                    score,
                    Placement {
                        direction,
                        sign_rect: rect,
                        keep_out: situation.keep_out,
                    },
                ));
            }
        }
    }
    best.map(|(_, placement)| placement)
}

/// The square keep-out zone, grown to cover the marked rect.
pub fn keep_out_zone(marked: Rect) -> Rect {
    let half = KEEP_OUT_SIDE / 2.0;
    let square = Rect::new(
        marked.mid_x() - half,
        marked.mid_y() - half,
        KEEP_OUT_SIDE,
        KEEP_OUT_SIDE,
    );
    square.union(&marked.inset(-12.0, -12.0))
}

fn sign_rect(size: (f64, f64), keep_out: Rect, direction: ApproachDirection, reach: f64) -> Rect {
    let unit = direction.unit();
    let distance = support((keep_out.width, keep_out.height), unit) + reach + support(size, unit);
    let center = keep_out.center() + unit * distance;
    Rect::new(
        center.x - size.0 / 2.0,
        center.y - size.1 / 2.0,
        size.0,
        size.1,
    )
}

/// How many other shafts, signs and target zones the shaft from `sign` to the target would cross.
fn crossings(situation: &Situation, sign: &Rect) -> usize {
    let (from, to) = (sign.center(), situation.keep_out.center());
    let shafts = situation
        .shafts
        .iter()
        .filter(|(start, end)| segments_cross(from, to, *start, *end))
        .count();
    let zones = situation
        .blocked
        .iter()
        .filter(|zone| segment_hits(from, to, zone))
        .count();
    shafts + zones
}

fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let side = |p: Point, q: Point, r: Point| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
}

/// Liang-Barsky clipping: true when the segment from `a` to `b` passes through `rect`.
fn segment_hits(a: Point, b: Point, rect: &Rect) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut enter, mut leave) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-dx, a.x - rect.min_x()),
        (dx, rect.max_x() - a.x),
        (-dy, a.y - rect.min_y()),
        (dy, rect.max_y() - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            enter = enter.max(t);
        } else {
            leave = leave.min(t);
        }
        if enter > leave {
            return false;
        }
    }
    true
}

/// Half extent of a rect of `size` along `unit`, the rect's support function.
fn support(size: (f64, f64), unit: Point) -> f64 {
    unit.x.abs() * size.0 / 2.0 + unit.y.abs() * size.1 / 2.0
}

fn clearance(rect: &Rect, area: &Rect) -> f64 {
    (rect.min_x() - area.min_x())
        .min(area.max_x() - rect.max_x())
        .min((rect.min_y() - area.min_y()).min(area.max_y() - rect.max_y()))
        .min(200.0)
}

/// No candidate fits cleanly (tiny display or huge sign): keep the sign on screen, overlap least.
fn fallback(situation: &Situation) -> Placement {
    let mut best: Option<(f64, Placement)> = None;
    for direction in ApproachDirection::ALL {
        let rect = sign_rect(
            situation.size,
            situation.keep_out,
            direction,
            situation.reach,
        )
        .clamped(&situation.usable);
        let overlap = rect.overlap_area(&situation.keep_out);
        if best.as_ref().is_none_or(|(least, _)| overlap < *least) {
            best = Some((
                overlap,
                Placement {
                    direction,
                    sign_rect: rect,
                    keep_out: situation.keep_out,
                },
            ));
        }
    }
    best.map(|(_, placement)| placement)
        .expect("eight directions give a placement")
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0.0, 33.0, 1512.0, 949.0);
    const SIGN: (f64, f64) = (421.0, 84.0);

    fn at(point: Point) -> Rect {
        Rect::new(point.x, point.y, 0.0, 0.0)
    }

    #[test]
    fn corner_and_edge_targets_stay_on_screen_and_clear_of_the_target() {
        let inner = Rect::new(4.0, 37.0, 1504.0, 941.0);
        let targets = [
            Point::new(inner.min_x(), inner.min_y()),
            Point::new(inner.max_x(), inner.min_y()),
            Point::new(inner.min_x(), inner.max_y()),
            Point::new(inner.max_x(), inner.max_y()),
            Point::new(inner.mid_x(), inner.min_y()),
            Point::new(inner.mid_x(), inner.max_y()),
            Point::new(inner.min_x(), inner.mid_y()),
            Point::new(inner.max_x(), inner.mid_y()),
        ];
        for target in targets {
            let placed = place(SIGN, at(target), AREA, 170.0, None, &[]);
            assert!(AREA.contains_rect(&placed.sign_rect), "{target:?}");
            assert!(!placed.sign_rect.intersects(&placed.keep_out), "{target:?}");
        }
    }

    #[test]
    fn placement_is_deterministic_and_honours_forced_sides() {
        let target = at(Point::new(756.0, 500.0));
        assert_eq!(
            place(SIGN, target, AREA, 170.0, None, &[]),
            place(SIGN, target, AREA, 170.0, None, &[])
        );
        for forced in [ApproachDirection::Right, ApproachDirection::TopLeft] {
            assert_eq!(
                place(SIGN, target, AREA, 170.0, Some(forced), &[]).direction,
                forced
            );
        }
        let edge = at(Point::new(AREA.max_x() - 30.0, 500.0));
        let fallback = place(SIGN, edge, AREA, 170.0, Some(ApproachDirection::Right), &[]);
        assert_ne!(fallback.direction, ApproachDirection::Right);
        assert!(!fallback.sign_rect.intersects(&fallback.keep_out));
    }

    #[test]
    fn a_second_sign_avoids_the_first_and_huge_signs_are_clamped() {
        let target = at(Point::new(756.0, 500.0));
        let first = place(SIGN, target, AREA, 170.0, None, &[]);
        let other = OtherArrow {
            sign: first.sign_rect,
            target,
        };
        let second = place(SIGN, target, AREA, 170.0, None, &[other]);
        assert!(
            !second
                .sign_rect
                .intersects(&first.sign_rect.inset(-OTHER_GAP, -OTHER_GAP))
        );
        let huge = place((1400.0, 800.0), target, AREA, 170.0, None, &[]);
        assert!(Rect::new(0.0, 0.0, 1512.0, 982.0).contains_rect(&huge.sign_rect));
        let marked = Rect::new(600.0, 400.0, 200.0, 80.0);
        assert!(keep_out_zone(marked).contains_rect(&marked));
    }

    #[test]
    fn signs_keep_off_other_targets_and_shafts() {
        let target = at(Point::new(756.0, 500.0));
        let right = Some(ApproachDirection::Right);
        let free = place(SIGN, target, AREA, 170.0, right, &[]);
        let under = OtherArrow {
            sign: Rect::new(100.0, 100.0, 200.0, 60.0),
            target: at(free.sign_rect.center()),
        };
        let placed = place(SIGN, target, AREA, 170.0, right, &[under]);
        assert!(!placed.sign_rect.intersects(&keep_out_zone(under.target)));

        let wall = OtherArrow {
            sign: Rect::new(1000.0, 100.0, 200.0, 60.0),
            target: at(Point::new(1100.0, 900.0)),
        };
        let placed = place(SIGN, target, AREA, 170.0, None, &[wall]);
        assert!(!segments_cross(
            placed.sign_rect.center(),
            placed.keep_out.center(),
            wall.sign.center(),
            wall.target.center()
        ));
    }
}
