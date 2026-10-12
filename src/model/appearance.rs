use serde::{Deserialize, Serialize};

use super::error::{Error, Result};
use super::geom::Point;

/// S, M, L presets. Everything that scales with size lives here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowSize {
    Small,
    Medium,
    Large,
}

/// Lengths in layout points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowMetrics {
    pub stroke: f64,
    pub head_length: f64,
    pub head_width: f64,
    pub font_size: f64,
    /// Preferred distance between the target and the near edge of the sign.
    pub reach: f64,
}

impl ArrowMetrics {
    /// Smallest font the sign shrinks to before giving up on the line limit.
    pub const MINIMUM_FONT_SIZE: f64 = 24.0;
    /// The sign wraps to more lines than this only at the minimum font size.
    pub const MAXIMUM_LINES: usize = 3;
    /// Share of the display width the sign may use.
    pub const MAXIMUM_SIGN_WIDTH_SHARE: f64 = 0.6;
}

impl ArrowSize {
    pub fn metrics(self) -> ArrowMetrics {
        match self {
            Self::Small => ArrowMetrics {
                stroke: 10.0,
                head_length: 46.0,
                head_width: 48.0,
                font_size: 30.0,
                reach: 130.0,
            },
            Self::Medium => ArrowMetrics {
                stroke: 15.0,
                head_length: 62.0,
                head_width: 64.0,
                font_size: 44.0,
                reach: 170.0,
            },
            Self::Large => ArrowMetrics {
                stroke: 21.0,
                head_length: 84.0,
                head_width: 88.0,
                font_size: 60.0,
                reach: 220.0,
            },
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw.to_uppercase().as_str() {
            "S" => Ok(Self::Small),
            "M" => Ok(Self::Medium),
            "L" => Ok(Self::Large),
            _ => Err(Error::bad_input(format!(
                "size '{raw}' is not one of S, M, L"
            ))),
        }
    }
}

/// How the target is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowStyle {
    Arrow,
    Ring,
    Box,
}

impl ArrowStyle {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.to_lowercase().as_str() {
            "arrow" => Ok(Self::Arrow),
            "ring" => Ok(Self::Ring),
            "box" => Ok(Self::Box),
            _ => Err(Error::bad_input(format!(
                "style '{raw}' is not one of arrow, ring, box"
            ))),
        }
    }
}

/// The shape of the shaft between sign and head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowShape {
    Bend,
    Straight,
    Zigzag,
    /// One loop around the sign, then to the target.
    Spiral,
}

impl ArrowShape {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.to_lowercase().as_str() {
            "bend" => Ok(Self::Bend),
            "straight" => Ok(Self::Straight),
            "zigzag" => Ok(Self::Zigzag),
            "spiral" => Ok(Self::Spiral),
            _ => Err(Error::bad_input(format!(
                "shape '{raw}' is not one of bend, straight, zigzag, spiral"
            ))),
        }
    }
}

/// The sign's corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignCorners {
    Round,
    Sharp,
}

impl SignCorners {
    /// Corner radius for a sign of `height`: a pill up to 30 pt, or square.
    pub fn radius(self, height: f64) -> f64 {
        match self {
            Self::Round => (height / 2.0).min(30.0),
            Self::Sharp => 0.0,
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw.to_lowercase().as_str() {
            "round" => Ok(Self::Round),
            "sharp" => Ok(Self::Sharp),
            _ => Err(Error::bad_input(format!(
                "corners '{raw}' is not one of round, sharp"
            ))),
        }
    }
}

/// `--border`: how arrow and sign stand off the screen behind them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrowBorder {
    /// The contrast border and a drop shadow (default).
    Shadow,
    /// The contrast border with a thin black edge outside it, no shadow.
    WhiteBlack,
    /// Only a thin black outline, no shadow.
    Black,
}

impl ArrowBorder {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.to_lowercase().as_str() {
            "shadow" => Ok(Self::Shadow),
            "white-black" => Ok(Self::WhiteBlack),
            "black" => Ok(Self::Black),
            _ => Err(Error::bad_input(format!(
                "border '{raw}' is not one of shadow, white-black, black"
            ))),
        }
    }
}

/// An sRGB colour, parsed from a preset name or a hex string. Channels 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArrowColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
}

