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
//! `SceneUpdate`s into the per-node [`NodeAnim`] table → the cascade expansion
//! hands the painter the current transforms. Alloc-bounded: the driver, the
//! diff table, and the transform table are all capped at [`MAX_NODE_ANIMS`];
//! the per-frame tick is allocation-free (`tick_into` into a stack buffer).

use super::*;
use animation::{
    shake_px, AnimProp, AnimationDriver, Easing, LayerId, MotionToken, SceneUpdate, SpringConfig,
};

mod bound;
mod damage;
mod interaction;
mod loops;
use bound::BoundRest;
pub(super) use interaction::HeldPress;
use loops::{is_loop_part, LoopEnt};
use nexus_dsl_runtime::theme_tokens::Tokens;
use nexus_dsl_runtime::{
    AnimIntent, AnimKind, LOOP_CAROUSEL, LOOP_CAROUSEL_SPOKES, LOOP_SHIMMER, LOOP_SWEEP,
};
use nexus_scene_raster::NodeAnim;

/// Max concurrent DSL node transforms (mirrors the engine's active-animation
/// cap). Bounds the driver, the value-diff table, and the paint transform
/// table so none grows on the heap. Sized for the modifier
/// anims + widget loops (a Spinner carousel alone holds 12 spoke entries) +
/// the INTERACTION motions (a press dip and its spring back, the toggle
/// thumb's stretch and slide).
pub(super) const MAX_NODE_ANIMS: usize = 32;
/// Expanded (subtree-cascaded) paint transform cap — each animated container
/// may fan out to its contained boxes (tile + glyph + label…).
pub(super) const MAX_EXPANDED_ANIMS: usize = 48;

/// Pulse peak scale (fraction over 1.0) of the `.effect(pulse)` token.
const PULSE_PEAK: f32 = 0.12;

/// The property target for a driving value under a token — `MotionToken`'s own
/// present/absent contract, which is the SSOT both this seed path and the app
/// conformance tests read. It used to live HERE, in a RISC-V-only module no
/// host test could reach; that is precisely why "an unfoldable `value:` rests
/// the node at opacity 0" reached a device unnoticed.
fn target_for(token: MotionToken, prop: AnimProp, value: i32) -> f32 {
    token.resting(prop, value != 0)
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
    /// widgets), keyed by root node + sub-kind. The sweeps and the carousel are positions
    /// on a time grid (`loop_math`); the breathe spring re-fires. These keep the frame
    /// pulse armed while the widget is on screen. Bounded by the cap.
    loops: alloc::vec::Vec<LoopEnt>,
    /// `.motion` nodes' rests by identity (`anim/bound.rs`), bounded by `bound::MAX_BOUND`.
    bound: alloc::vec::Vec<BoundRest>,
    /// The emit's intents, reused across syncs (transitions and loops first, so a bound cuts the
    /// least important): no snapshot array by value on the stack.
    intents: alloc::vec::Vec<(usize, AnimIntent)>,
    /// Last physics tick (ns) for dt integration (diagnostics/telemetry).
    last_ns: u64,
    /// The tokens that started a motion since the last tick (bit = id): the episode's line.
    started: u32,
}

