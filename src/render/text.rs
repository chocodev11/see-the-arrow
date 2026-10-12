//! Text layout with parley, glyph outlines with skrifa, drawn as tiny-skia paths. Only the
//! bundled fonts are loaded, so laying out a sign never scans the system's fonts; those load
//! once, and only when a character is missing from the bundled ones.

use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::style::{FontFamily, FontWeight, LineHeight, StyleProperty};
use parley::{
    Alignment, AlignmentOptions, FontContext, InlineBox, InlineBoxKind, Layout, LayoutContext,
    PositionedLayoutItem, VerticalAlign,
};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use tiny_skia::{Path, PathBuilder};

use crate::model::geom::Rect;

static SIGN_FONT: &[u8] = include_bytes!("../../assets/fonts/Nunito.ttf");
static CHIP_FONT: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");
const SIGN_FAMILY: &str = "Nunito";
const CHIP_FAMILY: &str = "JetBrains Mono";
/// Nunito's heaviest weight, the closest to SF Pro Rounded Heavy.
const SIGN_WEIGHT: f32 = 900.0;
const CHIP_WEIGHT: f32 = 700.0;

/// A run of text laid out in points: its glyphs as one path (y down, origin at the block's
/// top-left) and its size.
pub struct TextBlock {
    pub glyphs: Option<Path>,
    pub width: f64,
    pub height: f64,
    pub lines: usize,
    /// Every inline box, block-local, in the order they were pushed.
    pub boxes: Vec<Rect>,
}

/// An inline box in a paragraph: placed at byte `index`, `baseline` measured from its top.
pub struct Inline {
    pub index: usize,
    pub width: f64,
    pub height: f64,
    pub baseline: f64,
}

pub struct TextEngine {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    system_fonts: bool,
}

impl TextEngine {
    pub fn new() -> Self {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        for data in [SIGN_FONT, CHIP_FONT] {
            collection.register_fonts(Blob::new(Arc::new(data)), None);
        }
        Self {
            fonts: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            layouts: LayoutContext::new(),
            system_fonts: false,
        }
    }

    #[cfg(test)]
    pub fn loaded_system_fonts(&self) -> bool {
        self.system_fonts
    }

    /// Loads the system's fonts when the bundled ones miss a character of `text`.
    fn cover(&mut self, text: &str) {
        if self.system_fonts {
            return;
        }
        let covered = |data: &[u8], c: char| {
            FontRef::new(data).is_ok_and(|font| font.charmap().map(c).is_some())
        };
        if text
            .chars()
            .all(|c| c.is_control() || covered(SIGN_FONT, c) || covered(CHIP_FONT, c))
        {
            return;
        }
        self.fonts.collection.load_system_fonts();
        self.system_fonts = true;
    }

    /// Lays out the sign's sentence centred in at most `max_width`, with inline boxes.
    pub fn paragraph(
        &mut self,
        text: &str,
        font_size: f64,
        max_width: f64,
        boxes: &[Inline],
    ) -> TextBlock {
        self.cover(text);
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(SIGN_FAMILY)));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(SIGN_WEIGHT)));
        builder.push_default(StyleProperty::FontSize(font_size as f32));
        builder.push_default(StyleProperty::LineHeight(LineHeight::MetricsRelative(1.0)));
        for (id, inline) in boxes.iter().enumerate() {
            builder.push_inline_box(InlineBox {
                id: id as u64,
                kind: InlineBoxKind::InFlow,
                index: inline.index,
                width: inline.width as f32,
                height: inline.height as f32,
                baseline: Some(inline.baseline as f32),
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(max_width as f32));
        layout.align(Alignment::Center, AlignmentOptions::default());
        block(&layout)
    }

    /// One line in the chip's monospaced font.
    pub fn chip_line(&mut self, text: &str, font_size: f64) -> TextBlock {
        self.cover(text);
        let mut builder = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(CHIP_FAMILY)));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(CHIP_WEIGHT)));
        builder.push_default(StyleProperty::FontSize(font_size as f32));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(None);
        block(&layout)
    }
}