pub const PRESETS: [(&str, &str); 10] = [
    ("red", "FF3B1F"),
    ("orange", "FF8A00"),
    ("yellow", "FFD60A"),
    ("green", "21B24B"),
    ("teal", "00B5AD"),
    ("blue", "0A84FF"),
    ("purple", "8E44FF"),
    ("pink", "FF2D78"),
    ("black", "1C1C1E"),
    ("white", "FFFFFF"),
];

impl ArrowColor {
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    pub const fn rgb(red: f64, green: f64, blue: f64) -> Self {
        Self { red, green, blue }
    }

    pub fn accepted() -> String {
        let names: Vec<&str> = PRESETS.iter().map(|(name, _)| *name).collect();
        format!(
            "a preset ({}) or hex like #FF3B1F or #F31",
            names.join(", ")
        )
    }

    pub fn parse(raw: &str) -> Result<Self> {
        let key = raw.to_lowercase();
        let key = key.trim_matches([' ', '\t']);
        let hex = PRESETS
            .iter()
            .find(|(name, _)| *name == key)
            .map_or(key, |(_, hex)| hex);
        Self::hex(hex).ok_or_else(|| {
            Error::bad_input(format!(
                "colour '{raw}' is not valid, use {}",
                Self::accepted()
            ))
        })
    }

    /// `RRGGBB` or the short `RGB`, with or without `#`.
    fn hex(raw: &str) -> Option<Self> {
        let digits = raw.strip_prefix('#').unwrap_or(raw);
        let digits: String = if digits.chars().count() == 3 {
            digits.chars().flat_map(|c| [c, c]).collect()
        } else {
            digits.to_owned()
        };
        if digits.len() != 6 {
            return None;
        }
        let value = u32::from_str_radix(&digits, 16).ok()?;
        let channel = |shift: u32| f64::from((value >> shift) & 0xFF) / 255.0;
        Some(Self::rgb(channel(16), channel(8), channel(0)))
    }

    /// Rec. 709 luma of the sRGB values, good enough to pick a readable contrast colour.
    pub fn luminance(&self) -> f64 {
        0.2126 * self.red + 0.7152 * self.green + 0.0722 * self.blue
    }

    pub fn is_light(&self) -> bool {
        self.luminance() > 0.6
    }

    /// Sign text colour: white on dark arrows, near-black on light ones (yellow, white).
    pub fn contrast(&self) -> Self {
        if self.is_light() {
            Self::rgb(0.11, 0.11, 0.12)
        } else {
            Self::WHITE
        }
    }

    /// The thin outline of `--border black`: black, white on near-black arrows.
    pub fn outline(&self) -> Self {
        if self.luminance() < 0.2 {
            Self::WHITE
        } else {
            Self::BLACK
        }
    }
}

/// Where the sign sits relative to the target. The arrow approaches from that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApproachDirection {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Left,
    Right,
    Top,
    Bottom,
}

impl ApproachDirection {
    /// Placement tries sides in this order; it matters for scores and ties.
    pub const ALL: [Self; 8] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::Left,
        Self::Right,
        Self::Top,
        Self::Bottom,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    /// Unit vector from the target towards the sign, y-down.
    pub fn unit(self) -> Point {
        match self {
            Self::TopLeft => Point::new(-1.0, -1.0).normalized(),
            Self::TopRight => Point::new(1.0, -1.0).normalized(),
            Self::BottomLeft => Point::new(-1.0, 1.0).normalized(),
            Self::BottomRight => Point::new(1.0, 1.0).normalized(),
            Self::Left => Point::new(-1.0, 0.0),
            Self::Right => Point::new(1.0, 0.0),
            Self::Top => Point::new(0.0, -1.0),
            Self::Bottom => Point::new(0.0, 1.0),
        }
    }

    /// `auto` is `None`.
    pub fn parse(raw: &str) -> Result<Option<Self>> {
        let key = raw.to_lowercase();
        if key == "auto" {
            return Ok(None);
        }
        Self::ALL
            .into_iter()
            .find(|direction| direction.name() == key)
            .map(Some)
            .ok_or_else(|| {
                let names: Vec<&str> = std::iter::once("auto")
                    .chain(Self::ALL.iter().map(|d| d.name()))
                    .collect();
                Error::bad_input(format!("--from '{raw}' is not one of {}", names.join(", ")))
            })
    }
}

