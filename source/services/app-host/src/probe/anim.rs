// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host `DslApp` **animation** subsystem — the host tick + paint
//! tail of the DSL `.animate`/`.transition`/`.effect` binding (Tier 2 of
//! docs/dev/ui/foundations/animation.md, ADR-0031). The DSL front-end stamps a
//! value-typed [`AnimIntent`] onto each animated node (`View::animations`); the
//! DSL stays PURE (principles.md §4) and THIS owns the wall clock, the physics
//! driver (`animation::AnimationDriver`, the OS-wide SSOT), and the per-node
//! transform the CPU painter (`nexus-scene-raster`) applies. Ticked on the
//! compositor Choreographer **frame pulse** — the same cadence scroll momentum
//! rides — so an animation is a bounded, app-driven repaint, never a busy loop.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//!
//! FLOW: a re-emit (mount / tap / relayout) refreshes the intents → [`anim_sync`]
//! diffs each intent's committed value against the last emit and (re)starts the
//! token's motion on a change (seeding the mounted resting state on first
//! sight) → the frame pulse calls [`anim_tick`], folding the driver's
//! `SceneUpdate`s into the per-node [`NodeAnim`] table → [`node_anims_snapshot`]
//! hands the painter the current transforms. Alloc-bounded: the driver, the
//! diff table, and the transform table are all capped at [`MAX_NODE_ANIMS`];
//! the per-frame tick is allocation-free (`tick_into` into a stack buffer).

use super::*;
use animation::{AnimProp, AnimationDriver, Easing, LayerId, MotionToken, SceneUpdate, SpringConfig};
use nexus_dsl_runtime::theme_tokens::Tokens;
use nexus_dsl_runtime::{AnimIntent, AnimKind};
use nexus_scene_raster::NodeAnim;

/// Max concurrent DSL node transforms (mirrors the engine's active-animation
/// cap). Bounds the driver, the value-diff table, and the paint transform
/// table so none grows on the non-freeing bump heap.
pub(super) const MAX_NODE_ANIMS: usize = 6;

/// SlideUp travel (px): the offset a slide-in transition starts from.
const SLIDE_PX: f32 = 16.0;
/// Wiggle travel (px): the ± attention swing of the `.effect(wiggle)` token.
const WIGGLE_PX: f32 = 6.0;
/// Pulse peak scale (fraction over 1.0) of the `.effect(pulse)` token.
const PULSE_PEAK: f32 = 0.12;
/// FadeScale's absent-state scale (grows to 1.0 on enter, per animation.md).
const FADE_SCALE_FROM: f32 = 0.92;
/// Continuous-loop (`AnimKind::Loop`) breathe opacity endpoints + midpoint —
/// an inherently-animated widget (Skeleton) pulses between these forever via a
/// spring that re-fires toward the opposite endpoint on each convergence
/// (alloc-free: `spring_to` allocates nothing, so the loop never grows the heap).
const BREATHE_BRIGHT: f32 = 1.0;
const BREATHE_DIM: f32 = 0.15;
const BREATHE_MID: f32 = 0.55;

/// The identity value of an animation property (opacity/scale rest at 1.0,
/// translate at 0.0) — the "no visible effect" anchor for interpolation.
fn prop_identity(prop: AnimProp) -> f32 {
    match prop {
        AnimProp::Opacity | AnimProp::ScaleX | AnimProp::ScaleY => 1.0,
        _ => 0.0,
    }
}

/// The property target for a driving value under a token: opacity/scale/
/// translate present (value != 0) vs absent (value == 0). A nonzero value is
/// "shown/in place"; zero is "hidden/offset" — the value-tracking contract of
/// `.animate` (a Bool `value:` is the canonical driver).
fn target_for(token: MotionToken, prop: AnimProp, value: i32) -> f32 {
    let present = value != 0;
    match prop {
        AnimProp::Opacity => {
            if present {
                1.0
            } else {
                0.0
            }
        }
        // SlideUp rests IN place (0) when present, offset BELOW when absent.
        AnimProp::TranslateY => {
            if present {
                0.0
            } else {
                SLIDE_PX
            }
        }
        AnimProp::ScaleX | AnimProp::ScaleY => {
            if matches!(token, MotionToken::FadeScale) && !present {
                FADE_SCALE_FROM
            } else {
                1.0
            }
        }
        _ => prop_identity(prop),
    }
}

