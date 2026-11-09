// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A control's interaction LAYERS (RFC-0116 — the design artifacts' hover and press
//! layer): the hover layer (`fillHover`) and the press layer (`fillActive`) — over a coloured fill
//! the lightening pair instead (white 14 % hover, black 12 % press) — shown by opacity alone,
//! never a repaint of the control. Each layer is a WASH the compositor draws (RFC-0109): a patch
//! of the store spread over the control at an opacity. The app states only the opacity, frame by
//! frame while a layer fades (hover over `swift`, press over `instant`, on the token set's fade
//! curve). Pure — the theme's colours on the patches, the fades, what a motion frame carries — so
//! host tests reach it; `probe/` drives it from the pointer and the frame pulse.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below; the wash's wire bounds in
//!   `nexus-display-proto::surface_layers`.

use animation::Easing;
use nexus_display_proto::surface_layers::{MAX_PATCHES, MAX_WASHES};
use nexus_layout_types::Rgba8;
use nexus_theme_tokens::{ColorToken, MotionCurveToken, MotionDurationToken, Tokens};

/// The hover layer over a coloured fill (the design artifacts' on-fill layer): white at 14 %.
pub(crate) const ON_FILL_HOVER: Rgba8 = Rgba8::new(255, 255, 255, 36);
/// The press layer over a coloured fill: black at 12 %.
pub(crate) const ON_FILL_PRESS: Rgba8 = Rgba8::new(0, 0, 0, 31);
/// The store patches the layers spread: the structure's first and last — the ones between are
/// the motion layers' backdrops (`probe/motion_layers.rs`).
pub(crate) const WIRE_PATCHES: [u8; 2] = [0, MAX_PATCHES as u8 - 1];
/// Every layer fades on the token set's fade curve.
const FADE: Easing = Easing::Curve(MotionCurveToken::Smooth);

/// Which layer of a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Hover,
    Press,
}

impl Kind {
    /// The motion token the layer fades over: hover `swift`, press `instant`.
    pub(crate) fn duration(self) -> MotionDurationToken {
        match self {
            Kind::Hover => MotionDurationToken::Swift,
            Kind::Press => MotionDurationToken::Instant,
        }
    }
}

/// A layer as the compositor draws it: store patch `patch` (opaque) at `opacity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shade {
    pub(crate) patch: u8,
    pub(crate) opacity: u8,
}

/// The theme's layer colours on the patches: a colour is the patch's, its alpha the opacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Palette {
    /// What patch `WIRE_PATCHES[i]` holds (its alpha unused: the patch is opaque).
    colors: [Rgba8; 2],
    hover: Shade,
    press: Shade,
    on_fill_hover: Shade,
    on_fill_press: Shade,
}

impl Palette {
    /// The palette of a theme.
    pub(crate) fn of(tokens: &dyn Tokens) -> Self {
        Self::from_colors(tokens.color(ColorToken::FillHover), tokens.color(ColorToken::FillActive))
    }

    /// The palette of a theme whose layers are `hover` and `press`: colours of one hue share a
    /// patch (their alphas differ, the opacity carries them) — two patches in every theme the
    /// repo ships; a third colour takes the patch nearest it.
    pub(crate) fn from_colors(hover: Rgba8, press: Rgba8) -> Self {
        let mut colors = [hover; 2];
        let mut used = 1;
        let mut place = |c: Rgba8| {
            let rgb = |p: &Rgba8| (p.r, p.g, p.b);
            let i = match colors[..used].iter().position(|p| rgb(p) == rgb(&c)) {
                Some(i) => i,
                None if used < colors.len() => {
                    colors[used] = c;
                    used += 1;
                    used - 1
                }
                None => nearest(&colors[..used], c),
            };
            Shade { patch: WIRE_PATCHES[i], opacity: c.a }
        };
        let (hover, press) = (place(hover), place(press));
        let (on_fill_hover, on_fill_press) = (place(ON_FILL_HOVER), place(ON_FILL_PRESS));
        Self { colors, hover, press, on_fill_hover, on_fill_press }
    }

    /// The `kind` layer over a control — over a coloured fill or not.
    pub(crate) fn shade(&self, kind: Kind, on_fill: bool) -> Shade {
        match (kind, on_fill) {
            (Kind::Hover, false) => self.hover,
            (Kind::Press, false) => self.press,
            (Kind::Hover, true) => self.on_fill_hover,
            (Kind::Press, true) => self.on_fill_press,
        }
    }

