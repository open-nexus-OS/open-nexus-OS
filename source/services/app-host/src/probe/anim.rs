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
use nexus_dsl_runtime::{
    AnimIntent, AnimKind, LOOP_CAROUSEL, LOOP_CAROUSEL_SPOKES, LOOP_SWEEP,
};
use nexus_scene_raster::NodeAnim;

/// Max concurrent DSL node transforms (mirrors the engine's active-animation
/// cap). Bounds the driver, the value-diff table, and the paint transform
/// table so none grows on the non-freeing bump heap. Sized for the modifier
/// anims + widget loops (a Spinner carousel alone holds 12 spoke entries) +
/// the INTERACTION motions (hover-grow/press-bounce can touch several nodes
/// while sweeping the pointer).
pub(super) const MAX_NODE_ANIMS: usize = 32;
/// Expanded (subtree-cascaded) paint transform cap — each animated container
/// may fan out to its contained boxes (tile + glyph + label…).
pub(super) const MAX_EXPANDED_ANIMS: usize = 48;

/// SlideUp travel (px): the offset a slide-in transition starts from.
const SLIDE_PX: f32 = 16.0;
/// Wiggle travel (px): the ± attention swing of the `.effect(wiggle)` token.
const WIGGLE_PX: f32 = 6.0;
/// Pulse peak scale (fraction over 1.0) of the `.effect(pulse)` token.
const PULSE_PEAK: f32 = 0.12;
/// FadeScale's absent-state scale (grows to 1.0 on enter, per animation.md).
const FADE_SCALE_FROM: f32 = 0.92;
/// Interaction motion (design handoff "Animations & Motion"): hover grows the
/// control, press dips then springs back — swift, immediate, slightly elastic
/// (the `--motion-spring-soft` / `--motion-spring-icon` feel).
/// Hover-grow target scale ("Icons: scale(1.08) hover" — 1.06 generic reads
/// right on buttons too).
const HOVER_SCALE: f32 = 1.06;
/// Press dip ("instant down scale(0.9–0.95)").
const PRESS_SCALE: f32 = 0.92;
/// Press overshoot on the springy release (elastic pop past identity).
const PRESS_POP: f32 = 1.04;
/// Press pulse duration (down 0.1s + springy release).
const PRESS_MS: u64 = 280;
/// Hover spring: swift with subtle overshoot (`--motion-spring-soft`).
const HOVER_SPRING: animation::SpringConfig = animation::SpringConfig {
    stiffness: 420.0,
    damping: 22.0,
    mass: 1.0,
    initial_velocity: 0.0,
};
/// Toggle-thumb press: peak stretch along the travel axis (the handoff
/// "toggles stretch the thumb while pressed" — capsule, Y pinned).
const TOGGLE_STRETCH: f32 = 1.35;
/// Interaction motion applies to CONTROL-sized elements only: container
/// catch-all handlers (overlay backdrop = full screen, panel-body tap
/// consumers) must never hover-grow/press-dip — a 1.06 scale on a 328-wide
/// panel visibly displaces content from its (unscaled) hit boxes.
const INTERACTION_MAX_DIM: i32 = 160;

/// Continuous-loop (`AnimKind::Loop`) breathe opacity endpoints + midpoint —
/// an inherently-animated widget (Skeleton) pulses between these forever via a
/// spring that re-fires toward the opposite endpoint on each convergence
/// (alloc-free: `spring_to` allocates nothing, so the loop never grows the heap).
const BREATHE_BRIGHT: f32 = 1.0;
const BREATHE_DIM: f32 = 0.15;
const BREATHE_MID: f32 = 0.55;

/// Sweep loop spring (`LOOP_SWEEP`: shimmer band / indeterminate pip): an
/// overdamped ~1s glide across the track; the sawtooth RESETS to 0 on
/// convergence and re-fires. Springs only — a `keyframe_to` per cycle would
/// alloc a waypoint Vec on the non-freeing bump heap forever.
const SWEEP_SPRING: SpringConfig =
    SpringConfig { stiffness: 16.0, damping: 9.0, mass: 1.0, initial_velocity: 0.0 };
/// Carousel step interval (`LOOP_CAROUSEL`): ~80ms per spoke — one revolution
/// ≈ 1s over the 12 spokes, the classic activity-ring cadence.
const SPINNER_STEP_NS: u64 = 80_000_000;
/// Trailing-spoke minimum alpha (mirrors the Spinner builder's `TAIL_ALPHA`).
const SPINNER_TAIL_ALPHA: u16 = 64;

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