/// Host-owned animation state for one `DslApp` surface. See the module header.
pub(super) struct AnimState {
    /// The OS-wide physics SSOT — springs/keyframes interpolated on the pulse.
    driver: AnimationDriver,
    /// Last committed driving value per animated node, diffed across re-emits
    /// so a state change (re)starts motion exactly once. Bounded by the cap.
    seen: alloc::vec::Vec<(usize, i32)>,
    /// The current interpolated transform per node the painter consumes;
    /// entries that converge back to identity are pruned. Bounded by the cap.
    anims: alloc::vec::Vec<NodeAnim>,
    /// Node ids running a CONTINUOUS breathe loop (`AnimKind::Loop`, an
    /// inherently-animated kit widget). The spring re-fires toward the opposite
    /// endpoint on each convergence; these keep the frame pulse armed while the
    /// widget is on screen. Bounded by the cap.
    loops: alloc::vec::Vec<usize>,
    /// Last physics tick (ns) for dt integration (diagnostics/telemetry).
    last_ns: u64,
}

impl AnimState {
    pub(super) fn new() -> Self {
        Self {
            driver: AnimationDriver::new(),
            seen: alloc::vec::Vec::new(),
            anims: alloc::vec::Vec::new(),
            loops: alloc::vec::Vec::new(),
            last_ns: 0,
        }
    }

    /// Token motion duration in ns for the active theme. A reduced-motion theme
    /// zeroes `motion_ms`; the keyframe track treats 0 as "jump to the final
    /// frame" (`KeyframeTrack::new` clamps to 1ns) — so reduced motion is honored
    /// for free.
    fn dur_ns(token: MotionToken, tokens: &dyn Tokens) -> u64 {
        (tokens.motion_ms(token.duration()) as u64).saturating_mul(1_000_000)
    }

    /// The node's current transform slot, inserting an identity slot on first
    /// touch (evicting the oldest when at the cap — deterministic, alloc-bounded).
    fn node_mut(&mut self, node_id: usize) -> &mut NodeAnim {
        if let Some(i) = self.anims.iter().position(|a| a.node_id == node_id) {
            return &mut self.anims[i];
        }
        if self.anims.len() >= MAX_NODE_ANIMS {
            self.anims.swap_remove(0);
        }
        self.anims.push(NodeAnim::identity(node_id));
        let last = self.anims.len() - 1;
        &mut self.anims[last]
    }

    /// The node's current value for `prop` (identity when it has no transform
    /// yet) — the `from` anchor a fresh interpolation starts at, so a re-trigger
    /// mid-flight continues smoothly instead of snapping.
    fn cur(&self, node_id: usize, prop: AnimProp) -> f32 {
        let Some(a) = self.anims.iter().find(|a| a.node_id == node_id) else {
            return prop_identity(prop);
        };
        match prop {
            AnimProp::Opacity => a.opacity as f32 / 255.0,
            AnimProp::TranslateX => a.dx as f32,
            AnimProp::TranslateY => a.dy as f32,
            AnimProp::ScaleX | AnimProp::ScaleY => a.scale_pct as f32 / 100.0,
            _ => prop_identity(prop),
        }
    }

