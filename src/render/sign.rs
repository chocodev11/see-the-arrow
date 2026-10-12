//! The sign: a pill (or square) in the arrow colour with its border, heavy rounded text with a
//! copy chip for every `{{value}}`, and with `--close-button` an X in its right end.

use tiny_skia::{FillRule, LineCap, LineJoin, Path, PathBuilder, Pixmap, Stroke, Transform};

use super::text::{self, Inline, TextEngine};
use super::{paint, rounded_rect};
use crate::model::appearance::{ArrowBorder, ArrowMetrics, SignAppearance, SignCorners};
use crate::model::geom::{Point, Rect};
use crate::model::sign_text::{Segment, SignText};

const PADDING_X: f64 = 30.0;
const PADDING_Y: f64 = 16.0;
/// The contrast border inside the sign.
const OUTLINE: f64 = 3.5;
/// The thin black line of `--border white-black` and `black`.
const EDGE: f64 = 1.5;
const SEAM_OVERLAP: f64 = 0.5;
/// The X: diameter as a share of the font size, at least the minimum; gap after the text and
/// margin to the sign's edge.
const CROSS_SHARE: f64 = 0.8;
pub const CROSS_MINIMUM: f64 = 26.0;
const CROSS_GAP: f64 = 12.0;
const CROSS_MARGIN: f64 = 20.0;

/// One `{{value}}` of the sign: the value in a bold monospaced font and a copy icon on a rounded
/// chip, filled with the text colour. A value wider than the sign is shortened in the middle.
pub struct Chip {
    /// Sign-local top-left points.
    pub rect: Rect,
    pub value: String,
    glyphs: Option<Path>,
    /// Where the value's line box starts, chip-local.
    value_origin: Point,
    icon_origin: Point,
    icon_size: f64,
    corner: f64,
}

pub struct SignImage {
    pub width: f64,
    pub height: f64,
    text: Option<Path>,
    text_origin: Point,
    pub chips: Vec<Chip>,
    cross: Option<(Point, f64)>,
}

/// Chip proportions, shares of the sign's font size.
struct ChipLook {
    value_size: f64,
    height: f64,
    padding: f64,
    icon: f64,
    gap: f64,
}

impl ChipLook {
    fn new(font_size: f64) -> Self {
        Self {
            value_size: (font_size * 0.78).round(),
            height: (font_size * 1.12).ceil(),
            padding: (font_size * 0.36).round(),
            icon: (font_size * 0.74).round(),
            gap: (font_size * 0.28).round(),
        }
    }
}

impl SignImage {
    /// Lays out the text in at most 60 % of the display width (`display_width` in points),
    /// shrinking the font until it fits in three lines, never below the minimum.
    pub fn new(
        engine: &mut TextEngine,
        text: &SignText,
        look: &SignAppearance,
        display_width: f64,
    ) -> Self {
        let max_width = display_width * ArrowMetrics::MAXIMUM_SIGN_WIDTH_SHARE - PADDING_X * 2.0;
        let mut font_size = look.size.metrics().font_size;
        let (mut block, mut chips) = layout(engine, text, look, font_size, max_width);
        while block.lines > ArrowMetrics::MAXIMUM_LINES
            && font_size > ArrowMetrics::MINIMUM_FONT_SIZE
        {
            font_size = (font_size - 2.0).max(ArrowMetrics::MINIMUM_FONT_SIZE);
            (block, chips) = layout(engine, text, look, font_size, max_width);
        }
        let cross = if look.close_button {
            CROSS_MINIMUM.max((font_size * CROSS_SHARE).round())
        } else {
            0.0
        };
        let height = (block.height + PADDING_Y * 2.0).ceil();
        let cross_inset = (height / 2.0).min(cross / 2.0 + CROSS_MARGIN);
        let trailing = if cross > 0.0 {
            CROSS_GAP + cross / 2.0 + cross_inset
        } else {
            PADDING_X
        };
        let width = (PADDING_X + block.width + trailing).ceil();
        let text_origin = Point::new(PADDING_X, (height - block.height) / 2.0);
        for (chip, rect) in chips.iter_mut().zip(&block.boxes) {
            chip.rect = rect.offset(text_origin.x, text_origin.y);
        }
        Self {
            width,
            height,
            text: block.glyphs,
            text_origin,
            chips,
            cross: (cross > 0.0).then(|| (Point::new(width - cross_inset, height / 2.0), cross)),
        }
    }

