// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.motion(duration, curve)` in the host (TASK-0354 P10, docs/dev/dsl/modifiers.md §`.motion`):
//! the rest table — per declared node, keyed by its IDENTITY (its emitted path, so the two
//! branches of an `if` at one place are one node), the rest it was last laid out at — and the
//! tracks a change of rest starts: the deviation `bound_math` computes, decaying to identity on
//! the declared pair (opacity on `swift`/`smooth`, the design's surface rule). First sight
//! records, never moves; nothing starts inside the control the pointer holds; above
//! [`MAX_BOUND`] a node is not tracked and jumps to its new place.

use super::*;
use crate::bound_math::{deviation, opacity_target, Deviation, Rest};
use nexus_dsl_runtime::anim::unpack_bound;
use nexus_dsl_runtime::theme_tokens::{MotionCurveToken, MotionDurationToken};

/// The `.motion` nodes a surface tracks.
pub(super) const MAX_BOUND: usize = 128;
// The rest table outgrows the transform table: tracked nodes outnumber the moving ones.
const _: () = assert!(MAX_BOUND >= 4 * MAX_NODE_ANIMS);
const _: () = assert!(MAX_EXPANDED_ANIMS >= MAX_NODE_ANIMS);

/// One tracked node's rest.
#[derive(Clone, Copy)]
pub(super) struct BoundRest {
    identity: i32,
    rest: Rest,
}

/// A box's rest as `.motion` sees it.
fn rest_of(b: &nexus_layout::LayoutBox) -> Rest {
    Rest {
        x: b.rect.x.0,
        y: b.rect.y.0,
        w: b.rect.width.0,
        h: b.rect.height.0,
        rot: b.visual.rotate_deg,
        opacity: b.opacity,
    }
}

impl AnimState {
    /// What the node `node_id` is painted with over its rest right now (identity when nothing).
    fn carry(&self, node_id: usize) -> Deviation {
        let Some(a) = self.anims.iter().find(|a| a.node_id == node_id) else {
            return Deviation::IDENTITY;
        };
        Deviation {
            dx: a.dx as f32,
            dy: a.dy as f32,
            sx: f32::from(a.scale_pct) / 100.0,
            sy: f32::from(a.scale_y_pct.unwrap_or(a.scale_pct)) / 100.0,
            rot: f32::from(a.rot_deg),
            opacity: f32::from(a.opacity) / 255.0,
            base: a.base_opacity,
        }
    }

    /// One track of `prop` from `from` to `to` on the pair (a no-op when already there).
    fn track(&mut self, node_id: usize, prop: AnimProp, from: f32, to: f32, motion: (u64, Easing)) {
        self.driver.cancel(LayerId(node_id as u64), prop);
        self.set_prop(node_id, prop, from);
        if (from - to).abs() < 0.0001 {
            return;
        }
        self.driver.keyframe_to(
            LayerId(node_id as u64),
            prop,
            alloc::vec![(0.0, from), (1.0, to)],
            motion.0,
            motion.1,
        );
    }

    /// Reconcile the rest table with the `Bound` intents of a fresh emit over its fresh layout:
    /// a node seen for the first time records its rest; a node whose rest changed starts its
    /// deviation (unless it lies inside `held`, the rectangle of the control the pointer holds —
    /// it follows at once); a node gone from the tree is forgotten.
    pub(super) fn sync_bound(
        &mut self,
        present: &[(usize, AnimIntent)],
        boxes: &[nexus_layout::LayoutBox],
        held: Option<nexus_layout_types::Rect>,
        tokens: &dyn Tokens,
    ) {
        let ms = |d: MotionDurationToken| u64::from(tokens.motion_ms(d)).saturating_mul(1_000_000);
        let inside = |r: nexus_layout_types::Rect| {
            held.is_some_and(|h| {
                r.x >= h.x
                    && r.y >= h.y
                    && r.x + r.width <= h.x + h.width
                    && r.y + r.height <= h.y + h.height
            })
        };
        self.bound.retain(|e| {
            present.iter().any(|(_, i)| i.kind == AnimKind::Bound && i.value == e.identity)
        });
        for &(node_id, intent) in present.iter().filter(|(_, i)| i.kind == AnimKind::Bound) {
            let Some(b) = boxes.iter().find(|b| b.node_id == node_id) else { continue };
            let Some((dur, curve)) = unpack_bound(intent.token) else { continue };
            let new = rest_of(b);
            let Some(slot) = self.bound.iter().position(|e| e.identity == intent.value) else {
                if self.bound.len() < MAX_BOUND {
                    self.bound.push(BoundRest { identity: intent.value, rest: new });
                }
                continue;
            };
            let old = core::mem::replace(&mut self.bound[slot].rest, new);
            // Inside the held control the press owns the motion: a slider's fill follows the
            // pointer at once, and a tapped toggle's thumb stretches AS it slides (one gesture,
            // `interaction_toggle_thumb`); `.motion` carries every change without a press.
            if inside(b.rect) {
                continue;
            }
            let Some(dev) = deviation(old, new, self.carry(node_id)) else { continue };
            let geometry = (ms(dur), Easing::Curve(curve));
            let fade = (ms(MotionDurationToken::Swift), Easing::Curve(MotionCurveToken::Smooth));
            self.track(node_id, AnimProp::TranslateX, dev.dx, 0.0, geometry);
            self.track(node_id, AnimProp::TranslateY, dev.dy, 0.0, geometry);
            self.track(node_id, AnimProp::ScaleX, dev.sx, 1.0, geometry);
            // Always split the axes: a width change must not scale the height with it.
            self.track(node_id, AnimProp::ScaleY, dev.sy, 1.0, geometry);
            self.track(node_id, AnimProp::Rotate, dev.rot, 0.0, geometry);
            self.node_mut(node_id).base_opacity = dev.base;
            let to = opacity_target(&dev, new.opacity);
            self.track(node_id, AnimProp::Opacity, dev.opacity, to, fade);
        }
    }
}