    /// Write a property into the node's transform slot (the paint value) — used
    /// to SEED a mounted resting state and to fold a driver `SceneUpdate`.
    fn set_prop(&mut self, node_id: usize, prop: AnimProp, v: f32) {
        let a = self.node_mut(node_id);
        match prop {
            AnimProp::Opacity => a.opacity = (v.clamp(0.0, 1.0) * 255.0) as u8,
            AnimProp::TranslateX => a.dx = v as i32,
            AnimProp::TranslateY => a.dy = v as i32,
            AnimProp::ScaleX | AnimProp::ScaleY => a.scale_pct = (v.max(0.0) * 100.0) as u16,
            _ => {}
        }
    }

    /// Start a keyframe interpolation of `prop` from its current value to
    /// `to` over the token's themed duration + easing (the deterministic CPU
    /// track; springs seed the GPU path in Track C). A no-op when already there.
    fn start_prop(
        &mut self,
        node_id: usize,
        token: MotionToken,
        prop: AnimProp,
        to: f32,
        tokens: &dyn Tokens,
    ) {
        let from = self.cur(node_id, prop);
        if (from - to).abs() < 0.0001 {
            return;
        }
        self.driver.keyframe_to(
            LayerId(node_id as u64),
            prop,
            alloc::vec![(0.0, from), (1.0, to)],
            Self::dur_ns(token, tokens),
            token.easing(),
        );
    }

    /// Seed the absent state and animate the node IN — the `.transition` enter
    /// (fade/slide/fadeScale). Slide/scale tokens fade in too (a bare translate
    /// or scale reads as a pop; the cross-fade is the decided look).
    fn enter(&mut self, node_id: usize, token: MotionToken, tokens: &dyn Tokens) {
        let p = token.primary_prop();
        self.set_prop(node_id, p, target_for(token, p, 0));
        self.start_prop(node_id, token, p, target_for(token, p, 1), tokens);
        if let Some(sp) = token.secondary_prop() {
            self.set_prop(node_id, sp, target_for(token, sp, 0));
            self.start_prop(node_id, token, sp, target_for(token, sp, 1), tokens);
        }
        if p != AnimProp::Opacity {
            self.set_prop(node_id, AnimProp::Opacity, 0.0);
            self.start_prop(node_id, token, AnimProp::Opacity, 1.0, tokens);
        }
    }

    /// Fire a bounded attention effect (`.effect(wiggle|pulse)`): a keyframe
    /// oscillation that RETURNS to identity (so the entry is pruned on
    /// convergence). Linear between the explicit keyframes.
    fn start_effect(&mut self, node_id: usize, token: MotionToken, tokens: &dyn Tokens) {
        let layer = LayerId(node_id as u64);
        let dur = Self::dur_ns(token, tokens);
        match token {
            MotionToken::Wiggle => self.driver.keyframe_to(
                layer,
                AnimProp::TranslateX,
                alloc::vec![
                    (0.0, 0.0),
                    (0.2, WIGGLE_PX),
                    (0.4, -WIGGLE_PX),
                    (0.6, WIGGLE_PX * 0.5),
                    (0.8, -WIGGLE_PX * 0.3),
                    (1.0, 0.0),
                ],
                dur,
                Easing::Linear,
            ),
            MotionToken::Pulse => self.driver.keyframe_to(
                layer,
                AnimProp::ScaleX,
                alloc::vec![(0.0, 1.0), (0.5, 1.0 + PULSE_PEAK), (1.0, 1.0)],
                dur,
                Easing::Linear,
            ),
            // A value/transition token routed through `.effect` (the checker
            // steers authors to effect tokens): no bounded oscillation to play.
            _ => {}
        }
    }

    /// First sight of an animated node: seed its resting transform (no motion)
    /// for value/effect tokens, or play the enter for a `.transition`.
    fn seed(&mut self, node_id: usize, token: MotionToken, intent: AnimIntent, tokens: &dyn Tokens) {
        match intent.kind {
            AnimKind::Animate => {
                let p = token.primary_prop();
                self.set_prop(node_id, p, target_for(token, p, intent.value));
                if let Some(sp) = token.secondary_prop() {
                    self.set_prop(node_id, sp, target_for(token, sp, intent.value));
                }
            }
            AnimKind::Transition => self.enter(node_id, token, tokens),
            // Effects have no resting change — they fire on a trigger change.
            AnimKind::Effect => {}
            // Continuous loops are reconciled in `sync_loops`, never here.
            AnimKind::Loop => {}
        }
    }