    /// The thin black line of `--border white-black` and `black`; nothing for the default.
    pub fn draw_edge(
        &self,
        pixmap: &mut Pixmap,
        transform: Transform,
        look: &SignAppearance,
        alpha: f64,
    ) {
        if look.border == ArrowBorder::Shadow {
            return;
        }
        let rect = Rect::new(0.0, 0.0, self.width, self.height).inset(EDGE / 2.0, EDGE / 2.0);
        if let Some(path) = rounded_rect(rect, look.corners.radius(rect.height)) {
            pixmap.stroke_path(
                &path,
                &paint(look.edge_tint(), alpha),
                &stroke(EDGE),
                transform,
                None,
            );
        }
    }

    /// Fill and contrast border, inside the edge, overlapping it by half a point so no seam shows.
    pub fn draw_body(
        &self,
        pixmap: &mut Pixmap,
        transform: Transform,
        look: &SignAppearance,
        alpha: f64,
    ) {
        let edge = if look.border == ArrowBorder::Shadow {
            0.0
        } else {
            EDGE
        };
        let inset = (edge - SEAM_OVERLAP).max(0.0);
        let body = Rect::new(0.0, 0.0, self.width, self.height).inset(inset, inset);
        if look.border == ArrowBorder::Black {
            if let Some(path) = rounded_rect(body, look.corners.radius(body.height)) {
                pixmap.fill_path(
                    &path,
                    &paint(look.color, alpha),
                    FillRule::Winding,
                    transform,
                    None,
                );
            }
            return;
        }
        let line = body.inset(OUTLINE / 2.0, OUTLINE / 2.0);
        if let Some(path) = rounded_rect(line, look.corners.radius(line.height)) {
            pixmap.fill_path(
                &path,
                &paint(look.color, alpha),
                FillRule::Winding,
                transform,
                None,
            );
            pixmap.stroke_path(
                &path,
                &paint(look.border_tint(), alpha),
                &stroke(OUTLINE),
                transform,
                None,
            );
        }
    }

    /// Text, chips (`copied` shows a check instead of the copy icon) and the X.
    pub fn draw_text(
        &self,
        pixmap: &mut Pixmap,
        transform: Transform,
        look: &SignAppearance,
        alpha: f64,
        copied: Option<usize>,
    ) {
        if let Some(glyphs) = &self.text {
            let at = transform.pre_translate(self.text_origin.x as f32, self.text_origin.y as f32);
            pixmap.fill_path(
                glyphs,
                &paint(look.text_tint(), alpha),
                FillRule::Winding,
                at,
                None,
            );
        }
        for (index, chip) in self.chips.iter().enumerate() {
            chip.draw(pixmap, transform, look, alpha, copied == Some(index));
        }
        if let Some((center, diameter)) = self.cross {
            draw_cross(pixmap, transform, look, alpha, center, diameter);
        }
    }
}

/// Lays out the sentence once at `font_size`, chips as inline boxes.
fn layout(
    engine: &mut TextEngine,
    text: &SignText,
    look: &SignAppearance,
    font_size: f64,
    max_width: f64,
) -> (text::TextBlock, Vec<Chip>) {
    let chip_look = ChipLook::new(font_size);
    let cap = text::sign_cap_height(font_size);
    let mut sentence = String::new();
    let mut inlines = Vec::new();
    let mut chips = Vec::new();
    for segment in &text.segments {
        match segment {
            Segment::Plain(plain) => sentence.push_str(plain),
            Segment::Copy(value) => {
                let chip = Chip::new(engine, value, &chip_look, look.corners, max_width);
                // Centred on the cap height, like a text attachment: the box's baseline from its top.
                let baseline = chip.rect.height + ((cap - chip.rect.height) / 2.0).round();
                inlines.push(Inline {
                    index: sentence.len(),
                    width: chip.rect.width,
                    height: chip.rect.height,
                    baseline,
                });
                chips.push(chip);
            }
        }
    }
    (
        engine.paragraph(&sentence, font_size, max_width, &inlines),
        chips,
    )
}

