// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a motion layer's motion as segments (RFC-0109 §motion layers, TASK-0354 P9) — the
//! pure part of exits and turns. A layer's id is its life: the app flips `exit` on the same id
//! to close it and again to re-open it. Each direction is a segment with a start, a direction,
//! an optional turn state `(q0, v0)` and the backdrop's value at its start. The progress `q`
//! runs from the from-state (0) to the rest (1): an enter on the token's curve `E(p)`, an exit
//! `1 − E(p)`. A flip seen at the present is turned at the next frame's shown time: when the
//! interrupted segment was still moving, the new one is a critically damped `animation::Spring`
//! from where it was and how fast it moved (continuous in place and velocity, evaluated in
//! closed form — RFC-0110), else the design curve runs from the settled end. Also the motion's
//! own instrument: frames drawn while it moved against the refresh periods in its length.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `motion_geometry::exit_tests`, `layer_geometry_tests`
//! INVARIANTS: an uninterrupted enter's `q` is the token's curve unchanged; `q(at)` of a turned
//!   segment equals the interrupted one's at the turn; `q` stays in [Q_MIN, Q_MAX] and `|v0|`
//!   at most V_MAX; only the present of the owner's own structure flips its layers.

use super::{backdrop_at, in_flight, length_ns};
use animation::{MotionToken, Spring, SpringConfig};
use nexus_display_proto::surface_layers::{MotionLayerDesc, SurfaceTree, MAX_MOTION_LAYERS};

/// The bounds of a motion's progress: a spring may pass its rest or its from-state a little,
/// never far (the curated curves stay inside them: bezier `y` in 0..~1.1).
pub(crate) const Q_MIN: f32 = -0.25;
pub(crate) const Q_MAX: f32 = 1.25;
/// The fastest a turned segment starts, in progress per second (a whole motion in 20 ms).
pub(crate) const V_MAX: f32 = 50.0;
/// `ω · duration` of a turned segment's spring: critically damped, it is within ~0.1 % of its
/// target at the duration, where it is snapped.
const SETTLE: f32 = 9.2;
/// The step of the velocity's finite difference (ns).
const STEP_NS: u64 = 1_000_000;

/// One direction of a layer's motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Segment {
    /// The shown time of the frame that first drew it (ns; `0`: not drawn yet).
    pub(crate) start: u64,
    /// Toward the from-state (`MOTION_EXIT`), else toward the rest.
    pub(crate) exit: bool,
    /// A turned segment's progress and velocity (per second) at its start, bounded; `None`: the
    /// token's design curve.
    pub(crate) turn: Option<(f32, f32)>,
    /// The backdrop's multiplier at its start (0..=255): an enter fades it on to 255, an exit
    /// down to 0.
    pub(crate) b0: u32,
}

impl Segment {
    /// A layer's first segment: an enter from its from-state (its backdrop from transparent), a
    /// layer first declared leaving from its rest (its backdrop drawn).
    pub(crate) const fn fresh(exit: bool) -> Self {
        Self { start: 0, exit, turn: None, b0: if exit { 255 } else { 0 } }
    }

    /// Where the segment goes: the from-state (0) or the rest (1).
    const fn target(&self) -> f32 {
        if self.exit {
            0.0
        } else {
            1.0
        }
    }
}

#[cfg(test)]
impl Segment {
    /// An enter first drawn at `start` (`0`: not drawn yet).
    pub(crate) const fn enter_at(start: u64) -> Self {
        Self { start, ..Self::fresh(false) }
    }
}

/// What windowd keeps of a motion layer between frames, carried by id across structures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MotionState {
    /// The segment in effect.
    pub(crate) seg: Segment,
    /// A flip seen at the present and not yet drawn: the segment it interrupted and the layer
    /// as the structure then declared it — turned at the next frame's shown time.
    flipped: Option<(Segment, MotionLayerDesc)>,
    /// The instrument of the segment in effect.
    run: Run,
}

impl MotionState {
    pub(crate) const EMPTY: Self = Self::fresh(false);

    /// A new id's state: its first segment, nothing drawn.
    pub(crate) const fn fresh(exit: bool) -> Self {
        Self { seg: Segment::fresh(exit), flipped: None, run: Run::NONE }
    }

    /// An enter first drawn at `start`.
    #[cfg(test)]
    pub(crate) const fn started(start: u64) -> Self {
        Self { seg: Segment::enter_at(start), ..Self::EMPTY }
    }

    /// A flip waits for its frame (it owes one even when the new segment has no length).
    pub(crate) const fn flip_pending(&self) -> bool {
        self.flipped.is_some()
    }

    /// The state after the structure flipped the layer to `exit` (it was declared `old`). A
    /// flip back before any frame drew the first one leaves the interrupted segment running.
    fn flip(self, old: &MotionLayerDesc, exit: bool) -> Self {
        match self.flipped {
            Some((prev, _)) if self.seg.start == 0 && prev.exit == exit => {
                Self { seg: prev, flipped: None, run: self.run }
            }
            _ => Self {
                seg: Segment { start: 0, exit, turn: None, b0: 0 },
                flipped: Some((self.seg, *old)),
                run: self.run,
            },
        }
    }
}

/// The states of `next`'s motion layers: a layer whose id `now` declared keeps its state — the
/// same motion continues across structures — and a flipped `exit` is recorded for the next
/// frame; a new id starts with the frame that first draws it.
pub(crate) fn carry(
    now: &SurfaceTree,
    states: &[MotionState; MAX_MOTION_LAYERS],
    next: &SurfaceTree,
) -> [MotionState; MAX_MOTION_LAYERS] {
    let mut out = [MotionState::EMPTY; MAX_MOTION_LAYERS];
    for (slot, m) in out.iter_mut().zip(next.motions()) {
        *slot = match now.motions().iter().position(|o| o.id == m.id) {
            Some(k) if states[k].seg.exit == m.exit => states[k],
            Some(k) => states[k].flip(&now.motions()[k], m.exit),
            None => MotionState::fresh(m.exit),
        };
    }
    out
}

