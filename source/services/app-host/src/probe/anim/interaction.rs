// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! INTERACTION motion — the hover and press layers, the press dip and the toggle thumb.
//!
//! Split out of `anim.rs` because it is a different KIND of animation from everything else
//! there. The parent module drives motion a page DECLARED (`.animate`/`.transition`/`.effect`)
//! plus the loops a widget owns; this one is motion nobody declared, synthesised from pointer
//! events by the design artifacts' interaction rules (RFC-0116): hover never changes a control —
//! its layer fades in by opacity (`crate::interaction_layer`, a wash the compositor draws); a
//! press is HELD from windowd's TAP to its RELEASE — the press layer, and the dip its category
//! asks for (`crate::press_kind`), let go with the release. The dips share the driver and the
//! `NodeAnim` table with the declared motion, and nothing else — so this is a CHILD module: it
//! reaches the parent's private `AnimState` helpers without any of them widening for the move.

use super::{nsec_now, tokens_for, AnimProp, Easing, LayerId};
use crate::interaction_layer::{Kind, Shade};
use crate::press_kind::{self, PressKind, PressPlan};
use nexus_dsl_runtime::theme_tokens::{MotionCurveToken, MotionDurationToken};

/// A press held down: windowd's TAP until its RELEASE.
#[derive(Clone, Copy, Debug)]
pub(in crate::probe) struct HeldPress {
    /// The handler it landed on.
    handler: usize,
    /// What it dips: the control, an app icon's tile, the toggle's thumb.
    target: Option<usize>,
    kind: PressKind,
    /// Its press layer is shown.
    layered: bool,
}

impl crate::probe::DslApp {
    /// The pointer moved onto control `new` (or off every control): its hover layer fades in and
    /// the last one's out — hover never scales or repaints a control. Motion only; the caller
    /// arms the frame pulse.
    pub(in crate::probe) fn interaction_hover(&mut self, new: Option<usize>) {
        let target = new.and_then(|h| {
            let anchor = self.press_plan(h).anchor?;
            Some((anchor, self.palette.shade(Kind::Hover, self.on_fill(anchor))))
        });
        self.show_layer(Kind::Hover, target);
    }

    /// A press landed on `handler` (windowd's TAP): its press layer fades in and it dips as its
    /// category asks — held until the release ([`Self::interaction_release`]).
    pub(in crate::probe) fn interaction_press(&mut self, handler: usize) {
        self.interaction_release(); // a press still held (its release never came) goes first
        let plan = self.press_plan(handler);
        let layer = plan.anchor.map(|a| (a, self.palette.shade(Kind::Press, self.on_fill(a))));
        if layer.is_some() {
            self.show_layer(Kind::Press, layer);
        }
        if let (Some(target), Some(scale)) = (plan.target, plan.kind.scale()) {
            let (token, easing) = plan.kind.down();
            self.dip(target, AnimProp::ScaleX, scale, token, easing);
        }
        let layered = layer.is_some();
        self.press = Some(HeldPress { handler, target: plan.target, kind: plan.kind, layered });
    }

    /// The press lets go (windowd's RELEASE, or the pointer left the control): the press layer
    /// fades out and the dip springs back. Returns whether a press was held.
    pub(in crate::probe) fn interaction_release(&mut self) -> bool {
        let Some(p) = self.press.take() else { return false };
        if p.layered {
            self.show_layer(Kind::Press, None);
        }
        if let Some(target) = p.target {
            let (token, easing) = p.kind.up();
            self.dip(target, AnimProp::ScaleX, 1.0, token, easing);
            if p.kind == PressKind::Part {
                self.dip(target, AnimProp::TranslateX, 0.0, token, easing);
            }
        }
        true
    }

    /// Whether the held press's control is no longer under the pointer at `(x, y)` — the design
    /// lets a press go when the pointer leaves its control.
    pub(in crate::probe) fn press_left(&self, x: i32, y: i32) -> bool {
        self.press.is_some_and(|p| self.tap_target_at(x, y) != Some(p.handler))
    }

    /// What the pointer showed goes at once — the scene's nodes were laid out anew (a re-emit, a
    /// resize): the hovered control, its layers, the held press.
    pub(in crate::probe) fn interaction_reset(&mut self) {
        self.hovered = None;
        self.layers.clear();
        self.press = None;
        self.motion_dirty = true;
    }