    /// A node whose driving value CHANGED: (re)start the token's motion.
    fn restart(
        &mut self,
        node_id: usize,
        token: MotionToken,
        intent: AnimIntent,
        tokens: &dyn Tokens,
    ) {
        match intent.kind {
            AnimKind::Animate => {
                let p = token.primary_prop();
                self.start_prop(node_id, token, p, target_for(token, p, intent.value), tokens);
                if let Some(sp) = token.secondary_prop() {
                    self.start_prop(node_id, token, sp, target_for(token, sp, intent.value), tokens);
                }
            }
            AnimKind::Transition => self.enter(node_id, token, tokens),
            AnimKind::Effect => self.start_effect(node_id, token, tokens),
            // Continuous loops are reconciled in `sync_loops`, never here.
            AnimKind::Loop => {}
        }
    }

    /// Record/refresh the last committed value for a node (the change detector).
    fn upsert_seen(&mut self, node_id: usize, value: i32) {
        if let Some(e) = self.seen.iter_mut().find(|(id, _)| *id == node_id) {
            e.1 = value;
            return;
        }
        if self.seen.len() >= MAX_NODE_ANIMS {
            self.seen.swap_remove(0);
        }
        self.seen.push((node_id, value));
    }

    /// Drop a node's transform slot once it has converged back to identity
    /// (keeps the paint slice tiny; a node resting at a NON-identity value —
    /// e.g. faded out — is kept so it stays hidden).
    fn prune_identity(&mut self, node_id: usize) {
        if let Some(i) = self.anims.iter().position(|a| a.node_id == node_id) {
            if self.anims[i].is_identity() {
                self.anims.swap_remove(i);
            }
        }
    }

    /// A slow, smooth (over-damped) breathe spring — a calm ~1s approach with
    /// no overshoot, so the loop reads as a gentle pulse, not a bounce.
    fn breathe_config() -> SpringConfig {
        SpringConfig { stiffness: 18.0, damping: 9.0, mass: 1.0, initial_velocity: 0.0 }
    }

    /// (Re)start a breathe half-cycle for `node_id`: spring the opacity from its
    /// current value toward the OPPOSITE endpoint (dim when currently bright,
    /// bright when currently dim). Alloc-free (`spring_to` carries no Vec).
    fn start_breathe(&mut self, node_id: usize) {
        let cur = self.cur(node_id, AnimProp::Opacity);
        let target = if cur > BREATHE_MID { BREATHE_DIM } else { BREATHE_BRIGHT };
        self.driver.spring_to(
            LayerId(node_id as u64),
            AnimProp::Opacity,
            cur,
            target,
            Self::breathe_config(),
        );
    }

    /// Reconcile the continuous-loop set with the freshly-emitted `Loop`
    /// intents: forget nodes gone from the tree (cancel + restore to full
    /// opacity) and start a breathe for each newly-seen looping widget.
    fn sync_loops(&mut self, present: &[(usize, AnimIntent)]) {
        let is_present_loop = |n: usize| {
            present.iter().any(|(pn, pi)| *pn == n && pi.kind == AnimKind::Loop)
        };
        // Drop gone loops: cancel the spring, reset to bright, prune the slot.
        let mut i = 0;
        while i < self.loops.len() {
            let n = self.loops[i];
            if is_present_loop(n) {
                i += 1;
            } else {
                self.driver.cancel(LayerId(n as u64), AnimProp::Opacity);
                self.set_prop(n, AnimProp::Opacity, BREATHE_BRIGHT);
                self.prune_identity(n);
                self.loops.swap_remove(i);
            }
        }
        // Start newly-seen loops.
        for &(node_id, intent) in present {
            if intent.kind != AnimKind::Loop || self.loops.contains(&node_id) {
                continue;
            }
            if self.loops.len() >= MAX_NODE_ANIMS {
                break;
            }
            self.loops.push(node_id);
            self.start_breathe(node_id);
        }
    }

