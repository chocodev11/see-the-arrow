//! One arrow, drawn in one pass in layer order: every contrast border below every
//! coloured fill, so head and shaft share one continuous border and the shaft grows out of the
//! sign without a seam. By default one drop shadow for the whole arrow.

use tiny_skia::{
    FillRule, LineCap, LineJoin, Path, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash,
    Transform,
};

use super::animation::{FrameState, HEAD_OVERSHOOT, PULSE_SCALE, SIGN_OVERSHOOT};
use super::sign::SignImage;
use super::{blur_alpha, paint, rounded_rect};
use crate::model::appearance::{ArrowBorder, ArrowColor, SignAppearance};
use crate::model::arrow::ShaftLine;
use crate::model::geom::{Point, Rect};
use crate::model::layout::{OverlayLayout, TargetMark};

/// The contrast border, added to a stroke's width: 3 pt on each side.
const OUTLINE_WIDTH: f64 = 6.0;
/// The thin black line, added to a stroke's width: 1.5 pt on each side.
const EDGE_WIDTH: f64 = 3.0;
const MARK_LINE: f64 = 6.0;
const MARK_CORNER: f64 = 12.0;
const SHADOW_ALPHA: f64 = 0.35;
/// Core Animation's shadow radius 10 is a Gaussian of about half that.
const SHADOW_SIGMA: f64 = 5.0;
const SHADOW_DROP: f64 = 3.0;
/// How far a shadow reaches past what casts it.
const SHADOW_REACH: f64 = SHADOW_SIGMA * 3.0 + SHADOW_DROP;

pub struct Scene {
    pub layout: OverlayLayout,
    pub sign: SignImage,
    pub look: SignAppearance,
    /// Off where the screen cannot show translucent pixels (X11 without a compositor).
    pub shadow: bool,
    shaft: Option<Path>,
    shaft_length: f32,
    head: Option<Path>,
    root: Option<Path>,
    mark: Option<Path>,
}

impl Scene {
    pub fn new(layout: OverlayLayout, sign: SignImage, look: SignAppearance) -> Self {
        let arrow = &layout.arrow;
        let mut shaft = PathBuilder::new();
        let shaft_length = match &arrow.shaft {
            ShaftLine::Curve(curve) => {
                shaft.move_to(curve.start.x as f32, curve.start.y as f32);
                shaft.quad_to(
                    curve.control.x as f32,
                    curve.control.y as f32,
                    curve.end.x as f32,
                    curve.end.y as f32,
                );
                (0..64)
                    .map(|step| {
                        curve
                            .point(f64::from(step) / 64.0)
                            .distance(curve.point(f64::from(step + 1) / 64.0))
                    })
                    .sum()
            }
            ShaftLine::Polyline(points) => {
                polyline(&mut shaft, points);
                points
                    .windows(2)
                    .map(|pair| pair[0].distance(pair[1]))
                    .sum::<f64>()
            }
        };
        let mut head = PathBuilder::new();
        polyline(&mut head, &arrow.head_points);
        head.close();
        let mark = match layout.mark {
            TargetMark::None => None,
            TargetMark::Box(rect) => rounded_rect(rect, MARK_CORNER),
            TargetMark::Ring { center, radius } => {
                PathBuilder::from_circle(center.x as f32, center.y as f32, radius as f32)
            }
        };
        Self {
            shaft: shaft.finish(),
            shaft_length: shaft_length as f32,
            head: head.finish(),
            root: root_path(&layout),
            mark,
            layout,
            sign,
            look,
            shadow: true,
        }
    }

    /// Everything the arrow can cover in any frame, shadow included, display-local points.
    pub fn bounds(&self) -> Rect {
        let arrow = &self.layout.arrow;
        let mut rect = self
            .layout
            .sign_rect
            .scaled(SIGN_OVERSHOOT, self.layout.sign_rect.center())
            .union(&arrow.shaft_bounds())
            .union(&arrow.head_bounds().scaled(HEAD_OVERSHOOT, arrow.tip));
        if let Some(area) = self.layout.mark.area() {
            rect = rect.union(&area.scaled(PULSE_SCALE, area.center()));
        }
        let reach = self.layout.size.metrics().stroke / 2.0
            + (OUTLINE_WIDTH + EDGE_WIDTH) / 2.0
            + MARK_LINE / 2.0
            + SHADOW_REACH
            + 2.0;
        let grown = rect.inset(-reach, -reach);
        grown
            .intersection(&self.layout.display.local_bounds())
            .unwrap_or(grown)
    }

