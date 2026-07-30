// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! INTERACTION motion — hover, press and the toggle thumb.
//!
//! Split out of `anim.rs` because it is a different KIND of animation from
//! everything else there. The parent module drives motion a page DECLARED
//! (`.animate`/`.transition`/`.effect`) plus the loops a widget owns; this one
//! is motion nobody declared, synthesised from pointer events so every control
//! feels alive without an author asking. They share the driver and the
//! `NodeAnim` table, and nothing else — so this is a CHILD module: it reaches
//! the parent's private `AnimState` helpers without any of them widening to
//! `pub(super)` for the sake of the move.
//!
//! The size guard is the load-bearing rule here: interaction motion applies to
//! CONTROL-sized elements only. A 1.06 hover scale on a 328-wide panel visibly
//! displaces its content away from the (unscaled) hit boxes.

use super::{nsec_now, AnimProp, Easing, LayerId};

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
const HOVER_SPRING: animation::SpringConfig =
    animation::SpringConfig { stiffness: 420.0, damping: 22.0, mass: 1.0, initial_velocity: 0.0 };
/// Toggle-thumb press: peak stretch along the travel axis (the handoff
/// "toggles stretch the thumb while pressed" — capsule, Y pinned).
const TOGGLE_STRETCH: f32 = 1.35;
/// Interaction motion applies to CONTROL-sized elements only: container
/// catch-all handlers (overlay backdrop = full screen, panel-body tap
/// consumers) must never hover-grow/press-dip — a 1.06 scale on a 328-wide
/// panel visibly displaces content from its (unscaled) hit boxes.
const INTERACTION_MAX_DIM: i32 = 160;

impl crate::probe::DslApp {
    /// INTERACTION MOTION (design handoff): the pointer entered/left an
    /// interactive control — spring the new target up to the hover scale
    /// (subtle overshoot, `--motion-spring-soft`) and the old one back to
    /// identity. Rides the same driver + NodeAnim + frame-pulse machinery as
    /// every other animation (no extra loop).
    pub(in crate::probe) fn interaction_hover(&mut self, old: Option<usize>, new: Option<usize>) {
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
    pub(in crate::probe) fn interaction_press(&mut self, node_id: usize) {
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
    pub(in crate::probe) fn interaction_toggle_thumb(&mut self, thumb_id: usize, dx_from: f32) {
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
    pub(in crate::probe) fn interaction_sized(&self, node_id: usize) -> bool {
        self.layout.boxes.iter().find(|b| b.node_id == node_id).is_none_or(|b| {
            b.rect.width.0 <= INTERACTION_MAX_DIM && b.rect.height.0 <= INTERACTION_MAX_DIM
        })
    }
}