/// Cap height of the sign font at `font_size`, for centring chips on it like a text attachment.
pub fn sign_cap_height(font_size: f64) -> f64 {
    let font = FontRef::new(SIGN_FONT).expect("the bundled sign font parses");
    let metrics = font.metrics(Size::new(font_size as f32), LocationRef::default());
    f64::from(metrics.cap_height.unwrap_or(metrics.ascent * 0.7))
}

/// Ascender minus descender of the chip font at `font_size`.
pub fn chip_line_height(font_size: f64) -> f64 {
    let font = FontRef::new(CHIP_FONT).expect("the bundled chip font parses");
    let metrics = font.metrics(Size::new(font_size as f32), LocationRef::default());
    f64::from(metrics.ascent - metrics.descent)
}

/// Lines are centred in the full `max_width`; like TextKit's used rect, the block starts at the
/// leftmost line and ends at the rightmost ink, trailing spaces excluded.
fn block(layout: &Layout<()>) -> TextBlock {
    let (left, right) = layout
        .lines()
        .fold((f32::MAX, f32::MIN), |(left, right), line| {
            let metrics = line.metrics();
            let start = metrics.offset + metrics.inline_min_coord;
            (
                left.min(start),
                right.max(start + metrics.advance - metrics.hanging_advance),
            )
        });
    let (left, width) = if left <= right {
        (left, right - left)
    } else {
        (0.0, 0.0)
    };
    let mut builder = PathBuilder::new();
    let mut boxes = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            match item {
                PositionedLayoutItem::GlyphRun(glyph_run) => {
                    let run = glyph_run.run();
                    let font = run.font();
                    let Ok(font_ref) = FontRef::from_index(font.data.as_ref(), font.index) else {
                        continue;
                    };
                    let outlines = font_ref.outline_glyphs();
                    let coords: Vec<NormalizedCoord> = run
                        .normalized_coords()
                        .iter()
                        .map(|coord| NormalizedCoord::from_bits(coord.to_bits()))
                        .collect();
                    let size = Size::new(run.font_size());
                    for glyph in glyph_run.positioned_glyphs() {
                        let Some(outline) = outlines.get(GlyphId::new(glyph.id)) else {
                            continue;
                        };
                        let settings = DrawSettings::unhinted(size, LocationRef::new(&coords));
                        let mut pen = Pen {
                            builder: &mut builder,
                            x: glyph.x - left,
                            y: glyph.y,
                        };
                        let _ = outline.draw(settings, &mut pen);
                    }
                }
                PositionedLayoutItem::InlineBox(inline) => {
                    let x = f64::from(inline.x - left);
                    boxes.push((
                        inline.id,
                        Rect::new(
                            x,
                            inline.y.into(),
                            inline.width.into(),
                            inline.height.into(),
                        ),
                    ));
                }
            }
        }
    }
    boxes.sort_by_key(|(id, _)| *id);
    TextBlock {
        glyphs: builder.finish(),
        width: f64::from(width),
        height: f64::from(layout.height()),
        lines: layout.len().max(1),
        boxes: boxes.into_iter().map(|(_, rect)| rect).collect(),
    }
}

/// Font units are y up; the block is y down with the baseline at `y`.
struct Pen<'a> {
    builder: &'a mut PathBuilder,
    x: f32,
    y: f32,
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(self.x + x, self.y - y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(self.x + x, self.y - y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.builder
            .quad_to(self.x + cx0, self.y - cy0, self.x + x, self.y - y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.builder.cubic_to(
            self.x + cx0,
            self.y - cy0,
            self.x + cx1,
            self.y - cy1,
            self.x + x,
            self.y - y,
        );
    }

    fn close(&mut self) {
        self.builder.close();
    }
}
