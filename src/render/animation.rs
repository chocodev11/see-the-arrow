//! The entrance and the pulse (draw-on shaft, springs, eased pulse, fades), as plain functions of
//! the time since the arrow showed.

/// How the arrow enters and leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationMode {
    /// Draw-on shaft, spring head and sign, pulsing head and mark.
    Full,
    /// Reduce motion: short fades only, no pulse.
    Reduced,
    /// `--no-animation`: final frame immediately, no fades.
    None,
}

impl AnimationMode {
    /// Seconds the exit fade takes.
    pub fn fade_out(self) -> f64 {
        match self {
            Self::Full => 0.2,
            Self::Reduced => 0.1,
            Self::None => 0.0,
        }
    }
}

/// What changes from frame to frame; everything else is fixed by the layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameState {
    /// Share of the shaft drawn, 0..1.
    pub shaft: f64,
    /// Scale of the head around the tip.
    pub head: f64,
    /// Scale of the sign around its centre, and its opacity.
    pub sign: f64,
    pub sign_alpha: f64,
    /// Scale of the box or ring around its centre, and its opacity.
    pub mark: f64,
    pub mark_alpha: f64,
}

impl FrameState {
    pub const FINAL: Self = Self {
        shaft: 1.0,
        head: 1.0,
        sign: 1.0,
        sign_alpha: 1.0,
        mark: 1.0,
        mark_alpha: 1.0,
    };
}

const DRAW_ON: f64 = 0.45;
const HEAD_BEGIN: f64 = DRAW_ON * 0.8;
pub const PULSE_BEGIN: f64 = DRAW_ON + 0.5;
pub const PULSE_PERIOD: f64 = 0.9;
pub const PULSE_SCALE: f64 = 1.08;
/// After this the only motion left is the pulse.
pub const INTRO_END: f64 = 1.3;
/// The spring overshoots: heads grow to about this much of their size, signs to 1.06.
pub const HEAD_OVERSHOOT: f64 = 1.23;
pub const SIGN_OVERSHOOT: f64 = 1.06;

pub fn frame_at(mode: AnimationMode, t: f64) -> FrameState {
    if mode != AnimationMode::Full {
        return FrameState::FINAL;
    }
    let pulse = pulse(t);
    FrameState {
        shaft: cubic_bezier((0.0, 0.0, 0.58, 1.0), (t / DRAW_ON).min(1.0)),
        head: if t < HEAD_BEGIN {
            0.2
        } else if t < PULSE_BEGIN {
            spring(0.2, t - HEAD_BEGIN)
        } else {
            pulse
        },
        sign: spring(0.8, t),
        sign_alpha: (t / 0.18).min(1.0),
        mark: if t < PULSE_BEGIN { 1.0 } else { pulse },
        mark_alpha: (t / 0.25).min(1.0),
    }
}

/// Window opacity during the entrance: only reduced motion fades the whole arrow in.
pub fn entrance_alpha(mode: AnimationMode, t: f64) -> f64 {
    match mode {
        AnimationMode::Reduced => (t / 0.1).min(1.0),
        _ => 1.0,
    }
}

/// 1 → 1.08 → 1, ease in and out each way, forever.
fn pulse(t: f64) -> f64 {
    if t < PULSE_BEGIN {
        return 1.0;
    }
    let half = PULSE_PERIOD / 2.0;
    let phase = (t - PULSE_BEGIN) % PULSE_PERIOD;
    let progress = if phase < half {
        phase / half
    } else {
        1.0 - (phase - half) / half
    };
    1.0 + (PULSE_SCALE - 1.0) * cubic_bezier((0.42, 0.0, 0.58, 1.0), progress)
}

/// Core Animation's spring with damping 11, stiffness 220, mass 1, from `from` to 1.
fn spring(from: f64, t: f64) -> f64 {
    const DECAY: f64 = 5.5;
    const SETTLED: f64 = 1.26;
    if t <= 0.0 {
        return from;
    }
    if t >= SETTLED {
        return 1.0;
    }
    let omega = (220.0f64 - DECAY * DECAY).sqrt();
    1.0 - (1.0 - from)
        * (-DECAY * t).exp()
        * ((omega * t).cos() + DECAY / omega * (omega * t).sin())
}

/// CSS/Core Animation timing curve through (0,0), (x1,y1), (x2,y2), (1,1).
fn cubic_bezier((x1, y1, x2, y2): (f64, f64, f64, f64), progress: f64) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    let curve = |a: f64, b: f64, s: f64| {
        3.0 * a * s * (1.0 - s).powi(2) + 3.0 * b * s * s * (1.0 - s) + s.powi(3)
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..30 {
        let mid = (low + high) / 2.0;
        if curve(x1, x2, mid) < progress {
            low = mid;
        } else {
            high = mid;
        }
    }
    curve(y1, y2, (low + high) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_starts_small_and_settles() {
        let start = frame_at(AnimationMode::Full, 0.0);
        assert!(start.shaft.abs() < 1e-9);
        assert_eq!((start.head, start.sign, start.sign_alpha), (0.2, 0.8, 0.0));
        let settled = frame_at(AnimationMode::Full, 0.94);
        assert!((settled.shaft - 1.0).abs() < 1e-9);
        let peak = (0..200)
            .map(|step| spring(0.2, f64::from(step) * 0.01))
            .fold(0.0, f64::max);
        assert!(peak > 1.2 && peak <= HEAD_OVERSHOOT, "{peak}");
        let pulse_peak = frame_at(AnimationMode::Full, PULSE_BEGIN + PULSE_PERIOD / 2.0).head;
        assert!((pulse_peak - PULSE_SCALE).abs() < 1e-6);
        assert_eq!(frame_at(AnimationMode::Reduced, 0.0), FrameState::FINAL);
    }
}