/// Everything about how arrow and sign look, besides the text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignAppearance {
    pub color: ArrowColor,
    pub size: ArrowSize,
    pub corners: SignCorners,
    pub border: ArrowBorder,
    pub close_button: bool,
    pub border_color: Option<ArrowColor>,
    pub text_color: Option<ArrowColor>,
    pub edge_color: Option<ArrowColor>,
    pub close_color: Option<ArrowColor>,
    pub close_x_color: Option<ArrowColor>,
}

impl SignAppearance {
    pub fn new(color: ArrowColor, size: ArrowSize, corners: SignCorners) -> Self {
        Self {
            color,
            size,
            corners,
            border: ArrowBorder::Shadow,
            close_button: false,
            border_color: None,
            text_color: None,
            edge_color: None,
            close_color: None,
            close_x_color: None,
        }
    }

    /// The border: white on dark arrows, dark on light ones, unless set.
    pub fn border_tint(&self) -> ArrowColor {
        self.border_color.unwrap_or_else(|| self.color.contrast())
    }

    pub fn text_tint(&self) -> ArrowColor {
        self.text_color.unwrap_or_else(|| self.color.contrast())
    }

    /// The thin line: black (white for `--border black` on near-black arrows), unless set.
    pub fn edge_tint(&self) -> ArrowColor {
        self.edge_color.unwrap_or_else(|| {
            if self.border == ArrowBorder::Black {
                self.color.outline()
            } else {
                ArrowColor::BLACK
            }
        })
    }

    /// The X button's circle: the border colour, unless set.
    pub fn close_tint(&self) -> ArrowColor {
        self.close_color.unwrap_or_else(|| self.border_tint())
    }

    /// The X: the arrow colour on the default circle, else whatever reads on the chosen circle.
    pub fn close_x_tint(&self) -> ArrowColor {
        self.close_x_color
            .unwrap_or_else(|| match self.close_color {
                None => self.color,
                Some(_) => self.close_tint().contrast(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_accept_presets_and_hex() {
        assert_eq!(
            ArrowColor::parse("blue").unwrap(),
            ArrowColor::parse("#0A84FF").unwrap()
        );
        assert_eq!(
            ArrowColor::parse("ff0000").unwrap(),
            ArrowColor::rgb(1.0, 0.0, 0.0)
        );
        assert_eq!(
            ArrowColor::parse("#f31").unwrap(),
            ArrowColor::parse("FF3311").unwrap()
        );
        assert!(
            ArrowColor::parse("rainbow")
                .unwrap_err()
                .message
                .contains(&ArrowColor::accepted())
        );
        assert!(ArrowColor::parse("#12345").is_err());
    }

    #[test]
    fn only_yellow_and_white_are_light() {
        for (name, _) in PRESETS {
            let color = ArrowColor::parse(name).unwrap();
            let light = name == "yellow" || name == "white";
            assert_eq!(color.is_light(), light, "{name}");
            assert_eq!(color.contrast().is_light(), !light, "{name}");
        }
    }

    #[test]
    fn enums_parse_case_insensitively() {
        assert_eq!(ArrowSize::parse("l").unwrap(), ArrowSize::Large);
        assert_eq!(ArrowStyle::parse("RING").unwrap(), ArrowStyle::Ring);
        assert_eq!(ApproachDirection::parse("auto").unwrap(), None);
        assert_eq!(
            ApproachDirection::parse("Top-Left").unwrap(),
            Some(ApproachDirection::TopLeft)
        );
        assert!(ArrowSize::parse("XL").is_err());
        assert!(ApproachDirection::parse("north").is_err());
        assert!(ArrowBorder::parse("glow").is_err());
    }

    #[test]
    fn sizes_grow() {
        let [s, m, l] =
            [ArrowSize::Small, ArrowSize::Medium, ArrowSize::Large].map(ArrowSize::metrics);
        assert!(s.stroke < m.stroke && m.stroke < l.stroke);
        assert!(s.head_length < m.head_length && m.head_length < l.head_length);
        assert!(s.font_size >= ArrowMetrics::MINIMUM_FONT_SIZE);
    }
}
