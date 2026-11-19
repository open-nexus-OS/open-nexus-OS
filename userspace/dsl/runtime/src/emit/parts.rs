// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The kit widgets' moving parts as intents (TASK-0354 P10): for each row of
//! [`registry::motion_parts`](crate::registry::motion_parts) one intent on the part's path — a
//! presence part an `Animate` (its present/absent value), a travelling part a `Bound` keyed by
//! the part's identity (its emitted path), exactly what `.animate` / `.motion` on the part would
//! stamp.

use super::EmitCtx;
use crate::anim::{pack_bound, transition_identity, AnimIntent, AnimKind};
use crate::registry::{self, PartMotion};
use crate::store::Value;
use alloc::string::String;

/// Stamps the moving parts of the widget of `kind` at the current path.
pub(super) fn stamp_parts(ctx: &mut EmitCtx<'_, '_>, kind: &str, props: &[(String, Value)]) {
    for (part, motion) in registry::motion_parts(kind, props).into_iter().flatten() {
        let mark = ctx.path.len();
        ctx.path.extend_from_slice(part);
        let intent = match motion {
            PartMotion::Presence { token, present } => {
                AnimIntent::new(AnimKind::Animate, token.id(), i32::from(present))
            }
            PartMotion::Bound { duration, curve } => AnimIntent::new(
                AnimKind::Bound,
                pack_bound(duration, curve),
                transition_identity(&ctx.path),
            ),
        };
        ctx.anim_intents.push((ctx.path.clone(), intent));
        ctx.path.truncate(mark);
    }
}
