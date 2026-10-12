//! Validated `point`/`start` input. Every check that can fail on bad input happens here, before
//! anything is drawn, in the same order as bigarrow for macOS, so scripts see the same first error.

use std::path::PathBuf;

use super::options::PointArgs;
use crate::model::appearance::{
    ApproachDirection, ArrowBorder, ArrowColor, ArrowShape, ArrowSize, ArrowStyle, SignAppearance,
    SignCorners,
};
use crate::model::error::{Error, Result};
use crate::model::geom::{Point, Rect};
use crate::model::matcher::{ElementQuery, WindowAnchor, WindowQuery};
use crate::model::sign_text::SignText;
use crate::model::target::{parse_point, parse_rect};

/// How `point` and `start` differ: `start` runs in the background, 300 s by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Foreground,
    Background,
}

/// When a click ends the arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickDismissal {
    Off,
    OnTarget,
    Anywhere,
}

/// The one target `point` was asked for, before resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetSpec {
    Coordinate(Point, Option<i64>),
    Rect(Rect, Option<i64>),
    Mouse,
    Window(WindowQuery, WindowAnchor, String),
    Element(ElementQuery, Option<String>),
}

impl TargetSpec {
    /// Only targets that can move are followed.
    pub fn is_followable(&self) -> bool {
        matches!(self, Self::Window(..) | Self::Element(..))
    }
}

pub const DEFAULT_DURATION: f64 = 8.0;
/// Arrows that wait for the human still end by themselves, in case nobody runs `stop`.
pub const HUMAN_WAIT_DURATION: f64 = 300.0;

#[derive(Debug, Clone)]
pub struct PointConfig {
    pub target: TargetSpec,
    /// The app (and window or tab) the target is in: raised first, and the arrow hides while covered.
    pub home: Option<WindowQuery>,
    pub sign: SignText,
    /// The sentence without braces.
    pub text: String,
    pub duration: f64,
    pub duration_explicit: bool,
    pub forced: Option<ApproachDirection>,
    pub look: SignAppearance,
    pub style: ArrowStyle,
    pub shape: ArrowShape,
    pub no_animation: bool,
    pub say: bool,
    pub voice: Option<String>,
    pub follow: bool,
    pub click: ClickDismissal,
    pub raise: bool,
    pub json: bool,
    pub detach: bool,
    pub dry_run: bool,
    pub png: Option<PathBuf>,
}

impl PointConfig {
    pub fn new(args: &PointArgs, mode: Mode) -> Result<Self> {
        let (options, look_options, behaviour) = (&args.target, &args.look, &args.behaviour);
        let home = home(options.window.as_deref(), options.app.as_deref())?;
        let target = target(args, home.as_ref())?;
        let sign = SignText::parse(look_options.text.trim())?;
        let text = sign.plain();
        if text.is_empty() {
            return Err(Error::bad_input(
                "--text must not be empty, write what the human should do",
            ));
        }
        let forced = ApproachDirection::parse(&look_options.from)?;
        let size = ArrowSize::parse(&look_options.size)?;
        let style = ArrowStyle::parse(&look_options.style)?;
        let color = ArrowColor::parse(&look_options.color)?;
        let corners = SignCorners::parse(&look_options.corners)?;
        let shape = ArrowShape::parse(&look_options.shape)?;
        let border = ArrowBorder::parse(&look_options.border)?;
        let tint = |raw: &Option<String>| raw.as_deref().map(ArrowColor::parse).transpose();
        let mut look = SignAppearance::new(color, size, corners);
        look.border = border;
        look.border_color = tint(&look_options.border_color)?;
        look.text_color = tint(&look_options.text_color)?;
        look.edge_color = tint(&look_options.edge_color)?;
        look.close_color = tint(&look_options.close_color)?;
        look.close_x_color = tint(&look_options.close_x_color)?;
        look.close_button = behaviour.close_button;
        let click = match (behaviour.until_click, behaviour.until_any_click) {
            (true, true) => {
                return Err(Error::bad_input(
                    "use --until-click or --until-any-click, not both",
                ));
            }
            (true, false) => ClickDismissal::OnTarget,
            (false, true) => ClickDismissal::Anywhere,
            (false, false) => ClickDismissal::Off,
        };
        let waits_for_human =
            click != ClickDismissal::Off || behaviour.close_button || mode == Mode::Background;
        let duration = behaviour.duration.unwrap_or(if waits_for_human {
            HUMAN_WAIT_DURATION
        } else {
            DEFAULT_DURATION
        });
        if !(duration >= 0.0 && duration.is_finite()) {
            return Err(Error::bad_input("--duration must be 0 or more seconds"));
        }
        if behaviour.follow && !target.is_followable() {
            return Err(Error::bad_input(
                "--follow works with --window and --element only",
            ));
        }
        Ok(Self {
            raise: home.is_some() && !options.no_raise,
            home,
            target,
            sign,
            text,
            duration,
            duration_explicit: behaviour.duration.is_some(),
            forced,
            look,
            style,
            shape,
            no_animation: look_options.no_animation,
            say: behaviour.say,
            voice: behaviour.voice.clone(),
            follow: behaviour.follow,
            click,
            json: behaviour.json,
            detach: behaviour.detach || mode == Mode::Background,
            dry_run: behaviour.dry_run,
            png: behaviour.png.as_ref().map(PathBuf::from),
        })
    }
}

/// `--window App[:title]` is its own home; every other target may name one with `--app`.
fn home(window: Option<&str>, app: Option<&str>) -> Result<Option<WindowQuery>> {
    match (window, app) {
        (Some(_), Some(_)) => Err(Error::bad_input(
            "--window already names the app, drop --app",
        )),
        (Some(window), None) => WindowQuery::parse(window).map(Some),
        (None, app) => app.map(WindowQuery::parse).transpose(),
    }
}

fn target(args: &PointArgs, home: Option<&WindowQuery>) -> Result<TargetSpec> {
    let options = &args.target;
    let given = [
        options.at.is_some(),
        options.rect.is_some(),
        options.mouse,
        options.window.is_some(),
        options.element.is_some(),
    ];
    if given.iter().filter(|given| **given).count() != 1 {
        return Err(Error::bad_input(
            "give exactly one target: --at, --rect, --mouse, --window or --element",
        ));
    }
    if options.display.is_some() && options.at.is_none() && options.rect.is_none() {
        return Err(Error::bad_input(
            "--display only applies to --at and --rect",
        ));
    }
    if let Some(at) = &options.at {
        return Ok(TargetSpec::Coordinate(parse_point(at)?, options.display));
    }
    if let Some(rect) = &options.rect {
        return Ok(TargetSpec::Rect(parse_rect(rect)?, options.display));
    }
    if options.mouse {
        return Ok(TargetSpec::Mouse);
    }
    if let Some(window) = &options.window {
        let anchor = WindowAnchor::parse(&options.anchor)?;
        return Ok(TargetSpec::Window(
            WindowQuery::parse(window)?,
            anchor,
            options.anchor.to_lowercase(),
        ));
    }
    let element = options.element.as_deref().unwrap_or_default();
    Ok(TargetSpec::Element(
        ElementQuery::new(element, options.role.as_deref())?,
        home.map(|home| home.app.clone()),
    ))
}
