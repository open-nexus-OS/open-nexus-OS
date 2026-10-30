// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The pointer's motion on its way to a surface (RFC-0113 §2, the `frame` delivery
//! class): the applied sample's batch (display space, every report since the last frame) rides
//! the MOVE to the surface under the pointer or the DRAG to the surface that took the press,
//! shifted into that surface's space. A MOVE the frame pace holds back keeps its samples for the
//! next one; a DRAG a full queue refused keeps them for its retry; a frame the client's outbox
//! replaces hands them to its successor (`client_outbox`, `surface_input::join_motion_frames`).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the codec's tests (`nexus-display-proto`), `tests/input_staging.rs` (the
//!   staging keeps every sample); the usb-visible lane's hover and drag steps

#[cfg(nexus_env = "os")]
use super::*;
#[cfg(nexus_env = "os")]
use nexus_display_proto::client_surface as wire;
#[cfg(nexus_env = "os")]
use nexus_display_proto::surface_input::{
    encode_surface_motion, SURFACE_INPUT_FRAME_LEN, SURFACE_MOTION_FRAME_MAX,
};
use nexus_display_proto::surface_motion::MotionBatch;

/// Where the pointer's samples wait between the staged input and a surface's frame.
#[derive(Default)]
pub(crate) struct MotionRoute {
    /// The motion of the sample being applied (display space).
    pub(super) applied: MotionBatch,
    /// Hover samples the frame pace held back (display space).
    pub(super) hover: MotionBatch,
    /// Drag samples a full queue refused (display space).
    pub(super) owed: MotionBatch,
    /// The batch the next MOVE or DRAG frame carries (its surface's space).
    pub(super) out: MotionBatch,
}

#[cfg(nexus_env = "os")]
impl DisplayServerRuntime {
    /// One pointer frame for surface `id`: a MOVE or DRAG with the motion staged for it
    /// (`motion.out`, taken), a tap with the press's click count and modifiers (TASK-0346 P2),
    /// any other kind plain.
    pub(super) fn pointer_frame(
        &mut self,
        id: u32,
        kind: u8,
        x: i32,
        y: i32,
    ) -> ([u8; SURFACE_MOTION_FRAME_MAX], usize) {
        let (x, y) = (x.max(0) as u16, y.max(0) as u16);
        let motion = core::mem::take(&mut self.motion.out);
        if let Some(frame) = encode_surface_motion(id, kind, x, y, &motion) {
            return frame;
        }
        let facts =
            if kind == wire::INPUT_KIND_TAP { self.presses.facts } else { Default::default() };
        let mut frame = [0u8; SURFACE_MOTION_FRAME_MAX];
        frame[..SURFACE_INPUT_FRAME_LEN]
            .copy_from_slice(&wire::encode_surface_pointer(id, kind, x, y, facts));
        (frame, SURFACE_INPUT_FRAME_LEN)
    }
}