impl AnimState {
    pub(super) fn new() -> Self {
        Self {
            driver: AnimationDriver::new(),
            seen: alloc::vec::Vec::new(),
            anims: alloc::vec::Vec::new(),
            loops: alloc::vec::Vec::new(),
            bound: alloc::vec::Vec::new(),
            intents: alloc::vec::Vec::new(),
            last_ns: 0,
            started: 0,
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
            return prop.identity();
        };
        match prop {
            AnimProp::Opacity => a.opacity as f32 / 255.0,
            AnimProp::TranslateX => a.dx as f32,
            AnimProp::TranslateY => a.dy as f32,
            AnimProp::ScaleX => a.scale_pct as f32 / 100.0,
            // ScaleY mirrors X unless a non-uniform interaction split it
            // (toggle-thumb stretch) — the NodeAnim superset contract.
            AnimProp::ScaleY => a.scale_y_pct.unwrap_or(a.scale_pct) as f32 / 100.0,
            AnimProp::Rotate => f32::from(a.rot_deg),
            _ => prop.identity(),
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
            AnimProp::Rotate => {
                a.rot_deg = (if v >= 0.0 { v + 0.5 } else { v - 0.5 }).clamp(-720.0, 720.0) as i16;
            }
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
        self.started |= 1 << (token as u32);
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

    /// An `.animate` part that became PRESENT. The scene draws it now; the token's enter plays
    /// from where it is seen — from its absent rest when nothing was painting it, or from the
    /// point an exit had reached (no jump).
    fn present(&mut self, node_id: usize, token: MotionToken, tokens: &dyn Tokens) {
        let Some(a) = self.anims.iter_mut().find(|a| a.node_id == node_id) else {
            self.enter(node_id, token, tokens);
            return;
        };
        // An exit was painting it over a base; the box itself is drawn again.
        let seen =
            a.base_opacity.map_or(1.0, |b| f32::from(b) / 255.0) * f32::from(a.opacity) / 255.0;
        a.base_opacity = None;
        self.set_prop(node_id, AnimProp::Opacity, seen);
        for prop in [Some(token.primary_prop()), token.secondary_prop(), Some(AnimProp::Opacity)] {
            let Some(prop) = prop else { continue };
            self.start_prop(node_id, token, prop, target_for(token, prop, 1), tokens);
        }
    }

    /// An `.animate` part that became ABSENT. The scene no longer draws it (its own opacity 0);
    /// the exit paints it over a base of full opacity, the token played back toward its absent
    /// rest, and is reset to identity when the fade ends ([`Self::fold`]).
    fn absent(&mut self, node_id: usize, token: MotionToken, tokens: &dyn Tokens) {
        self.node_mut(node_id).base_opacity = Some(255);
        for prop in [Some(token.primary_prop()), token.secondary_prop(), Some(AnimProp::Opacity)] {
            let Some(prop) = prop else { continue };
            self.start_prop(node_id, token, prop, target_for(token, prop, 0), tokens);
        }
    }

    /// Fold one driver update into the node's transform. An opacity track that ended over a base
    /// hands the picture back to the scene: faded out (an exit, a `.motion` to 0) the whole slot
    /// returns to identity and the node's other tracks stop — the scene draws nothing there;
    /// faded to a lower rest (a `.motion` down) the base and the factor clear, the box draws its
    /// own opacity, which is where the track ended.
    fn fold(&mut self, u: &SceneUpdate) {
        let node_id = u.layer_id.0 as usize;
        self.set_prop(node_id, u.property, u.value);
        if u.property != AnimProp::Opacity || u.progress < 1.0 {
            return;
        }
        let Some(a) = self.anims.iter_mut().find(|a| a.node_id == node_id) else { return };
        if a.base_opacity.is_none() {
            return;
        }
        if a.opacity == 0 {
            *a = NodeAnim::identity(node_id);
            let layer = LayerId(node_id as u64);
            for prop in [
                AnimProp::TranslateX,
                AnimProp::TranslateY,
                AnimProp::ScaleX,
                AnimProp::ScaleY,
                AnimProp::Rotate,
            ] {
                self.driver.cancel(layer, prop);
            }
        } else {
            a.base_opacity = None;
            a.opacity = 255;
        }
    }

    /// Fire a bounded attention effect (`.effect(wiggle|pulse)`): a keyframe
    /// oscillation that RETURNS to identity (so the entry is pruned on
    /// convergence). Linear between the explicit keyframes.
    fn start_effect(
        &mut self,
        node_id: usize,
        token: MotionToken,
        prev: i32,
        now: i32,
        tokens: &dyn Tokens,
    ) {
        let layer = LayerId(node_id as u64);
        let dur = Self::dur_ns(token, tokens);
        match token {
            // Content swapped in place enters from the side it travels from (a larger trigger
            // from the trailing side), invisible, and glides to rest.
            MotionToken::SwapInline => {
                let dir = if now >= prev { 1 } else { -1 };
                self.set_prop(node_id, AnimProp::TranslateX, animation::motion::swap_px(0.0, dir));
                self.set_prop(node_id, AnimProp::Opacity, 0.0);
                self.start_prop(node_id, token, AnimProp::TranslateX, 0.0, tokens);
                self.start_prop(node_id, token, AnimProp::Opacity, 1.0, tokens);
            }
            // The design's shake, a track through its eased steps (five points a step).
            MotionToken::Wiggle => self.driver.keyframe_to(
                layer,
                AnimProp::TranslateX,
                (0..=50u8).map(|k| f32::from(k) / 50.0).map(|t| (t, shake_px(t))).collect(),
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
    fn seed(
        &mut self,
        node_id: usize,
        token: MotionToken,
        intent: AnimIntent,
        tokens: &dyn Tokens,
    ) {
        match intent.kind {
            // The scene holds the rest (absent = the node's own opacity 0): first sight records.
            AnimKind::Animate => {}
            AnimKind::Transition => self.enter(node_id, token, tokens),
            // Effects have no resting change — they fire on a trigger change.
            AnimKind::Effect => {}
            // Loops are reconciled in `sync_loops`, `.motion` in `sync_bound`, never here.
            AnimKind::Loop | AnimKind::Bound => {}
        }
    }

    /// A node whose driving value CHANGED from `prev`: (re)start the token's motion.
    fn restart(
        &mut self,
        node_id: usize,
        token: MotionToken,
        intent: AnimIntent,
        prev: i32,
        tokens: &dyn Tokens,
    ) {
        match intent.kind {
            AnimKind::Animate if intent.value != 0 => self.present(node_id, token, tokens),
            AnimKind::Animate => self.absent(node_id, token, tokens),
            AnimKind::Transition => self.enter(node_id, token, tokens),
            AnimKind::Effect => self.start_effect(node_id, token, prev, intent.value, tokens),
            // Loops are reconciled in `sync_loops`, `.motion` in `sync_bound`, never here.
            AnimKind::Loop | AnimKind::Bound => {}
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
        let mut buf = core::mem::take(&mut self.anim.intents);
        buf.clear();
        let cap = bound::MAX_BOUND + MAX_NODE_ANIMS;
        let first = |i: &AnimIntent| matches!(i.kind, AnimKind::Transition | AnimKind::Loop);
        let all = self.view.animations();
        buf.extend(all.iter().filter(|(_, i)| first(i)).copied());
        buf.extend(all.iter().filter(|(_, i)| !first(i)).copied());
        buf.truncate(cap);
        let present = &buf[..];
        // The motion layers follow the nodes that still enter — first, so a panel closing in the
        // same change as a modal opening leaves the modal room to be a layer.
        let mut entering = [(0usize, 0i32); MAX_NODE_ANIMS];
        let e = present.iter().filter(|(_, i)| i.kind == AnimKind::Transition).count();
        for (slot, (id, i)) in
            entering.iter_mut().zip(present.iter().filter(|(_, i)| i.kind == AnimKind::Transition))
        {
            *slot = (*id, i.value);
        }
        self.sync_motion_layers(&entering[..e]);
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
            // TASK-0354 P2: an enter the compositor can draw is stated once — never ticked here.
            let enters =
                intent.kind == AnimKind::Transition && prev.is_none_or(|pv| pv != intent.value);
            let layered = enters
                && match self.start_motion_layer((node_id, intent.value), token, prev.is_some()) {
                    Ok(()) => true,
                    // An effect returns to rest and is the app's by design; any other enter the
                    // compositor did not take says why, once.
                    Err(why) => {
                        if why != "effect" {
                            raw_marker(&alloc::format!(
                                "apphost: motion in app (token={} why={why})",
                                token.id()
                            ));
                        }
                        false
                    }
                };
            match prev {
                _ if layered => {}
                None => self.anim.seed(node_id, token, intent, tokens),
                Some(pv) if pv != intent.value => {
                    self.anim.restart(node_id, token, intent, pv, tokens)
                }
                _ => {}
            }
            self.anim.upsert_seen(node_id, intent.value);
        }
        // Nodes removed from the tree: forget them (no exit motion in the
        // immediate-mode model — an exit needs the node kept alive, Track C).
        self.anim
            .seen
            .retain(|(id, _)| present.iter().any(|(p, i)| p == id && i.kind != AnimKind::Loop));
        // Keep a node's transform when it is intent-driven OR its driver
        // animation still runs OR a held press dips it — INTERACTION motions
        // target handler boxes that carry no intent and must survive re-emits
        // (a tap re-emits the scene mid-dip; a press stays down for seconds).
        let held = self.press_target();
        let driver = &self.anim.driver;
        let loops = &self.anim.loops;
        self.anim.anims.retain(|a| {
            let layer = LayerId(a.node_id as u64);
            present.iter().any(|(p, _)| *p == a.node_id)
                || held == Some(a.node_id)
                || driver.is_active(layer, AnimProp::ScaleX)
                || driver.is_active(layer, AnimProp::Opacity)
                || driver.is_active(layer, AnimProp::TranslateX)
                || driver.is_active(layer, AnimProp::TranslateY)
                // Loop-owned parts carry no driver spring at every instant.
                || is_loop_part(loops, a.node_id)
        });
        // Continuous loops (inherently-animated widgets): start new, drop gone.
        self.anim.sync_loops(present);
        // `.motion`: a changed rest travels (nothing inside the control the pointer holds).
        let held =
            self.press_target().and_then(|id| self.layout.boxes.iter().find(|b| b.node_id == id));
        self.anim.sync_bound(present, &self.layout.boxes, held.map(|b| b.rect), tokens);
        // Arm the clock on the idle→active edge (see `was_idle` above).
        if was_idle && self.anim.driver.active_count() > 0 {
            self.anim.driver.reset_clock(nsec_now());
        }
        self.anim.intents = buf;
    }

    /// Advance the animation physics to `at` on the frame pulse (RFC-0110: when the frame is
    /// shown; the own clock under a plain pulse) and fold the `SceneUpdate`s into the node table.
    /// What the changes damage is pending with it: per touched node its transformed rectangle,
    /// old and new (`anim/damage.rs`). The present repaints EXACTLY those (the 120Hz damage
    /// contract: a 16px skeleton breathe must never trigger a full-surface repaint + full-band
    /// re-blit + full recomposite). Allocation-free.
    pub(super) fn anim_tick(&mut self, at: Option<u64>) {
        if self.anim.driver.active_count() == 0 && self.anim.loops.is_empty() {
            return;
        }
        let now = at.unwrap_or_else(nsec_now).max(self.anim.last_ns);
        self.motion_hops.tokens(core::mem::take(&mut self.anim.started));
        let mut updates = [SceneUpdate::default(); MAX_NODE_ANIMS * 2];
        let count = self.anim.driver.tick_into(now, &mut updates);
        self.anim.last_ns = now;
        for u in &updates[..count] {
            let node_id = u.layer_id.0 as usize;
            // Damage BEFORE the fold (the node's currently-painted rect).
            self.damage_anim(node_id);
            self.anim.fold(u);
            // Converged (progress==1) back to identity → drop the slot.
            if u.progress >= 1.0 {
                self.anim.prune_identity(node_id);
            }
            // …and AFTER (where it paints now) — the two cover the motion.
            self.damage_anim(node_id);
        }
        // Advance the continuous loops (sawtooth re-fire / breathe re-fire /
        // carousel step): their extra paint damage is rows.
        // A reduced-motion theme parks the loops at rest (bars full, pip and band outside).
        let reduced = tokens_for(self.theme_mode)
            .motion_ms(nexus_dsl_runtime::theme_tokens::MotionDurationToken::Quick)
            == 0;
        if let Some((y0, y1)) = self.anim.tick_loops(now, &self.layout.boxes, reduced) {
            self.pending.rows((y0.max(0), y1.min(self.layout_bottom())), self.w);
        }
    }

    /// Whether any DSL animation is still interpolating OR a continuous widget
    /// loop is live — the FRAME-arm re-arms the compositor pulse while true
    /// (windowd parks the request for hidden windows, so a continuous loop
    /// costs nothing off-screen).
    pub(super) fn anim_active(&self) -> bool {
        self.anim.driver.active_count() > 0 || !self.anim.loops.is_empty()
    }

    /// The per-node transforms for the painter, each covering its SUBTREE (`NodeAnim::span_end`,
    /// pre-order — one walk of the scene for all of them): a pressed button dips pill, glyph and
    /// label as one control, a swapped month slides every cell — with no bound on the boxes a
    /// node holds (a copied entry per contained box stopped at 48: the calendar's lower rows
    /// changed at once, board--2026-11-19T12-30-24). A box with its own entry keeps it
    /// (`nexus_scene_raster::covering_index`). Anchored at the node's centre, a level bar at its
    /// bottom, a token that grows from its corner at that corner. Alloc-free, bounded by
    /// `out.len()`.
    pub(super) fn expand_node_anims(&self, out: &mut [NodeAnim]) -> usize {
        let mut n = 0usize;
        let mut ranges = [(0usize, 0usize); MAX_EXPANDED_ANIMS];
        let mut m = 0usize;
        for a in &self.anim.anims {
            if n >= out.len() {
                break;
            }
            let Some(owner) = self.layout.boxes.iter().find(|b| b.node_id == a.node_id) else {
                out[n] = *a;
                n += 1;
                continue;
            };
            let (ox, oy, ow, oh) =
                (owner.rect.x.0, owner.rect.y.0, owner.rect.width.0, owner.rect.height.0);
            // A level bar scales from its bottom.
            let centre =
                (ox + ow / 2, if self.anim.level_bar(a.node_id) { oy + oh } else { oy + oh / 2 });
            // A token that grows from the corner it hangs on (a menu) scales about that corner.
            let (cx, cy) =
                self.enter_anchor(a.node_id).filter(|_| !a.is_identity()).unwrap_or(centre);
            out[n] = a.anchored_at(cx, cy);
            if !a.is_identity() && m < ranges.len() {
                ranges[m] = (a.node_id, a.node_id + 1);
                m += 1;
            }
            n += 1;
        }
        // The subtrees the moving nodes cover, in one walk.
        let ranges = &mut ranges[..m];
        ranges.sort_unstable_by_key(|r| r.0);
        crate::motion_math::subtree_ends(self.view.scene(), ranges);
        for a in out[..n].iter_mut() {
            if let Ok(k) = ranges.binary_search_by_key(&a.node_id, |r| r.0) {
                a.span_end = ranges[k].1;
            }
        }
        n
    }
}