/// One continuous widget loop (reconciled against the emitted Loop intents).
#[derive(Clone, Copy)]
struct LoopEnt {
    /// The widget ROOT node the intent targets.
    node_id: usize,
    /// Loop sub-kind (`nexus_dsl_runtime::LOOP_*`, stamped in the intent value).
    sub: i32,
    /// Carousel: current leading-spoke step on the time grid.
    step: u8,
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
    /// CONTINUOUS widget loops (`AnimKind::Loop`, inherently-animated kit
    /// widgets), keyed by root node + sub-kind. Sweep springs re-fire on each
    /// convergence; the carousel steps on a time grid. These keep the frame
    /// pulse armed while the widget is on screen. Bounded by the cap.
    loops: alloc::vec::Vec<LoopEnt>,
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
            AnimProp::ScaleX => a.scale_pct as f32 / 100.0,
            // ScaleY mirrors X unless a non-uniform interaction split it
            // (toggle-thumb stretch) — the NodeAnim superset contract.
            AnimProp::ScaleY => a.scale_y_pct.unwrap_or(a.scale_pct) as f32 / 100.0,
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
            // ScaleX stays the uniform scale every existing consumer animates
            // (hover/press/pulse); an explicit ScaleY SPLITS the axes (the
            // toggle-thumb stretch). While split, ScaleX only moves X.
            AnimProp::ScaleX => a.scale_pct = (v.max(0.0) * 100.0) as u16,
            AnimProp::ScaleY => a.scale_y_pct = Some((v.max(0.0) * 100.0) as u16),
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

    /// Sweep travel distance (px) for a loop root: parent width − band/pip
    /// child width, from the CURRENT layout boxes (layout-fresh each cycle).
    fn sweep_travel(node_id: usize, boxes: &[nexus_layout::LayoutBox]) -> f32 {
        let w_of = |id: usize| {
            boxes.iter().find(|b| b.node_id == id).map_or(0, |b| b.rect.width.0)
        };
        (w_of(node_id) - w_of(node_id + 1)).max(0) as f32
    }

    /// (Re)start a sweep half…full cycle: the band/pip child springs from the
    /// track's left edge to the right end of its travel. Alloc-free.
    fn start_sweep(&mut self, node_id: usize, boxes: &[nexus_layout::LayoutBox]) {
        let travel = Self::sweep_travel(node_id, boxes);
        if travel < 1.0 {
            return; // no room (or boxes mid-relayout) — retried next tick
        }
        let child = node_id + 1;
        self.set_prop(child, AnimProp::TranslateX, 0.0);
        self.driver.spring_to(
            LayerId(child as u64),
            AnimProp::TranslateX,
            0.0,
            travel,
            SWEEP_SPRING,
        );
    }

    /// Write the carousel's stepped spoke opacities: the leading spoke is
    /// opaque, trailing spokes fade linearly to the tail alpha (mirrors the
    /// Spinner builder's resting fade — stepping the lead rotates it).
    fn write_carousel(&mut self, node_id: usize, step: u8) {
        let n = LOOP_CAROUSEL_SPOKES;
        for i in 0..n {
            let d = (i + n - step as usize) % n;
            let a = (255u16 - (255 - SPINNER_TAIL_ALPHA) * d as u16 / (n as u16 - 1)) as u8;
            self.node_mut(node_id + 1 + i).opacity = a;
        }
    }

    /// Forget one loop's paint state (the widget left the tree): cancel its
    /// springs and reset the touched nodes to identity so nothing stays faded.
    fn forget_loop(&mut self, ent: LoopEnt) {
        match ent.sub {
            LOOP_SWEEP => {
                let child = ent.node_id + 1;
                self.driver.cancel(LayerId(child as u64), AnimProp::TranslateX);
                self.set_prop(child, AnimProp::TranslateX, 0.0);
                self.prune_identity(child);
            }
            LOOP_CAROUSEL => {
                for i in 0..LOOP_CAROUSEL_SPOKES {
                    let c = ent.node_id + 1 + i;
                    self.node_mut(c).opacity = 255;
                    self.prune_identity(c);
                }
            }
            _ => {
                self.driver.cancel(LayerId(ent.node_id as u64), AnimProp::Opacity);
                self.set_prop(ent.node_id, AnimProp::Opacity, BREATHE_BRIGHT);
                self.prune_identity(ent.node_id);
            }
        }
    }