impl Chip {
    fn new(
        engine: &mut TextEngine,
        value: &str,
        look: &ChipLook,
        corners: SignCorners,
        max_width: f64,
    ) -> Self {
        let room = max_width - look.padding * 2.0 - look.gap - look.icon;
        let block = fit_value(engine, value, look.value_size, room);
        let value_width = block.width.ceil().min(room).max(0.0);
        let line_height = text::chip_line_height(look.value_size).ceil();
        let width = look.padding * 2.0 + value_width + look.gap + look.icon;
        Self {
            rect: Rect::new(0.0, 0.0, width, look.height),
            value: value.to_owned(),
            glyphs: block.glyphs,
            value_origin: Point::new(look.padding, ((look.height - line_height) / 2.0).round()),
            icon_origin: Point::new(
                look.padding + value_width + look.gap,
                (look.height - look.icon) / 2.0,
            ),
            icon_size: look.icon,
            corner: if corners == SignCorners::Round {
                look.height / 2.0
            } else {
                look.height * 0.18
            },
        }
    }

    fn draw(
        &self,
        pixmap: &mut Pixmap,
        transform: Transform,
        look: &SignAppearance,
        alpha: f64,
        copied: bool,
    ) {
        let at = transform.pre_translate(self.rect.x as f32, self.rect.y as f32);
        let shape = Rect::new(0.0, 0.0, self.rect.width, self.rect.height);
        if let Some(path) = rounded_rect(shape, self.corner) {
            pixmap.fill_path(
                &path,
                &paint(look.text_tint(), alpha),
                FillRule::Winding,
                at,
                None,
            );
        }
        if let Some(glyphs) = &self.glyphs {
            let origin = at.pre_translate(self.value_origin.x as f32, self.value_origin.y as f32);
            pixmap.fill_path(
                glyphs,
                &paint(look.color, alpha),
                FillRule::Winding,
                origin,
                None,
            );
        }
        let unit = (self.icon_size / 24.0) as f32;
        let icon = at
            .pre_translate(self.icon_origin.x as f32, self.icon_origin.y as f32)
            .pre_scale(unit, unit);
        let path = if copied { check_icon() } else { copy_icon() };
        if let Some(path) = path {
            let mut line = stroke(2.0 * 1.15);
            line.line_join = LineJoin::Round;
            pixmap.stroke_path(&path, &paint(look.color, alpha), &line, icon, None);
        }
    }
}

/// The value as shown: whole if it fits in `room`, else shortened in the middle with an ellipsis.
fn fit_value(engine: &mut TextEngine, value: &str, size: f64, room: f64) -> text::TextBlock {
    let block = engine.chip_line(value, size);
    if block.width <= room {
        return block;
    }
    let chars: Vec<char> = value.chars().collect();
    let advance = block.width / chars.len().max(1) as f64;
    let mut keep = ((room / advance).floor() as usize)
        .saturating_sub(1)
        .min(chars.len());
    loop {
        let head: String = chars[..keep.div_ceil(2)].iter().collect();
        let tail: String = chars[chars.len() - keep / 2..].iter().collect();
        let block = engine.chip_line(&format!("{head}…{tail}"), size);
        if block.width <= room || keep == 0 {
            return block;
        }
        keep -= 1;
    }
}