/// The progress of segment `seg` of layer `m` (token `token`) at `at` while it moves —
/// `None` once it is over or when it has no length: it stands at its target. A segment not
/// drawn yet is at its start.
pub(crate) fn progress(
    token: MotionToken,
    m: &MotionLayerDesc,
    seg: &Segment,
    at: u64,
) -> Option<f32> {
    let len = u64::from(m.duration_ms) * 1_000_000;
    if len == 0 {
        return None;
    }
    let t = if seg.start == 0 { 0.0 } else { at.saturating_sub(seg.start) as f32 / len as f32 };
    if t >= 1.0 {
        return None;
    }
    let q = match seg.turn {
        None => {
            let e = token.easing().ease(t);
            if seg.exit {
                1.0 - e
            } else {
                e
            }
        }
        Some((q0, v0)) => {
            // The spring runs in the offset from `q0`: at its start it is exactly `q0`.
            let w = SETTLE / (len as f32 * 1e-9);
            let config = SpringConfig {
                stiffness: w * w,
                damping: 2.0 * w,
                mass: 1.0,
                initial_velocity: v0,
            };
            let (t0, now) = if seg.start == 0 { (0, 0) } else { (seg.start, at) };
            q0 + Spring::new(0.0, seg.target() - q0, config, t0).at(now).0
        }
    };
    Some(q.clamp(Q_MIN, Q_MAX))
}

/// `x` inside `lo..=hi`; a value that is not finite is 0.
fn bounded(x: f32, lo: f32, hi: f32) -> f32 {
    if x.is_finite() {
        x.clamp(lo, hi)
    } else {
        0.0
    }
}

/// The turn state of a flip at `at` from segment `prev` of the layer as declared `old`: its
/// progress and velocity (a forward 1 ms difference) when it still moved, bounded; `None` when
/// it was over — the new segment's design curve runs from the settled end.
pub(crate) fn turn_from(old: &MotionLayerDesc, prev: &Segment, at: u64) -> Option<(f32, f32)> {
    let token = MotionToken::from_id(old.token)?;
    let q = progress(token, old, prev, at)?;
    let ahead = progress(token, old, prev, at.saturating_add(STEP_NS)).unwrap_or(prev.target());
    let v = (ahead - q) * (1e9 / STEP_NS as f32);
    Some((bounded(q, Q_MIN, Q_MAX), bounded(v, -V_MAX, V_MAX)))
}

/// What the frame being built knows of itself: the refresh period (ns; `0`: not timed yet) and
/// the pixels it redraws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameNote {
    pub(crate) period_ns: u64,
    pub(crate) damage_px: u64,
}

/// A segment's instrument while it moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    /// Frames drawn while it moved.
    frames: u32,
    /// The shown time of the last of them.
    last_at: u64,
    /// The most a frame came later than a refresh period after the one before it (µs).
    late_max_us: u64,
    /// The most pixels a frame drawn while it moved redrew.
    damage_px_max: u64,
    /// Its end was said.
    said: bool,
}

impl Run {
    const NONE: Self =
        Self { frames: 0, last_at: 0, late_max_us: 0, damage_px_max: 0, said: false };

    fn note(&mut self, at: u64, note: FrameNote) {
        self.frames = self.frames.saturating_add(1);
        if self.last_at != 0 && note.period_ns > 0 {
            let late = at.saturating_sub(self.last_at).saturating_sub(note.period_ns) / 1000;
            self.late_max_us = self.late_max_us.max(late);
        }
        self.last_at = at;
        self.damage_px_max = self.damage_px_max.max(note.damage_px);
    }
}

/// A motion that ended: the frames drawn while it moved, the refresh periods in its length
/// (`0`: not timed), the latest frame and the largest redraw among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Done {
    pub(crate) frames: u32,
    pub(crate) of: u64,
    pub(crate) late_max_us: u64,
    pub(crate) damage_px_max: u64,
}

/// What a frame did to a motion layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Said {
    Nothing,
    /// A segment started with this frame.
    Started,
    /// The segment ended before this frame (said once).
    Done(Done),
}

/// The frame shown at `at` and motion layer `m`: a flip waiting for it is turned at `at`; a
/// segment not drawn yet starts with it; a moving one counts it; one that ended is said once.
pub(crate) fn stamp(m: &MotionLayerDesc, s: &mut MotionState, at: u64, note: FrameNote) -> Said {
    if s.seg.start == 0 {
        if let Some((prev, old)) = s.flipped.take() {
            s.seg.b0 = backdrop_at(&old, &prev, at);
            s.seg.turn = turn_from(&old, &prev, at);
        }
        if !in_flight(m, &s.seg, at) {
            return Said::Nothing;
        }
        s.seg.start = at.max(1);
        s.run = Run::NONE;
        s.run.note(at, note);
        return Said::Started;
    }
    if s.run.said {
        return Said::Nothing;
    }
    if in_flight(m, &s.seg, at) {
        s.run.note(at, note);
        return Said::Nothing;
    }
    s.run.said = true;
    let of = if note.period_ns == 0 { 0 } else { length_ns(m).div_ceil(note.period_ns) };
    Said::Done(Done {
        frames: s.run.frames,
        of,
        late_max_us: s.run.late_max_us,
        damage_px_max: s.run.damage_px_max,
    })
}