    /// Keep every breathe loop alive: a node whose opacity spring has converged
    /// re-fires toward the opposite endpoint. Called each frame pulse after the
    /// driver tick — the driver is never idle while a loop exists, so the pulse
    /// stays armed. Alloc-free.
    fn tick_loops(&mut self) {
        for idx in 0..self.loops.len() {
            let node_id = self.loops[idx];
            if !self.driver.is_active(LayerId(node_id as u64), AnimProp::Opacity) {
                self.start_breathe(node_id);
            }
        }
    }
}

impl super::DslApp {
    /// Reconcile the animation driver with the freshly-emitted scene: seed a
    /// newly-seen node's resting transform, (re)start motion where the driving
    /// value changed, and forget nodes gone from the tree. Called after every
    /// re-emit (mount / tap / relayout). The caller then asks the compositor
    /// for a frame pulse when [`anim_active`](Self::anim_active) is true.
    pub(super) fn anim_sync(&mut self) {
        // Snapshot the intents (Copy) so the driver can be borrowed mutably
        // without holding the `view.animations()` borrow across the loop.
        let mut buf = [(0usize, AnimIntent::new(AnimKind::Animate, 0, 0)); MAX_NODE_ANIMS];
        let mut n = 0;
        for &(node_id, intent) in self.view.animations() {
            if n >= buf.len() {
                break;
            }
            buf[n] = (node_id, intent);
            n += 1;
        }
        let present = &buf[..n];
        // Durations resolve against the pushed theme (reduced-motion = 0ms).
        let tokens = tokens_for(self.theme_mode);
        // Seed the driver clock BEFORE arming an idle driver: the first tick
        // otherwise measures `now − stale last_tick` = the whole idle gap and a
        // keyframe jumps straight to its end (an instant pop, no fade). Only on
        // the idle→active edge — a mid-flight driver already tracks dt.
        let was_idle = self.anim.driver.active_count() == 0;
        for &(node_id, intent) in present {
            // Continuous kit-widget loops are reconciled below (not value-diffed).
            if intent.kind == AnimKind::Loop {
                continue;
            }
            let Some(token) = MotionToken::from_id(intent.token) else {
                continue;
            };
            let prev = self.anim.seen.iter().find(|(id, _)| *id == node_id).map(|(_, v)| *v);
            match prev {
                None => self.anim.seed(node_id, token, intent, tokens),
                Some(pv) if pv != intent.value => self.anim.restart(node_id, token, intent, tokens),
                _ => {}
            }
            self.anim.upsert_seen(node_id, intent.value);
        }
        // Nodes removed from the tree: forget them (no exit motion in the
        // immediate-mode model — an exit needs the node kept alive, Track C).
        self.anim.seen.retain(|(id, _)| {
            present.iter().any(|(p, i)| p == id && i.kind != AnimKind::Loop)
        });
        self.anim.anims.retain(|a| present.iter().any(|(p, _)| *p == a.node_id));
        // Continuous loops (inherently-animated widgets): start new, drop gone.
        self.anim.sync_loops(present);
        // Arm the clock on the idle→active edge (see `was_idle` above).
        if was_idle && self.anim.driver.active_count() > 0 {
            self.anim.driver.reset_clock(nsec_now());
        }
    }