    /// The node a held press dips: the animation table keeps its transform across a re-emit.
    pub(in crate::probe) fn press_target(&self) -> Option<usize> {
        self.press.and_then(|p| p.target)
    }

    /// Whether an interaction layer still fades (the frame pulse stays armed).
    pub(in crate::probe) fn layers_live(&self) -> bool {
        self.layers.live()
    }

    /// The interaction layers at the pulse's shown time `now`: `true` when their washes changed
    /// (a motion frame is owed — no raster).
    pub(in crate::probe) fn layers_tick(&mut self, now: u64) -> bool {
        self.layers.tick(now)
    }

    /// The toggle's press (the design's switch): the thumb stretches along the travel by
    /// `round(w · 0.3)`, anchored to the end it stands at (Y pinned: a capsule, never an
    /// ellipse), and when the flip moved it to the other end (`dx_from` = old − new x) it slides
    /// in from where it was — `quick`, spring-soft; held until the release.
    pub(in crate::probe) fn interaction_toggle_thumb(
        &mut self,
        track: usize,
        thumb: usize,
        dx_from: f32,
    ) {
        self.interaction_release();
        let rect = |id| self.layout.boxes.iter().find(|b| b.node_id == id).map(|b| b.rect);
        let (Some(t), Some(k)) = (rect(track), rect(thumb)) else { return };
        let (stretch, grow) = press_kind::thumb_stretch(k.width.0);
        // The scale grows about the thumb's centre: the shift keeps its end's edge in place.
        let at_end = k.x.0 * 2 + k.width.0 >= t.x.0 * 2 + t.width.0;
        let shift = if at_end { -grow as f32 / 2.0 } else { grow as f32 / 2.0 };
        // Split the axes for this node: pin Y at identity.
        self.anim.set_prop(thumb, AnimProp::ScaleY, 1.0);
        let from = self.anim.cur(thumb, AnimProp::TranslateX) + dx_from;
        self.anim.set_prop(thumb, AnimProp::TranslateX, from);
        let (token, easing) =
            (MotionDurationToken::Quick, Easing::Curve(MotionCurveToken::SpringSoft));
        self.dip(thumb, AnimProp::ScaleX, stretch, token, easing);
        self.dip(thumb, AnimProp::TranslateX, shift, token, easing);
        let held = HeldPress {
            handler: track,
            target: Some(thumb),
            kind: PressKind::Part,
            layered: false,
        };
        self.press = Some(held);
    }

    /// `prop` of `node` moves from where it is to `to` over `token` on `easing` — arming the
    /// clock of an idle driver (its first tick would otherwise measure the whole idle gap).
    fn dip(
        &mut self,
        node: usize,
        prop: AnimProp,
        to: f32,
        token: MotionDurationToken,
        easing: Easing,
    ) {
        let was_idle = self.anim.driver.active_count() == 0;
        let ns = u64::from(tokens_for(self.theme_mode).motion_ms(token)) * 1_000_000;
        let from = self.anim.cur(node, prop);
        let frames = alloc::vec![(0.0, from), (1.0, to)];
        self.anim.driver.keyframe_to(LayerId(node as u64), prop, frames, ns, easing);
        if was_idle {
            self.anim.driver.reset_clock(nsec_now());
        }
    }

    /// The `kind` layer to `target` over its motion token (motion: a frame is owed).
    fn show_layer(&mut self, kind: Kind, target: Option<(usize, Shade)>) {
        let ns = u64::from(tokens_for(self.theme_mode).motion_ms(kind.duration())) * 1_000_000;
        self.layers.show(kind, target, nsec_now(), ns);
        self.motion_dirty = true;
    }

    /// What a press on (or the hover of) handler `h` does on this surface: a kit control that
    /// animates a part of itself (the toggle's thumb, `press_offset`) takes no layer of its own.
    fn press_plan(&self, h: usize) -> PressPlan {
        let part = self.view.handlers().iter().any(|(id, e)| *id == h && e.press_offset != 0);
        press_kind::plan(&self.layout.boxes, h, part, self.base_alpha != 0)
    }

    /// Whether control `anchor` is filled: its layers take the on-fill colours.
    fn on_fill(&self, anchor: usize) -> bool {
        let tokens = tokens_for(self.theme_mode);
        let b = self.layout.boxes.iter().find(|b| b.node_id == anchor);
        b.is_some_and(|b| press_kind::on_fill(b, tokens))
    }
}