    /// Reconcile the continuous-loop set with the freshly-emitted `Loop`
    /// intents: forget loops gone from the tree and start each newly-seen
    /// looping widget's motion (sub-kind from the intent value).
    fn sync_loops(&mut self, present: &[(usize, AnimIntent)], boxes: &[nexus_layout::LayoutBox]) {
        let is_present = |e: &LoopEnt| {
            present
                .iter()
                .any(|(pn, pi)| *pn == e.node_id && pi.kind == AnimKind::Loop && pi.value == e.sub)
        };
        let mut i = 0;
        while i < self.loops.len() {
            if is_present(&self.loops[i]) {
                i += 1;
            } else {
                let ent = self.loops.swap_remove(i);
                self.forget_loop(ent);
            }
        }
        // Start newly-seen loops.
        for &(node_id, intent) in present {
            if intent.kind != AnimKind::Loop
                || self.loops.iter().any(|e| e.node_id == node_id)
            {
                continue;
            }
            if self.loops.len() >= MAX_NODE_ANIMS {
                break;
            }
            // step 0xFF = "not painted yet": the first tick always writes.
            self.loops.push(LoopEnt { node_id, sub: intent.value, step: 0xFF });
            match intent.value {
                LOOP_SWEEP => self.start_sweep(node_id, boxes),
                // Carousel paints on the first tick's time grid.
                LOOP_CAROUSEL => {}
                _ => self.start_breathe(node_id),
            }
        }
    }

