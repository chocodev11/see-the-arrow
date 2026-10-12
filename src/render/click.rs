//! Where a click counts: on the sign or the shaft, never near the head or on the target mark, so a
//! click meant for the target still reaches it. A click on a copy chip copies instead of closing.

use tiny_skia::{
    BlendMode, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};

use super::sign::SignImage;
use crate::model::arrow::ShaftLine;
use crate::model::geom::{Point, Rect};
use crate::model::layout::OverlayLayout;

/// Extra width around the shaft that still counts as a hit, so the line is easy to click.
const SHAFT_SLOP: f64 = 12.0;
/// Distance around the tip, beyond the head's length, that always clicks through.
const TIP_MARGIN: f64 = 16.0;
const MARK_LINE: f64 = 6.0;

pub struct ClickRegion {
    sign: Rect,
    /// The copy chips, display-local points, in reading order.
    pub chips: Vec<Rect>,
    shaft: Vec<Point>,
    shaft_width: f64,
    tip: Point,
    tip_clearance: f64,
    mark: Option<Rect>,
}

impl ClickRegion {
    pub fn new(layout: &OverlayLayout, sign: &SignImage) -> Self {
        let metrics = layout.size.metrics();
        let shaft = match &layout.arrow.shaft {
            ShaftLine::Curve(curve) => (0..=64)
                .map(|step| curve.point(f64::from(step) / 64.0))
                .collect(),
            ShaftLine::Polyline(points) => points.clone(),
        };
        Self {
            sign: layout.sign_rect,
            chips: sign
                .chips
                .iter()
                .map(|chip| chip.rect.offset(layout.sign_rect.x, layout.sign_rect.y))
                .collect(),
            shaft,
            shaft_width: metrics.stroke + SHAFT_SLOP,
            tip: layout.arrow.tip,
            tip_clearance: metrics.head_length + TIP_MARGIN,
            mark: layout
                .mark
                .area()
                .map(|area| area.inset(-MARK_LINE, -MARK_LINE)),
        }
    }

    /// `point` in display-local points.
    #[cfg(test)]
    pub fn contains(&self, point: Point) -> bool {
        if point.distance(self.tip) <= self.tip_clearance
            || self.mark.is_some_and(|mark| mark.contains(point))
        {
            return false;
        }
        self.sign.contains(point)
            || distance_to_polyline(point, &self.shaft) <= self.shaft_width / 2.0
    }

    pub fn chip_at(&self, point: Point) -> Option<usize> {
        self.chips.iter().position(|chip| chip.contains(point))
    }

    /// The region as alpha bytes (255 where a click counts) for `width` x `height` pixels whose
    /// top-left sits at `origin` (display-local points), `scale` pixels per point.
    pub fn mask(&self, width: u32, height: u32, origin: Point, scale: f64) -> Vec<u8> {
        let Some(mut pixmap) = Pixmap::new(width.max(1), height.max(1)) else {
            return Vec::new();
        };
        let at = Transform::from_row(
            scale as f32,
            0.0,
            0.0,
            scale as f32,
            (-origin.x * scale) as f32,
            (-origin.y * scale) as f32,
        );
        let mut solid = Paint::default();
        solid.set_color_rgba8(0, 0, 0, 255);
        if let Some(rect) = tiny_skia::Rect::from_xywh(
            self.sign.x as f32,
            self.sign.y as f32,
            self.sign.width as f32,
            self.sign.height as f32,
        ) {
            pixmap.fill_rect(rect, &solid, at, None);
        }
        let mut line = PathBuilder::new();
        for (index, point) in self.shaft.iter().enumerate() {
            if index == 0 {
                line.move_to(point.x as f32, point.y as f32)
            } else {
                line.line_to(point.x as f32, point.y as f32)
            }
        }
        if let Some(path) = line.finish() {
            let stroke = Stroke {
                width: self.shaft_width as f32,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &solid, &stroke, at, None);
        }
        let clear = Paint {
            blend_mode: BlendMode::Clear,
            ..Paint::default()
        };
        if let Some(circle) = PathBuilder::from_circle(
            self.tip.x as f32,
            self.tip.y as f32,
            self.tip_clearance as f32,
        ) {
            pixmap.fill_path(&circle, &clear, FillRule::Winding, at, None);
        }
        if let Some(mark) = self.mark.and_then(|m| {
            tiny_skia::Rect::from_xywh(m.x as f32, m.y as f32, m.width as f32, m.height as f32)
        }) {
            pixmap.fill_rect(mark, &clear, at, None);
        }
        pixmap
            .pixels()
            .iter()
            .map(|pixel| if pixel.alpha() >= 128 { 255 } else { 0 })
            .collect()
    }
}

#[cfg(test)]
fn distance_to_polyline(point: Point, line: &[Point]) -> f64 {
    line.windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let along = b - a;
            let length = along.dot(along);
            let t = if length > 0.0 {
                ((point - a).dot(along) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            point.distance(a + along * t)
        })
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::appearance::{
        ApproachDirection, ArrowColor, ArrowShape, ArrowSize, ArrowStyle, SignAppearance,
        SignCorners,
    };
    use crate::model::display::Display;
    use crate::model::layout::{LayoutRequest, plan_layout};
    use crate::model::sign_text::SignText;
    use crate::model::target::TargetShape;
    use crate::render::text::TextEngine;

    #[test]
    fn clicks_count_on_sign_and_shaft_never_near_the_tip() {
        let display = Display {
            index: 0,
            id: 1,
            frame: Rect::new(0.0, 0.0, 1512.0, 982.0),
            visible: Rect::new(0.0, 0.0, 1512.0, 982.0),
            scale: 1.0,
        };
        let look = SignAppearance::new(
            ArrowColor::parse("red").unwrap(),
            ArrowSize::Medium,
            SignCorners::Round,
        );
        let sign = SignImage::new(
            &mut TextEngine::new(),
            &SignText::parse("Code {{482913}}").unwrap(),
            &look,
            1512.0,
        );
        let layout = plan_layout(&LayoutRequest {
            target: TargetShape::Point(Point::new(300.0, 250.0)),
            display,
            sign_size: (sign.width, sign.height),
            style: ArrowStyle::Arrow,
            size: ArrowSize::Medium,
            forced: Some(ApproachDirection::BottomRight),
            corners: SignCorners::Round,
            shape: ArrowShape::Bend,
            others: Vec::new(),
        });
        let region = ClickRegion::new(&layout, &sign);
        assert!(region.contains(layout.sign_rect.center()));
        assert!(!region.contains(Point::new(300.0, 250.0)));
        assert!(!region.contains(layout.arrow.tip));
        assert!(!region.contains(Point::new(1400.0, 60.0)));
        let middle = layout.arrow.shaft_bounds().center();
        assert!((0..40).any(|step| {
            region.contains(Point::new(middle.x + f64::from(step - 20) * 3.0, middle.y))
        }));
        let chip = region.chips[0];
        assert_eq!(region.chip_at(chip.center()), Some(0));
        assert_eq!(
            region.chip_at(Point::new(
                layout.sign_rect.x + 8.0,
                layout.sign_rect.mid_y()
            )),
            None
        );
        let mask = region.mask(1512, 982, Point::default(), 1.0);
        let at = |p: Point| mask[p.y as usize * 1512 + p.x as usize];
        assert_eq!(at(layout.sign_rect.center()), 255);
        assert_eq!(at(Point::new(300.0, 250.0)), 0);
    }
}