fn stroke(width: f64) -> Stroke {
    Stroke {
        width: width as f32,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

/// The X button: a filled circle with the X on it.
fn draw_cross(
    pixmap: &mut Pixmap,
    transform: Transform,
    look: &SignAppearance,
    alpha: f64,
    center: Point,
    diameter: f64,
) {
    let (cx, cy) = (center.x as f32, center.y as f32);
    if let Some(circle) = PathBuilder::from_circle(cx, cy, (diameter / 2.0) as f32) {
        pixmap.fill_path(
            &circle,
            &paint(look.close_tint(), alpha),
            FillRule::Winding,
            transform,
            None,
        );
    }
    let arm = (diameter * 0.2) as f32;
    let mut cross = PathBuilder::new();
    cross.move_to(cx - arm, cy - arm);
    cross.line_to(cx + arm, cy + arm);
    cross.move_to(cx - arm, cy + arm);
    cross.line_to(cx + arm, cy - arm);
    if let Some(path) = cross.finish() {
        pixmap.stroke_path(
            &path,
            &paint(look.close_x_tint(), alpha),
            &stroke((diameter * 0.11).max(2.5)),
            transform,
            None,
        );
    }
}

/// Feather's `copy` (MIT, feathericons.com), 24-unit box, y down.
fn copy_icon() -> Option<Path> {
    let k = 0.552_284_8 * 2.0;
    let mut path = PathBuilder::new();
    // <rect x="9" y="9" width="13" height="13" rx="2" ry="2">
    path.move_to(11.0, 9.0);
    path.line_to(20.0, 9.0);
    path.cubic_to(20.0 + k, 9.0, 22.0, 11.0 - k, 22.0, 11.0);
    path.line_to(22.0, 20.0);
    path.cubic_to(22.0, 20.0 + k, 20.0 + k, 22.0, 20.0, 22.0);
    path.line_to(11.0, 22.0);
    path.cubic_to(11.0 - k, 22.0, 9.0, 20.0 + k, 9.0, 20.0);
    path.line_to(9.0, 11.0);
    path.cubic_to(9.0, 11.0 - k, 11.0 - k, 9.0, 11.0, 9.0);
    path.close();
    // <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1">
    path.move_to(5.0, 15.0);
    path.line_to(4.0, 15.0);
    path.cubic_to(4.0 - k, 15.0, 2.0, 13.0 + k, 2.0, 13.0);
    path.line_to(2.0, 4.0);
    path.cubic_to(2.0, 4.0 - k, 4.0 - k, 2.0, 4.0, 2.0);
    path.line_to(13.0, 2.0);
    path.cubic_to(13.0 + k, 2.0, 15.0, 4.0 - k, 15.0, 4.0);
    path.line_to(15.0, 5.0);
    path.finish()
}

/// Feather's `check`: `<polyline points="20 6 9 17 4 12">`.
fn check_icon() -> Option<Path> {
    let mut path = PathBuilder::new();
    path.move_to(20.0, 6.0);
    path.line_to(9.0, 17.0);
    path.line_to(4.0, 12.0);
    path.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::appearance::{ArrowColor, ArrowSize};

    fn look(close_button: bool) -> SignAppearance {
        let mut look = SignAppearance::new(
            ArrowColor::parse("red").unwrap(),
            ArrowSize::Medium,
            SignCorners::Round,
        );
        look.close_button = close_button;
        look
    }

    #[test]
    fn the_x_widens_the_sign_by_at_least_its_diameter() {
        let mut engine = TextEngine::new();
        let text = SignText::parse("Franz, click Allow").unwrap();
        let plain = SignImage::new(&mut engine, &text, &look(false), 1512.0);
        let crossed = SignImage::new(&mut engine, &text, &look(true), 1512.0);
        assert_eq!(plain.height, crossed.height);
        assert!(crossed.width - plain.width >= CROSS_MINIMUM);
    }

    #[test]
    fn chips_lie_inside_the_sign_in_reading_order() {
        let mut engine = TextEngine::new();
        let text = SignText::parse("User {{franz}} and password {{correct horse battery staple}}")
            .unwrap();
        let sign = SignImage::new(&mut engine, &text, &look(false), 1512.0);
        let bounds = Rect::new(0.0, 0.0, sign.width, sign.height);
        assert_eq!(sign.chips.len(), 2);
        for chip in &sign.chips {
            assert!(
                bounds.contains_rect(&chip.rect),
                "{:?} leaves {bounds:?}",
                chip.rect
            );
        }
        let (first, second) = (sign.chips[0].rect, sign.chips[1].rect);
        assert!(!first.intersects(&second));
        assert!(first.max_y() <= second.min_y() + 1.0 || first.min_x() < second.min_x());
    }

    #[test]
    fn long_values_are_shortened_but_copied_whole() {
        let mut engine = TextEngine::new();
        let value =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl "
                .repeat(4);
        let value = value.trim();
        let text = SignText::parse(&format!("Paste {{{{{value}}}}}")).unwrap();
        let sign = SignImage::new(&mut engine, &text, &look(false), 1512.0);
        assert_eq!(sign.chips[0].value, value);
        assert!(sign.width <= 1512.0 * 0.6 + 1.0, "{}", sign.width);
        assert!(Rect::new(0.0, 0.0, sign.width, sign.height).contains_rect(&sign.chips[0].rect));
    }

    #[test]
    fn vietnamese_text_is_laid_out_with_the_bundled_font() {
        let mut engine = TextEngine::new();
        let text = SignText::parse("Bấm Cho phép {{mật khẩu}}").unwrap();
        let sign = SignImage::new(&mut engine, &text, &look(false), 1512.0);
        let one_line = SignImage::new(
            &mut engine,
            &SignText::parse("Franz").unwrap(),
            &look(false),
            1512.0,
        );
        assert!(sign.text.is_some());
        assert_eq!(sign.height, one_line.height);
        assert!(!engine.loaded_system_fonts());
    }
}