    /// Each patch the layers spread and the colour it holds.
    pub(crate) fn patches(&self) -> [(u8, Rgba8); 2] {
        [(WIRE_PATCHES[0], self.colors[0]), (WIRE_PATCHES[1], self.colors[1])]
    }
}

/// The index of the colour in `colors` nearest `c` (squared RGB distance, the first on a tie).
fn nearest(colors: &[Rgba8], c: Rgba8) -> usize {
    let d = |p: &Rgba8| {
        let (r, g, b) = (
            i32::from(p.r) - i32::from(c.r),
            i32::from(p.g) - i32::from(c.g),
            i32::from(p.b) - i32::from(c.b),
        );
        r * r + g * g + b * b
    };
    let mut best = 0;
    for (i, p) in colors.iter().enumerate() {
        if d(p) < d(&colors[best]) {
            best = i;
        }
    }
    best
}

/// One layer: over the control `anchor` (its node), fading `from` → `to` over
/// `[start, start + dur)` in `shade`'s patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layer {
    anchor: usize,
    kind: Kind,
    shade: Shade,
    from: u8,
    to: u8,
    start: u64,
    dur: u64,
    /// The opacity the last tick found.
    last: u8,
}

impl Layer {
    /// Its opacity at `now`.
    fn opacity(&self, now: u64) -> u8 {
        if now >= self.end() {
            return self.to;
        }
        let t = now.saturating_sub(self.start) as f32 / self.dur as f32;
        let (from, to) = (f32::from(self.from), f32::from(self.to));
        let v = from + (to - from) * FADE.ease(t);
        // Round half up (`f32::round` is not in core).
        (v + 0.5).clamp(0.0, 255.0) as u8
    }

    fn end(&self) -> u64 {
        self.start.saturating_add(self.dur)
    }

    /// Fades toward `to` from where it is at `now`, over the share of `dur` the way left takes
    /// (a hover that comes back mid-fade turns around without a jump).
    fn retarget(&mut self, to: u8, now: u64, dur: u64) {
        let cur = self.opacity(now);
        let full = u64::from(self.shade.opacity.max(to).max(1));
        (self.from, self.to, self.start) = (cur, to, now);
        self.dur = dur.saturating_mul(u64::from(cur.abs_diff(to))) / full;
    }
}

/// The layers on a surface: one wash each, at most the wire's bound.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Layers {
    slots: [Option<Layer>; MAX_WASHES],
    /// The time the layers are shown at: the last frame pulse's, or the last change's.
    at: u64,
}

impl Layers {
    pub(crate) const EMPTY: Self = Self { slots: [None; MAX_WASHES], at: 0 };

    /// The `kind` layer goes to `target` — `(anchor, shade)` — fading in from where it is, and
    /// every other `kind` layer fades out; `None`: the `kind` layer goes. `dur`: the kind's fade
    /// (ns; 0 = at once).
    pub(crate) fn show(&mut self, kind: Kind, target: Option<(usize, Shade)>, now: u64, dur: u64) {
        self.at = self.at.max(now);
        let now = self.at;
        let mut kept = false;
        for l in self.slots.iter_mut().flatten().filter(|l| l.kind == kind) {
            match target {
                Some((anchor, shade)) if anchor == l.anchor => {
                    l.shade = shade;
                    l.retarget(shade.opacity, now, dur);
                    kept = true;
                }
                _ if l.to > 0 => l.retarget(0, now, dur),
                _ => {}
            }
        }
        let Some((anchor, shade)) = target.filter(|_| !kept) else { return };
        let slot = self.free_slot(now);
        self.slots[slot] = Some(Layer {
            anchor,
            kind,
            shade,
            from: 0,
            to: shade.opacity,
            start: now,
            dur,
            last: 0,
        });
    }