    /// What the pulse redraws: head and mark at their largest, with their borders and shadow.
    pub fn pulse_bounds(&self) -> Rect {
        let arrow = &self.layout.arrow;
        let mut rect = arrow.head_bounds().scaled(PULSE_SCALE, arrow.tip);
        if let Some(area) = self.layout.mark.area() {
            rect = rect.union(&area.scaled(PULSE_SCALE, area.center()));
        }
        let reach = (OUTLINE_WIDTH + EDGE_WIDTH) / 2.0 + MARK_LINE / 2.0 + SHADOW_REACH + 2.0;
        rect.inset(-reach, -reach)
    }

    /// Draws `frame` into a cleared `pixmap` whose top-left pixel sits at `origin` (display-local
    /// points), `scale` pixels per point.
    pub fn draw(
        &self,
        pixmap: &mut Pixmap,
        origin: Point,
        scale: f64,
        frame: &FrameState,
        copied: Option<usize>,
    ) {
        let base = Transform::from_row(
            scale as f32,
            0.0,
            0.0,
            scale as f32,
            (-origin.x * scale) as f32,
            (-origin.y * scale) as f32,
        );
        if self.look.border != ArrowBorder::Shadow || !self.shadow {
            self.paint(pixmap, base, frame, copied);
            return;
        }
        let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
            return;
        };
        self.paint(&mut layer, base, frame, copied);
        let (width, height) = (layer.width() as usize, layer.height() as usize);
        let mut alpha: Vec<u8> = layer.pixels().iter().map(|pixel| pixel.alpha()).collect();
        blur_alpha(&mut alpha, width, height, SHADOW_SIGMA * scale);
        let drop = (SHADOW_DROP * scale).round() as usize;
        let data = pixmap.data_mut();
        for y in drop..height {
            for x in 0..width {
                let shadow =
                    (f64::from(alpha[(y - drop) * width + x]) * SHADOW_ALPHA).round() as u8;
                data[(y * width + x) * 4 + 3] = shadow;
            }
        }
        pixmap.draw_pixmap(
            0,
            0,
            layer.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    fn paint(
        &self,
        pixmap: &mut Pixmap,
        base: Transform,
        frame: &FrameState,
        copied: Option<usize>,
    ) {
        let look = &self.look;
        let layout = &self.layout;
        let borders = borders(look);
        let (edges, inner) = borders.split_at(borders.len() - 1);
        let head = scaled_about(base, layout.arrow.tip, frame.head);
        let sign_center = layout.sign_rect.center();
        let sign = scaled_about(base, sign_center, frame.sign)
            .pre_translate(layout.sign_rect.x as f32, layout.sign_rect.y as f32);
        if let (Some(mark), Some(area)) = (&self.mark, layout.mark.area()) {
            let at = scaled_about(base, area.center(), frame.mark);
            for (color, width) in &borders {
                pixmap.stroke_path(
                    mark,
                    &paint(*color, frame.mark_alpha),
                    &round(MARK_LINE + width),
                    at,
                    None,
                );
            }
            pixmap.stroke_path(
                mark,
                &paint(look.color, frame.mark_alpha),
                &round(MARK_LINE),
                at,
                None,
            );
        }
        for (color, width) in edges {
            self.arrow_part(pixmap, base, head, *color, *width, frame.shaft);
        }
        self.sign.draw_edge(pixmap, sign, look, frame.sign_alpha);
        for (color, width) in inner {
            self.arrow_part(pixmap, base, head, *color, *width, frame.shaft);
        }
        self.sign.draw_body(pixmap, sign, look, frame.sign_alpha);
        let stroke = layout.size.metrics().stroke;
        if let (Some(line), Some(shaft)) = (self.shaft_stroke(stroke, frame.shaft), &self.shaft) {
            pixmap.stroke_path(shaft, &paint(look.color, 1.0), &line, base, None);
        }
        if let Some(root) = &self.root {
            pixmap.fill_path(root, &paint(look.color, 1.0), FillRule::Winding, base, None);
        }
        if let Some(path) = &self.head {
            pixmap.fill_path(path, &paint(look.color, 1.0), FillRule::Winding, head, None);
        }
        self.sign
            .draw_text(pixmap, sign, look, frame.sign_alpha, copied);
    }

    /// One border of shaft, root and head: the shaft stroked wider, root and head filled and outlined.
    fn arrow_part(
        &self,
        pixmap: &mut Pixmap,
        base: Transform,
        head: Transform,
        color: ArrowColor,
        width: f64,
        progress: f64,
    ) {
        let stroke = self.layout.size.metrics().stroke;
        if let (Some(shaft), Some(line)) =
            (&self.shaft, self.shaft_stroke(stroke + width, progress))
        {
            pixmap.stroke_path(shaft, &paint(color, 1.0), &line, base, None);
        }
        for (path, at) in [(&self.root, base), (&self.head, head)] {
            if let Some(path) = path {
                pixmap.fill_path(path, &paint(color, 1.0), FillRule::Winding, at, None);
                pixmap.stroke_path(path, &paint(color, 1.0), &round(width), at, None);
            }
        }
    }

    /// The shaft's stroke, dashed to the drawn share while it draws on; `None` before it starts.
    fn shaft_stroke(&self, width: f64, progress: f64) -> Option<Stroke> {
        if progress <= 0.001 {
            return None;
        }
        let mut stroke = round(width);
        if progress < 1.0 {
            stroke.dash = StrokeDash::new(
                vec![self.shaft_length * progress as f32, self.shaft_length],
                0.0,
            );
        }
        Some(stroke)
    }
}

/// The border layers of one part, outermost first.
fn borders(look: &SignAppearance) -> Vec<(ArrowColor, f64)> {
    match look.border {
        ArrowBorder::Shadow => vec![(look.border_tint(), OUTLINE_WIDTH)],
        ArrowBorder::WhiteBlack => vec![
            (look.edge_tint(), OUTLINE_WIDTH + EDGE_WIDTH),
            (look.border_tint(), OUTLINE_WIDTH),
        ],
        ArrowBorder::Black => vec![(look.edge_tint(), EDGE_WIDTH)],
    }
}

fn round(width: f64) -> Stroke {
    Stroke {
        width: width as f32,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

fn scaled_about(base: Transform, point: Point, factor: f64) -> Transform {
    let (x, y, factor) = (point.x as f32, point.y as f32, factor as f32);
    base.pre_translate(x, y)
        .pre_scale(factor, factor)
        .pre_translate(-x, -y)
}

fn polyline(builder: &mut PathBuilder, points: &[Point]) {
    for (index, point) in points.iter().enumerate() {
        if index == 0 {
            builder.move_to(point.x as f32, point.y as f32);
        } else {
            builder.line_to(point.x as f32, point.y as f32);
        }
    }
}

/// The flared root: flanks from the sign edge into the shaft's sides, reaching into the sign.
fn root_path(layout: &OverlayLayout) -> Option<Path> {
    let root = &layout.arrow.root;
    let normal = root.junction.normal;
    let [first, second] = root.flanks();
    let depth = normal * crate::model::arrow::ArrowRoot::DEPTH;
    let point = |builder: &mut PathBuilder, p: Point, line: bool| {
        if line {
            builder.line_to(p.x as f32, p.y as f32)
        } else {
            builder.move_to(p.x as f32, p.y as f32)
        }
    };
    let curve = |builder: &mut PathBuilder, a: Point, b: Point, end: Point| {
        builder.cubic_to(
            a.x as f32,
            a.y as f32,
            b.x as f32,
            b.y as f32,
            end.x as f32,
            end.y as f32,
        );
    };
    let mut path = PathBuilder::new();
    point(&mut path, first.on_edge - depth, false);
    point(&mut path, first.on_edge, true);
    curve(&mut path, first.near_edge, first.near_shaft, first.on_shaft);
    point(&mut path, second.on_shaft, true);
    curve(
        &mut path,
        second.near_shaft,
        second.near_edge,
        second.on_edge,
    );
    point(&mut path, second.on_edge - depth, true);
    path.close();
    path.finish()
}
