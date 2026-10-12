//! One arrow on screen, from show to dismissal. Platform code only pumps events and puts pixels
//! on the screen; every decision is made here.

use std::fs::File;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;

use super::config::{ClickDismissal, PointConfig, TargetSpec};
use super::runner::{self, Planned};
use super::{output, targeting};
use crate::model::error::{Error, Result};
use crate::model::geom::{Point, Rect};
use crate::model::owner::ArrowOwner;
use crate::model::placement;
use crate::model::registry::{ArrowRecord, Registry, now_millis};
use crate::model::result::DismissReason;
use crate::model::target::ResolvedTarget;
use crate::platform::{self, Event, Overlay, PixelRect, Speech};
use crate::render::animation::{self, AnimationMode, INTRO_END, PULSE_BEGIN, PULSE_PERIOD};
use crate::render::click::ClickRegion;
use crate::render::text::TextEngine;

const INTRO_FRAME: Duration = Duration::from_millis(16);
const PULSE_FRAME: Duration = Duration::from_millis(33);
/// Pulse frames per half period, about 30 per second.
const PULSE_STEPS: usize = 14;
/// A pulse region larger than this many pixels stays still: redrawing it would cost too much.
const PULSE_LIMIT: u64 = 600_000;
const HOVER_ALPHA: f64 = 0.92;
const HOVER_FADE: f64 = 0.12;
const COPIED_FEEDBACK: Duration = Duration::from_millis(1500);
const FOLLOW_INTERVAL: Duration = Duration::from_millis(250);
const GONE_LIMIT: Duration = Duration::from_secs(2);
/// Window changes come in bursts while a window is dragged; at most one check per this interval.
const WINDOWS_SETTLE: Duration = Duration::from_millis(30);
/// Points around a point target where `--until-click` counts.
const CLICK_ZONE: f64 = 80.0;

pub fn run(config: PointConfig, planned: Planned, engine: TextEngine, child: bool) -> Result<()> {
    let mut session = Session::start(config, planned, engine, child)?;
    while session.done.is_none() {
        let event = session.overlay.wait(session.next_deadline());
        session.handle(event);
    }
    session.end();
    Ok(())
}

struct Session {
    config: PointConfig,
    planned: Planned,
    region: ClickRegion,
    engine: TextEngine,
    overlay: Overlay,
    /// The window, global pixels, and its content.
    window: PixelRect,
    frame: Pixmap,
    mode: AnimationMode,
    shown: Instant,
    pulse: bool,
    /// Pulse frames drawn once and replayed: the pulse repeats every 0.9 s.
    pulse_frames: Option<PulseFrames>,
    last_frame: Instant,
    registry: Registry,
    _lock: File,
    covered: bool,
    missing: bool,
    /// Hover opacity: where it goes, where it came from and since when.
    hover: (f64, f64, Instant),
    copies: u32,
    copied: Option<(usize, Instant)>,
    finishing: Option<(DismissReason, Instant, f64)>,
    done: Option<(DismissReason, f64)>,
    follow: Option<Receiver<Option<ResolvedTarget>>>,
    gone_since: Option<Instant>,
    windows_dirty: Option<Instant>,
    speech: Option<Speech>,
    child: bool,
}

impl Session {
    fn start(
        config: PointConfig,
        planned: Planned,
        engine: TextEngine,
        child: bool,
    ) -> Result<Self> {
        let mode = if config.no_animation {
            AnimationMode::None
        } else if platform::reduce_motion() {
            AnimationMode::Reduced
        } else {
            AnimationMode::Full
        };
        let owner = ArrowOwner::current();
        let window = window_rect(&planned);
        let mut overlay = Overlay::new(window, owner.pid)?;
        let mut planned = planned;
        planned.scene.shadow = overlay.translucent();
        if config.click != ClickDismissal::Off && !overlay.watch_clicks() {
            let unavailable =
                "Dismissing on click (--until-click) is not available in this session.";
            if !config.duration_explicit {
                return Err(Error::permission(unavailable));
            }
            output::print_error(&format!(
                "bigarrow: {unavailable} Falling back to --duration {} s.",
                output::number(config.duration)
            ));
        }
        let registry = Registry::new(platform::registry_dir());
        let lock = registry
            .claim(std::process::id())
            .map_err(|error| Error::bad_input(format!("cannot write the pid file: {error}")))?;
        let now = Instant::now();
        let region = ClickRegion::new(&planned.scene.layout, &planned.scene.sign);
        let frame = Pixmap::new(window.width, window.height)
            .ok_or_else(|| Error::bad_input("the arrow is too large to show"))?;
        let pulse = mode == AnimationMode::Full && pulse_area(&planned, window) <= PULSE_LIMIT;
        let mut session = Self {
            config,
            planned,
            region,
            engine,
            overlay,
            window,
            frame,
            mode,
            shown: now,
            pulse,
            pulse_frames: None,
            last_frame: now,
            registry,
            _lock: lock,
            covered: false,
            missing: false,
            hover: (1.0, 1.0, now),
            copies: 0,
            copied: None,
            finishing: None,
            done: None,
            follow: None,
            gone_since: None,
            windows_dirty: None,
            speech: None,
            child,
        };
        session.redraw();
        session.update_hit_mask();
        session
            .overlay
            .set_alpha(animation::entrance_alpha(mode, 0.0));
        // Recorded before it shows, so the other arrows find it when its window event wakes them.
        session.write_record()?;
        session.overlay.set_visible(true);
        if session.child {
            output::print("ready");
        }
        if session.config.home.is_some() {
            session.check_windows();
        }
        if session.config.follow && matches!(session.config.target, TargetSpec::Element(..)) {
            session.follow = Some(follow_elements(
                session.config.target.clone(),
                session.overlay.waker(),
            ));
        }
        if session.config.say {
            session.speech = Speech::start(&session.config.text, session.config.voice.as_deref());
        }
        Ok(session)
    }