    /// Keep every loop alive: a converged sweep resets to the left edge and
    /// re-fires (sawtooth), a converged breathe springs to the opposite
    /// endpoint, and the carousel advances its leading spoke on the time
    /// grid. Returns the union ROW SPAN of loop-driven paint changes the
    /// driver's own updates do NOT cover (the sawtooth jump-back + the
    /// carousel steps). Alloc-free.
    fn tick_loops(
        &mut self,
        now_ns: u64,
        boxes: &[nexus_layout::LayoutBox],
    ) -> Option<(i32, i32)> {
        let mut span: Option<(i32, i32)> = None;
        let mut grow = |boxes: &[nexus_layout::LayoutBox], node_id: usize,
                        span: &mut Option<(i32, i32)>| {
            if let Some(b) = boxes.iter().find(|b| b.node_id == node_id) {
                let (y0, y1) = (b.rect.y.0 - 1, b.rect.y.0 + b.rect.height.0 + 1);
                *span = Some(match *span {
                    Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
                    None => (y0, y1),
                });
            }
        };
        for idx in 0..self.loops.len() {
            let ent = self.loops[idx];
            match ent.sub {
                LOOP_SWEEP => {
                    let child = ent.node_id + 1;
                    if !self.driver.is_active(LayerId(child as u64), AnimProp::TranslateX) {
                        // Sawtooth: jump back to the left edge, glide again.
                        self.start_sweep(ent.node_id, boxes);
                        grow(boxes, ent.node_id, &mut span);
                    }
                }
                LOOP_CAROUSEL => {
                    let step =
                        ((now_ns / SPINNER_STEP_NS) % LOOP_CAROUSEL_SPOKES as u64) as u8;
                    if step != ent.step {
                        self.loops[idx].step = step;
                        self.write_carousel(ent.node_id, step);
                        grow(boxes, ent.node_id, &mut span);
                    }
                }
                _ => {
                    if !self.driver.is_active(LayerId(ent.node_id as u64), AnimProp::Opacity) {
                        self.start_breathe(ent.node_id);
                    }
                }
            }
        }
        span
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
        // Keep a node's transform when it is intent-driven OR its driver
        // animation still runs — INTERACTION motions (hover/press) target
        // handler boxes that carry no intent and must survive re-emits (a tap
        // re-emits the scene mid-press-bounce).
        let driver = &self.anim.driver;
        let loops = &self.anim.loops;
        self.anim.anims.retain(|a| {
            let layer = LayerId(a.node_id as u64);
            present.iter().any(|(p, _)| *p == a.node_id)
                || driver.is_active(layer, AnimProp::ScaleX)
                || driver.is_active(layer, AnimProp::Opacity)
                || driver.is_active(layer, AnimProp::TranslateX)
                || driver.is_active(layer, AnimProp::TranslateY)
                // Loop-owned CHILD entries (sweep band / carousel spokes)
                // carry no driver spring at every instant — keep them so a
                // re-emit mid-cycle doesn't flash the widget to identity.
                || loops.iter().any(|e| match e.sub {
                    LOOP_SWEEP => a.node_id == e.node_id + 1,
                    LOOP_CAROUSEL => {
                        a.node_id > e.node_id
                            && a.node_id <= e.node_id + LOOP_CAROUSEL_SPOKES
                    }
                    _ => a.node_id == e.node_id,
                })
        });
        // Continuous loops (inherently-animated widgets): start new, drop gone.
        self.anim.sync_loops(present, &self.layout.boxes);
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
        // Advance the continuous loops (sawtooth re-fire / breathe re-fire /
        // carousel step) and merge their extra paint damage.
        if let Some((y0, y1)) = self.anim.tick_loops(now, &self.layout.boxes) {
            let (y0, y1) = (y0.max(0), y1.min(self.h as i32));
            if y0 < y1 {
                span = Some(match span {
                    Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
                    None => (y0, y1),
                });
            }
        }
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

    /// INTERACTION MOTION (design handoff): the pointer entered/left an
    /// interactive control — spring the new target up to the hover scale
    /// (subtle overshoot, `--motion-spring-soft`) and the old one back to
    /// identity. Rides the same driver + NodeAnim + frame-pulse machinery as
    /// every other animation (no extra loop).
    pub(super) fn interaction_hover(&mut self, old: Option<usize>, new: Option<usize>) {
        // Containers (overlay backdrops, panel bodies) never grow.
        let new = new.filter(|&id| self.interaction_sized(id));
        let old = old.filter(|&id| self.interaction_sized(id));
        let was_idle = self.anim.driver.active_count() == 0;
        if let Some(id) = old {
            let cur = self.anim.cur(id, AnimProp::ScaleX);
            if (cur - 1.0).abs() > 0.001
                || self.anim.driver.is_active(LayerId(id as u64), AnimProp::ScaleX)
            {
                self.anim.driver.spring_to(
                    LayerId(id as u64),
                    AnimProp::ScaleX,
                    cur,
                    1.0,
                    HOVER_SPRING,
                );
            }
        }
        if let Some(id) = new {
            let cur = self.anim.cur(id, AnimProp::ScaleX);
            self.anim.driver.spring_to(
                LayerId(id as u64),
                AnimProp::ScaleX,
                cur,
                HOVER_SCALE,
                HOVER_SPRING,
            );
        }
        if was_idle && self.anim.driver.active_count() > 0 {
            self.anim.driver.reset_clock(nsec_now());
        }
    }

    /// INTERACTION MOTION: press feedback on tap — instant dip to 92% then a
    /// springy release with an elastic pop past identity (the handoff's
    /// "instant down, springy release" / `--motion-spring-icon` character).
    pub(super) fn interaction_press(&mut self, node_id: usize) {
        if !self.interaction_sized(node_id) {
            return; // container catch-all (backdrop/panel body): no dip
        }
        let was_idle = self.anim.driver.active_count() == 0;
        let cur = self.anim.cur(node_id, AnimProp::ScaleX);
        self.anim.driver.keyframe_to(
            LayerId(node_id as u64),
            AnimProp::ScaleX,
            alloc::vec![(0.0, cur), (0.3, PRESS_SCALE), (0.7, PRESS_POP), (1.0, 1.0)],
            PRESS_MS * 1_000_000,
            Easing::EaseOut,
        );
        if was_idle {
            self.anim.driver.reset_clock(nsec_now());
        }
    }

    /// INTERACTION MOTION (handoff): toggle press — the THUMB stretches along
    /// the travel axis (X) with Y pinned (capsule, never an ellipse), and when
    /// the flip moved the knob to the other end (`dx_from` = old − new x), it
    /// elastically slides into place from where it was. Non-uniform scale is
    /// the `NodeAnim` superset; every other interaction stays uniform.
    pub(super) fn interaction_toggle_thumb(&mut self, thumb_id: usize, dx_from: f32) {
        let was_idle = self.anim.driver.active_count() == 0;
        let layer = LayerId(thumb_id as u64);
        // Split the axes for this node: pin Y at identity.
        self.anim.set_prop(thumb_id, AnimProp::ScaleY, 1.0);
        let cur = self.anim.cur(thumb_id, AnimProp::ScaleX).max(1.0);
        self.anim.driver.keyframe_to(
            layer,
            AnimProp::ScaleX,
            alloc::vec![(0.0, cur), (0.35, TOGGLE_STRETCH), (1.0, 1.0)],
            PRESS_MS * 1_000_000,
            Easing::EaseOut,
        );
        if dx_from.abs() >= 1.0 {
            self.anim.set_prop(thumb_id, AnimProp::TranslateX, dx_from);
            self.anim.driver.spring_to(layer, AnimProp::TranslateX, dx_from, 0.0, HOVER_SPRING);
        }
        if was_idle && self.anim.driver.active_count() > 0 {
            self.anim.driver.reset_clock(nsec_now());
        }
    }

    /// Whether `node_id`'s box is CONTROL-sized (see `INTERACTION_MAX_DIM`):
    /// interaction motion targets buttons/tiles/pills, never container
    /// catch-all handlers (overlay backdrops, panel-body tap consumers).
    pub(super) fn interaction_sized(&self, node_id: usize) -> bool {
        self.layout
            .boxes
            .iter()
            .find(|b| b.node_id == node_id)
            .is_none_or(|b| {
                b.rect.width.0 <= INTERACTION_MAX_DIM && b.rect.height.0 <= INTERACTION_MAX_DIM
            })
    }

    /// Whether a BOUNDED (non-loop) animation is interpolating — the only
    /// animation state that may arm the recv-timeout SELF-PACE fallback. A
    /// continuous loop must ride the compositor frame pulse EXCLUSIVELY
    /// (the compositor owns pacing + visibility; a self-paced loop kept
    /// rendering at ~80Hz forever, hidden windows included).
    pub(super) fn anim_transient_active(&self) -> bool {
        let driver = &self.anim.driver;
        let loop_springs = self
            .anim
            .loops
            .iter()
            .filter(|e| match e.sub {
                LOOP_SWEEP => {
                    driver.is_active(LayerId((e.node_id + 1) as u64), AnimProp::TranslateX)
                }
                // The carousel owns no spring — it steps on the pulse only.
                LOOP_CAROUSEL => false,
                _ => driver.is_active(LayerId(e.node_id as u64), AnimProp::Opacity),
            })
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

    /// Expand the per-node transforms into a SUBTREE CASCADE for the painter:
    /// each animated container also transforms every box laid out INSIDE it
    /// (pre-order descendants ≈ rect containment + higher `node_id`),
    /// anchored at the CONTAINER's center — a hovered launcher tile grows
    /// tile, glyph and all as one control (the handoff interaction feel).
    /// A box with its OWN animation keeps its own entry. Alloc-free, bounded
    /// by `out.len()`.
    pub(super) fn expand_node_anims(&self, out: &mut [NodeAnim]) -> usize {
        let mut n = 0usize;
        for a in &self.anim.anims {
            if n >= out.len() {
                break;
            }
            let Some(owner) = self.layout.boxes.iter().find(|b| b.node_id == a.node_id)
            else {
                out[n] = *a;
                n += 1;
                continue;
            };
            let (ox, oy, ow, oh) = (
                owner.rect.x.0,
                owner.rect.y.0,
                owner.rect.width.0,
                owner.rect.height.0,
            );
            let (cx, cy) = (ox + ow / 2, oy + oh / 2);
            out[n] = a.anchored_at(cx, cy);
            n += 1;
            if a.is_identity() {
                continue; // identity: nothing to cascade
            }
            for b in &self.layout.boxes {
                if n >= out.len() {
                    break;
                }
                if b.node_id <= a.node_id
                    || b.rect.width.0 <= 0
                    || b.rect.height.0 <= 0
                {
                    continue;
                }
                let contained = b.rect.x.0 >= ox
                    && b.rect.y.0 >= oy
                    && b.rect.x.0 + b.rect.width.0 <= ox + ow
                    && b.rect.y.0 + b.rect.height.0 <= oy + oh;
                if !contained {
                    continue;
                }
                if self.anim.anims.iter().any(|o| o.node_id == b.node_id) {
                    continue; // its own animation wins
                }
                out[n] = NodeAnim { node_id: b.node_id, ..a.anchored_at(cx, cy) };
                n += 1;
            }
        }
        n
    }
}