    /// Advance the animation physics by real elapsed time on the frame pulse and
    /// fold the driver's `SceneUpdate`s into the per-node transform table.
    /// Returns the union ROW SPAN the changes damage (old ∪ new transformed
    /// AABB per touched node, ±1 row rounding pad) — `None` = nothing moved.
    /// The caller repaints EXACTLY that span (the 120Hz damage contract: a
    /// 16px skeleton breathe must never trigger a full-surface repaint +
    /// full-band re-blit + full recomposite). Allocation-free.
    pub(super) fn anim_tick(&mut self) -> Option<(i32, i32)> {
        if self.anim.driver.active_count() == 0 && self.anim.loops.is_empty() {
            return None;
        }
        let now = nsec_now();
        let mut updates = [SceneUpdate::default(); MAX_NODE_ANIMS * 2];
        let count = self.anim.driver.tick_into(now, &mut updates);
        self.anim.last_ns = now;
        let mut span: Option<(i32, i32)> = None;
        for u in &updates[..count] {
            let node_id = u.layer_id.0 as usize;
            // Damage rows BEFORE the fold (the node's currently-painted rect).
            self.grow_anim_span(node_id, &mut span);
            self.anim.set_prop(node_id, u.property, u.value);
            // Converged (progress==1) back to identity → drop the slot.
            if u.progress >= 1.0 {
                self.anim.prune_identity(node_id);
            }
            // …and AFTER (where it paints now) — the union covers the motion.
            self.grow_anim_span(node_id, &mut span);
        }
        // Re-fire any breathe loop whose spring has converged, so it never stops.
        self.anim.tick_loops();
        span
    }

    /// Grow `span` by `node_id`'s CURRENT painted row extent: its layout box
    /// transformed by the node's active [`NodeAnim`] (scale can overpaint the
    /// box; translate moves it), ±1 row rounding pad, surface-clamped. A node
    /// without a layout box (mid-relayout) falls back to the full surface —
    /// correctness over thrift on the rare path.
    fn grow_anim_span(&self, node_id: usize, span: &mut Option<(i32, i32)>) {
        let (y0, y1) = match self.layout.boxes.iter().find(|b| b.node_id == node_id) {
            Some(b) => {
                let (x, y, w, h) =
                    (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
                match self.anim.anims.iter().find(|a| a.node_id == node_id) {
                    Some(a) => {
                        let (_, ny, _, nh) = a.transform_rect(x, y, w, h);
                        (ny - 1, ny + nh + 1)
                    }
                    None => (y - 1, y + h + 1),
                }
            }
            None => (0, self.h as i32),
        };
        let (y0, y1) = (y0.max(0), y1.min(self.h as i32));
        if y0 >= y1 {
            return;
        }
        *span = Some(match *span {
            Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
            None => (y0, y1),
        });
    }

    /// Whether any DSL animation is still interpolating OR a continuous widget
    /// loop is live — the FRAME-arm re-arms the compositor pulse while true
    /// (windowd parks the request for hidden windows, so a continuous loop
    /// costs nothing off-screen).
    pub(super) fn anim_active(&self) -> bool {
        self.anim.driver.active_count() > 0 || !self.anim.loops.is_empty()
    }

    /// Whether a BOUNDED (non-loop) animation is interpolating — the only
    /// animation state that may arm the recv-timeout SELF-PACE fallback. A
    /// continuous loop must ride the compositor frame pulse EXCLUSIVELY
    /// (the compositor owns pacing + visibility; a self-paced loop kept
    /// rendering at ~80Hz forever, hidden windows included).
    pub(super) fn anim_transient_active(&self) -> bool {
        let loop_springs = self
            .anim
            .loops
            .iter()
            .filter(|&&n| self.anim.driver.is_active(LayerId(n as u64), AnimProp::Opacity))
            .count();
        self.anim.driver.active_count() > loop_springs
    }

    /// Copy the current per-node transforms into `out` for the painter; returns
    /// the count written. A copy (not a borrow) so the caller can hold its
    /// render scratch mutably at the same time. Bounded by [`MAX_NODE_ANIMS`].
    pub(super) fn node_anims_snapshot(&self, out: &mut [NodeAnim]) -> usize {
        let n = self.anim.anims.len().min(out.len());
        out[..n].copy_from_slice(&self.anim.anims[..n]);
        n
    }
}