    fn write_record(&self) -> Result<()> {
        let layout = &self.planned.scene.layout;
        let anchor = self.planned.resolved.shape.anchor();
        let owner = ArrowOwner::current();
        let record = ArrowRecord {
            direction: layout.direction.name().to_owned(),
            display: layout.display.number(),
            owner: (!owner.is_empty()).then_some(owner),
            pid: std::process::id(),
            sign_frame: layout.global_sign().to_array(),
            started_at: now_millis(),
            target_frame: Some(layout.global_target().to_array()),
            target_x: anchor.x,
            target_y: anchor.y,
            text: self.config.text.clone(),
        };
        self.registry
            .write(&record)
            .map_err(|error| Error::bad_input(format!("cannot write the pid file: {error}")))
    }

    fn elapsed(&self) -> f64 {
        self.shown.elapsed().as_secs_f64()
    }

    fn hidden(&self) -> bool {
        self.covered || self.missing
    }

    /// How often the window needs a new frame now, if at all.
    fn frame_interval(&self) -> Option<Duration> {
        if self.hidden() {
            return None;
        }
        let t = self.elapsed();
        let entering = match self.mode {
            AnimationMode::Full => t < INTRO_END,
            AnimationMode::Reduced => t < 0.1,
            AnimationMode::None => false,
        };
        if entering || self.hover.2.elapsed().as_secs_f64() < HOVER_FADE {
            Some(INTRO_FRAME)
        } else if self.pulse {
            Some(PULSE_FRAME)
        } else {
            None
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.finishing.is_some() {
            return Some(self.last_frame + INTRO_FRAME);
        }
        let mut deadlines: Vec<Instant> = Vec::new();
        if self.config.duration > 0.0 {
            deadlines.push(self.shown + Duration::from_secs_f64(self.config.duration));
        }
        if let Some(interval) = self.frame_interval() {
            deadlines.push(self.last_frame + interval);
        }
        if let Some((_, at)) = self.copied {
            deadlines.push(at + COPIED_FEEDBACK);
        }
        if let Some(since) = self.gone_since {
            deadlines.push(since + GONE_LIMIT);
        }
        if let Some(at) = self.windows_dirty {
            deadlines.push(at + WINDOWS_SETTLE);
        }
        deadlines.into_iter().min()
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Timeout => self.tick(),
            Event::Pointer(point) => self.pointer(point),
            Event::Click(point) => self.click(point),
            Event::GlobalClick(point) => {
                if self.config.click == ClickDismissal::Anywhere
                    || self.click_zone().contains(point)
                {
                    self.finish(DismissReason::Clicked);
                }
            }
            Event::Stop => self.finish(DismissReason::Signal),
            Event::OwnerGone => self.finish(DismissReason::OwnerGone),
            Event::DisplaysChanged => {
                let id = self.planned.scene.layout.display.id;
                let present = platform::screens()
                    .is_ok_and(|screens| screens.displays.iter().any(|display| display.id == id));
                if !present {
                    self.finish(DismissReason::DisplayGone);
                }
            }
            // A newer arrow's window counts too: it may point under this sign.
            Event::WindowsChanged => {
                self.windows_dirty.get_or_insert_with(Instant::now);
            }
            Event::Wake => self.drain_follow(),
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if let Some((reason, since, from)) = self.finishing {
            let fade = self.mode.fade_out();
            let progress = if fade > 0.0 {
                since.elapsed().as_secs_f64() / fade
            } else {
                1.0
            };
            if progress >= 1.0 || self.hidden() {
                self.done = Some((reason, self.elapsed()));
                return;
            }
            self.overlay.set_alpha(from * (1.0 - progress));
            self.last_frame = now;
            return;
        }
        if self.config.duration > 0.0 && self.elapsed() >= self.config.duration {
            self.finish(DismissReason::Timeout);
            return;
        }
        if self
            .windows_dirty
            .is_some_and(|at| now >= at + WINDOWS_SETTLE)
        {
            self.windows_dirty = None;
            self.check_windows();
        }
        if self
            .gone_since
            .is_some_and(|since| now >= since + GONE_LIMIT)
            && self.config.duration > 0.0
        {
            self.finish(DismissReason::TargetGone);
            return;
        }
        if self
            .copied
            .is_some_and(|(_, at)| now >= at + COPIED_FEEDBACK)
        {
            self.copied = None;
            self.redraw();
        }
        if self
            .frame_interval()
            .is_some_and(|interval| now >= self.last_frame + interval)
        {
            self.animate();
        }
    }

    /// Draws the next frame: the whole window during the entrance, only head and mark after;
    /// fades and hover dimming only change the window's opacity.
    fn animate(&mut self) {
        self.overlay.set_alpha(self.alpha());
        self.last_frame = Instant::now();
        let t = self.elapsed();
        if self.mode != AnimationMode::Full {
            return;
        }
        if t < INTRO_END + 0.05 {
            self.redraw();
            return;
        }
        if !self.pulse {
            return;
        }
        let copied = self.copied.map(|(index, _)| index);
        if self
            .pulse_frames
            .as_ref()
            .is_none_or(|frames| frames.copied != copied)
        {
            self.pulse_frames = PulseFrames::draw(&self.planned, self.window, copied);
        }
        let Some(frames) = &self.pulse_frames else {
            return;
        };
        // The pulse grows for half a period and shrinks back the same way.
        let half = PULSE_PERIOD / 2.0;
        let phase = (t - PULSE_BEGIN).max(0.0) % PULSE_PERIOD;
        let rising = if phase < half {
            phase / half
        } else {
            2.0 - phase / half
        };
        let index = (rising * PULSE_STEPS as f64).round() as usize;
        copy_into(
            &mut self.frame,
            &frames.parts[index],
            frames.source,
            frames.dirty,
            self.window,
        );
        self.overlay.present(&self.frame, Some(frames.dirty));
    }

    /// Redraws the whole window at the current time.
    fn redraw(&mut self) {
        let scene = &self.planned.scene;
        let display = &scene.layout.display;
        self.frame.fill(tiny_skia::Color::TRANSPARENT);
        let origin = display.local(Point::new(
            f64::from(self.window.x),
            f64::from(self.window.y),
        ));
        let state = animation::frame_at(self.mode, self.elapsed());
        scene.draw(
            &mut self.frame,
            origin,
            display.scale,
            &state,
            self.copied.map(|(index, _)| index),
        );
        self.overlay.present(&self.frame, None);
        self.last_frame = Instant::now();
    }

    /// Where clicks count: the sign and the shaft, not the head or the target.
    fn update_hit_mask(&mut self) {
        let display = &self.planned.scene.layout.display;
        let origin = display.local(Point::new(
            f64::from(self.window.x),
            f64::from(self.window.y),
        ));
        let mask = self
            .region
            .mask(self.window.width, self.window.height, origin, display.scale);
        self.overlay.set_hit_mask(&mask);
    }

    /// Window opacity: entrance fade (reduced motion) times hover dimming.
    fn alpha(&self) -> f64 {
        let (to, from, since) = self.hover;
        let progress = (since.elapsed().as_secs_f64() / HOVER_FADE).min(1.0);
        animation::entrance_alpha(self.mode, self.elapsed()) * (from + (to - from) * progress)
    }

    /// The pointer over the click region (global pixels), or `None` once it left. The arrow dims
    /// to say a click removes it; never over a copy chip, where a click copies.
    fn pointer(&mut self, point: Option<Point>) {
        let display = &self.planned.scene.layout.display;
        let dims = point.is_some_and(|point| self.region.chip_at(display.local(point)).is_none());
        let target = if dims { HOVER_ALPHA } else { 1.0 };
        if (target - self.hover.0).abs() > f64::EPSILON {
            self.hover = (target, self.alpha(), Instant::now());
            self.overlay.set_alpha(self.alpha());
        }
    }

    /// A chip copies its value and keeps the arrow; anywhere else on the sign or shaft closes it.
    fn click(&mut self, point: Point) {
        let local = self.planned.scene.layout.display.local(point);
        let Some(index) = self.region.chip_at(local) else {
            self.finish(DismissReason::Closed);
            return;
        };
        if platform::set_clipboard(&self.planned.scene.sign.chips[index].value) {
            self.copies += 1;
            self.copied = Some((index, Instant::now()));
            self.redraw();
        }
    }

    /// The target rect grown by 8 points, or a square around a point target, global pixels.
    fn click_zone(&self) -> Rect {
        let scale = self.planned.scene.layout.display.scale;
        let shape = self.planned.resolved.shape;
        match shape.rect() {
            Some(rect) => rect.inset(-8.0 * scale, -8.0 * scale),
            None => {
                let half = CLICK_ZONE * scale / 2.0;
                let anchor = shape.anchor();
                Rect::new(anchor.x - half, anchor.y - half, half * 2.0, half * 2.0)
            }
        }
    }

    /// Re-checks what window events may have changed: whether a newer arrow points under this
    /// sign, whether another app covers the target, and where a followed window went.
    fn check_windows(&mut self) {
        self.make_way();
        let follows_window =
            self.config.follow && matches!(self.config.target, TargetSpec::Window(..));
        if !follows_window && self.config.home.is_none() {
            return;
        }
        let Ok(list) = platform::window_list() else {
            return;
        };
        if follows_window {
            let resolved = platform::screens()
                .ok()
                .and_then(|screens| targeting::resolve(&self.config.target, &screens).ok());
            self.followed(resolved);
        }
        if let Some(home) = &self.config.home {
            let covered = targeting::is_covered(home, self.planned.resolved.shape.anchor(), &list);
            self.set_hidden(covered, self.missing);
        }
    }

    fn drain_follow(&mut self) {
        let Some(receiver) = &self.follow else { return };
        let latest = receiver.try_iter().last();
        if let Some(resolved) = latest {
            self.followed(resolved);
        }
    }

    /// Moves the arrow with its target; a target gone for 2 s ends it, unless the arrow is sticky.
    fn followed(&mut self, resolved: Option<ResolvedTarget>) {
        let Some(resolved) = resolved else {
            self.gone_since.get_or_insert_with(Instant::now);
            self.set_hidden(self.covered, true);
            return;
        };
        self.gone_since = None;
        self.set_hidden(self.covered, false);
        let scale = self.planned.scene.layout.display.scale;
        let (old, new) = (self.planned.resolved.shape, resolved.shape);
        let moved = old.anchor().distance(new.anchor()) > 0.5 * scale
            || old.rect().map(|r| (r.width, r.height)) != new.rect().map(|r| (r.width, r.height));
        if moved {
            self.move_to(resolved);
        }
    }

    /// Places the sign again when it covers the target of an arrow that showed after it. Signs
    /// planned later already keep off this arrow, so moving settles after one step.
    fn make_way(&mut self) {
        let layout = &self.planned.scene.layout;
        let covers = runner::others().iter().any(|(number, other)| {
            *number == layout.display.number()
                && layout.sign_rect.intersects(&placement::keep_out_zone(
                    layout.display.local_rect(&other.target),
                ))
        });
        if !covers {
            return;
        }
        let Some(planned) = self.replan(self.planned.resolved.clone()) else {
            return;
        };
        if planned.scene.layout.sign_rect != self.planned.scene.layout.sign_rect {
            self.apply(planned);
        }
    }

    fn move_to(&mut self, resolved: ResolvedTarget) {
        if let Some(planned) = self.replan(resolved) {
            self.apply(planned);
        }
    }

    fn replan(&mut self, resolved: ResolvedTarget) -> Option<Planned> {
        let screens = platform::screens().ok()?;
        let mut planned = runner::plan(
            &self.config,
            resolved,
            &screens,
            &mut self.engine,
            &runner::others(),
        )
        .ok()?;
        planned.scene.shadow = self.overlay.translucent();
        Some(planned)
    }

    fn apply(&mut self, planned: Planned) {
        self.region = ClickRegion::new(&planned.scene.layout, &planned.scene.sign);
        self.window = window_rect(&planned);
        self.planned = planned;
        self.pulse_frames = None;
        let Some(frame) = Pixmap::new(self.window.width, self.window.height) else {
            return;
        };
        self.frame = frame;
        let _ = self.write_record();
        self.overlay.set_rect(self.window);
        self.redraw();
        self.update_hit_mask();
    }

    fn set_hidden(&mut self, covered: bool, missing: bool) {
        let was = self.hidden();
        (self.covered, self.missing) = (covered, missing);
        if was != self.hidden() {
            self.overlay.set_visible(!self.hidden());
            if !self.hidden() {
                self.redraw();
            }
        }
    }

    /// Starts the exit fade; the loop ends once it is over.
    fn finish(&mut self, reason: DismissReason) {
        if self.finishing.is_some() || self.done.is_some() {
            return;
        }
        self.speech = None;
        self.follow = None;
        if self.mode.fade_out() == 0.0 || self.hidden() {
            self.done = Some((reason, self.elapsed()));
            return;
        }
        self.finishing = Some((reason, Instant::now(), self.alpha()));
    }

    fn end(mut self) {
        self.overlay.close();
        self.registry.remove(std::process::id());
        let Some((reason, after)) = self.done else {
            return;
        };
        if self.child {
            return;
        }
        let mut result = runner::result(&self.planned, std::process::id());
        result.dismissed_after = Some((after * 100.0).round() / 100.0);
        result.dismissed_reason = Some(reason);
        result.copied = (self.copies > 0).then_some(self.copies);
        runner::report(&result, self.config.json);
    }
}

/// The pulse's region drawn at every step of its rising half, from rest to full size.
struct PulseFrames {
    copied: Option<usize>,
    /// What changes (global pixels) and the larger area drawn around it, so its shadow is whole.
    dirty: PixelRect,
    source: PixelRect,
    parts: Vec<Pixmap>,
}

impl PulseFrames {
    fn draw(planned: &Planned, window: PixelRect, copied: Option<usize>) -> Option<Self> {
        let scene = &planned.scene;
        let display = &scene.layout.display;
        let dirty =
            PixelRect::enclosing(display.global_rect(&scene.pulse_bounds())).intersect(&window)?;
        let reach = (25.0 * display.scale).ceil() as i32;
        let source = dirty.grown(reach).intersect(&window)?;
        let origin = display.local(Point::new(f64::from(source.x), f64::from(source.y)));
        let half = PULSE_PERIOD / 2.0;
        let parts = (0..=PULSE_STEPS)
            .map(|step| {
                let t = PULSE_BEGIN + half * step as f64 / PULSE_STEPS as f64;
                let mut part = Pixmap::new(source.width, source.height)?;
                scene.draw(
                    &mut part,
                    origin,
                    display.scale,
                    &animation::frame_at(AnimationMode::Full, t),
                    copied,
                );
                Some(part)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            copied,
            dirty,
            source,
            parts,
        })
    }
}

/// The window around everything the arrow can cover, global pixels, inside its display.
fn window_rect(planned: &Planned) -> PixelRect {
    let display = &planned.scene.layout.display;
    let rect = PixelRect::enclosing(display.global_rect(&planned.scene.bounds()));
    rect.intersect(&PixelRect::enclosing(display.frame))
        .unwrap_or(rect)
}

fn pulse_area(planned: &Planned, window: PixelRect) -> u64 {
    let display = &planned.scene.layout.display;
    PixelRect::enclosing(display.global_rect(&planned.scene.pulse_bounds()))
        .intersect(&window)
        .map_or(0, |rect| u64::from(rect.width) * u64::from(rect.height))
}

/// Copies `dirty` (global pixels) from `part` (covering `source`) into `frame` (covering `window`).
fn copy_into(
    frame: &mut Pixmap,
    part: &Pixmap,
    source: PixelRect,
    dirty: PixelRect,
    window: PixelRect,
) {
    let row = dirty.width as usize * 4;
    let frame_width = frame.width() as usize;
    let part_width = part.width() as usize;
    let data = frame.data_mut();
    for y in 0..dirty.height as usize {
        let from_y = (dirty.y - source.y) as usize + y;
        let from_x = (dirty.x - source.x) as usize;
        let to_y = (dirty.y - window.y) as usize + y;
        let to_x = (dirty.x - window.x) as usize;
        let from = (from_y * part_width + from_x) * 4;
        let to = (to_y * frame_width + to_x) * 4;
        data[to..to + row].copy_from_slice(&part.data()[from..from + row]);
    }
}

/// `--follow --element`: re-resolves every 250 ms on a thread of its own, because accessibility
/// calls can block; the newest result wakes the session.
fn follow_elements(spec: TargetSpec, waker: platform::Waker) -> Receiver<Option<ResolvedTarget>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(FOLLOW_INTERVAL);
            let resolved = platform::screens()
                .ok()
                .and_then(|screens| targeting::resolve(&spec, &screens).ok());
            if sender.send(resolved).is_err() {
                return;
            }
            waker.wake();
        }
    });
    receiver
}