    /// A slot for a new layer: a free one, else the faintest layer on its way out (there is one
    /// whenever all are taken: at most one layer of each kind is on its way in).
    fn free_slot(&self, now: u64) -> usize {
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            return i;
        }
        let mut best = 0;
        let mut faintest = u16::MAX;
        for (i, l) in self.slots.iter().enumerate() {
            let Some(l) = l else { continue };
            let v = if l.to == 0 { u16::from(l.opacity(now)) } else { u16::MAX - 1 };
            if v < faintest {
                (best, faintest) = (i, v);
            }
        }
        best
    }

    /// Every layer gone at once (the scene's nodes were laid out anew).
    pub(crate) fn clear(&mut self) {
        self.slots = [None; MAX_WASHES];
    }

    /// The layers at `now` (the pulse's shown time): a layer that faded out goes. Returns whether
    /// what a motion frame carries changed since the last tick.
    pub(crate) fn tick(&mut self, now: u64) -> bool {
        self.at = self.at.max(now);
        let at = self.at;
        let mut changed = false;
        for slot in &mut self.slots {
            let Some(l) = slot.as_mut() else { continue };
            let v = l.opacity(at);
            changed |= v != l.last;
            l.last = v;
            if v == 0 && at >= l.end() {
                *slot = None;
            }
        }
        changed
    }

    /// Whether a layer is still fading (the frame pulse stays armed).
    pub(crate) fn live(&self) -> bool {
        self.slots.iter().flatten().any(|l| self.at < l.end())
    }

    /// What a motion frame carries now — `(anchor, shade at its opacity)`, the hover layers
    /// below the press layers (the design's press layer lies over its hover layer) — and how
    /// many. A layer at opacity 0 shows nothing and is left out.
    pub(crate) fn shown(&self) -> ([(usize, Shade); MAX_WASHES], usize) {
        let mut out = [(0, Shade { patch: 0, opacity: 0 }); MAX_WASHES];
        let mut n = 0;
        for kind in [Kind::Hover, Kind::Press] {
            for l in self.slots.iter().flatten().filter(|l| l.kind == kind) {
                let opacity = l.opacity(self.at);
                if opacity > 0 {
                    out[n] = (l.anchor, Shade { patch: l.shade.patch, opacity });
                    n += 1;
                }
            }
        }
        (out, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens, HighContrastTokens, LightTokens};

    const MS: u64 = 1_000_000;

    fn shade(opacity: u8) -> Shade {
        Shade { patch: 0, opacity }
    }

    fn shown(l: &Layers) -> Vec<(usize, u8)> {
        let (out, n) = l.shown();
        out[..n].iter().map(|(a, s)| (*a, s.opacity)).collect()
    }

    /// The light theme: `fillHover` and `fillActive` are one black at 6 / 12 %, so both share
    /// the first patch; the on-fill white takes the second, its black press the first.
    #[test]
    fn the_light_palette_shares_the_black_patch() {
        for tokens in [&LightTokens as &dyn Tokens, &BaseTokens] {
            let p = Palette::of(tokens);
            assert_eq!(p.shade(Kind::Hover, false), Shade { patch: 0, opacity: 15 });
            assert_eq!(p.shade(Kind::Press, false), Shade { patch: 0, opacity: 31 });
            assert_eq!(p.shade(Kind::Hover, true), Shade { patch: 3, opacity: 36 });
            assert_eq!(p.shade(Kind::Press, true), Shade { patch: 0, opacity: 31 });
            let [(p0, c0), (p1, c1)] = p.patches();
            assert_eq!((p0, (c0.r, c0.g, c0.b)), (0, (0, 0, 0)));
            assert_eq!((p1, (c1.r, c1.g, c1.b)), (3, (255, 255, 255)));
        }
    }

    /// The dark theme mirrors it: white layers (10 / 18 %), the on-fill press is the black one.
    #[test]
    fn the_dark_palette_shares_the_white_patch() {
        let p = Palette::of(&DarkTokens);
        assert_eq!(p.shade(Kind::Hover, false), Shade { patch: 0, opacity: 26 });
        assert_eq!(p.shade(Kind::Press, false), Shade { patch: 0, opacity: 46 });
        assert_eq!(p.shade(Kind::Hover, true), Shade { patch: 0, opacity: 36 });
        assert_eq!(p.shade(Kind::Press, true), Shade { patch: 3, opacity: 31 });
        let hc = Palette::of(&HighContrastTokens);
        assert_eq!(hc.shade(Kind::Press, true).patch, 3);
    }

    /// A theme whose two layer colours differ in hue fills both patches; the on-fill colours
    /// take the nearest — never a third patch the structure does not have.
    #[test]
    fn a_third_colour_takes_the_nearest_patch() {
        let p = Palette::from_colors(Rgba8::new(250, 250, 250, 20), Rgba8::new(10, 10, 10, 40));
        assert_eq!(p.shade(Kind::Hover, false).patch, 0);
        assert_eq!(p.shade(Kind::Press, false).patch, 3);
        assert_eq!(p.shade(Kind::Hover, true), Shade { patch: 0, opacity: 36 });
        assert_eq!(p.shade(Kind::Press, true), Shade { patch: 3, opacity: 31 });
    }

    /// The fades are the motion tokens' durations: hover 160 ms, press 100 ms.
    #[test]
    fn hover_fades_over_swift_and_press_over_instant() {
        assert_eq!(BaseTokens.motion_ms(Kind::Hover.duration()), 160);
        assert_eq!(BaseTokens.motion_ms(Kind::Press.duration()), 100);
    }

    /// Hover fades IN — never on at once — and is at its full opacity when the fade ends.
    #[test]
    fn a_hover_fades_in_by_opacity_alone() {
        let mut l = Layers::EMPTY;
        l.show(Kind::Hover, Some((7, shade(15))), 1_000 * MS, 160 * MS);
        assert!(shown(&l).is_empty(), "the first frame shows nothing yet");
        assert!(l.live());
        assert!(l.tick(1_080 * MS));
        let mid = shown(&l)[0].1;
        assert!(mid > 0 && mid < 15, "half way it is part way: {mid}");
        assert!(l.tick(1_160 * MS));
        assert_eq!(shown(&l), [(7, 15)]);
        assert!(!l.live(), "at rest nothing keeps the pulse armed");
        assert!(!l.tick(1_200 * MS), "a resting layer owes no frame");
    }

    /// The pointer moves on: the old control's layer fades out while the new one's fades in,
    /// and the old one is gone once it reached zero.
    #[test]
    fn moving_on_fades_the_old_out_and_the_new_in() {
        let mut l = Layers::EMPTY;
        l.show(Kind::Hover, Some((1, shade(15))), 0, 160 * MS);
        l.tick(160 * MS);
        l.show(Kind::Hover, Some((2, shade(15))), 160 * MS, 160 * MS);
        l.tick(240 * MS);
        let both = shown(&l);
        assert_eq!(both.len(), 2, "{both:?}");
        assert!(both.iter().all(|(_, o)| *o > 0 && *o < 15));
        l.tick(320 * MS);
        assert_eq!(shown(&l), [(2, 15)]);
        l.show(Kind::Hover, None, 400 * MS, 160 * MS);
        l.tick(560 * MS);
        assert!(shown(&l).is_empty());
        assert!(!l.live());
    }

    /// Back onto a control whose layer is on its way out: it turns around from where it is —
    /// no jump to zero, no jump to full.
    #[test]
    fn coming_back_turns_the_fade_around() {
        let mut l = Layers::EMPTY;
        l.show(Kind::Hover, Some((1, shade(26))), 0, 160 * MS);
        l.tick(160 * MS);
        l.show(Kind::Hover, None, 160 * MS, 160 * MS);
        l.tick(220 * MS);
        let going = shown(&l)[0].1;
        l.show(Kind::Hover, Some((1, shade(26))), 220 * MS, 160 * MS);
        assert_eq!(shown(&l), [(1, going)], "the turn starts where the fade was");
        l.tick(400 * MS);
        assert_eq!(shown(&l), [(1, 26)]);
    }

    /// The press layer lies over the hover layer of the same control (drawn after it).
    #[test]
    fn the_press_layer_is_drawn_over_the_hover_layer() {
        let mut l = Layers::EMPTY;
        l.show(Kind::Press, Some((4, shade(31))), 0, 100 * MS);
        l.show(Kind::Hover, Some((4, shade(15))), 0, 160 * MS);
        l.tick(200 * MS);
        assert_eq!(shown(&l), [(4, 15), (4, 31)]);
        l.show(Kind::Press, None, 200 * MS, 100 * MS);
        l.tick(300 * MS);
        assert_eq!(shown(&l), [(4, 15)], "the release lets the press layer go, hover stays");
    }

    /// A sweep across many controls never needs more washes than the wire carries: the
    /// faintest layer on its way out gives its slot up.
    #[test]
    fn a_full_set_gives_up_the_faintest_going_layer() {
        let mut l = Layers::EMPTY;
        for (k, t) in [(1, 0), (2, 20), (3, 40), (4, 60), (5, 80)] {
            l.show(Kind::Hover, Some((k, shade(15))), t * MS, 160 * MS);
        }
        let (_, n) = l.shown();
        assert!(n <= MAX_WASHES);
        let anchors: Vec<usize> = l.slots.iter().flatten().map(|s| s.anchor).collect();
        assert!(anchors.contains(&5), "the new layer always gets a slot");
        assert!(!anchors.contains(&1), "the faintest going layer (the first) gave way");
    }

    /// Reduced motion zeroes the durations: a layer is at its target at once.
    #[test]
    fn a_zero_duration_is_at_once() {
        let mut l = Layers::EMPTY;
        l.show(Kind::Press, Some((9, shade(46))), 50, 0);
        assert_eq!(shown(&l), [(9, 46)]);
        assert!(!l.live());
        l.clear();
        assert!(shown(&l).is_empty());
    }
}
