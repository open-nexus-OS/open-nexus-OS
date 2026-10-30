// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The visible state's push with the pointer's motion behind it (RFC-0113 §2, wire v4):
//! inputd throttles its pushes, so a push carries every sample the device reported since the
//! last one — positions in display space, each with its event's time and sequence — and windowd
//! stages them into the frame's batch. The batch type is `nexus_display_proto::surface_motion`'s:
//! one type for this hop and the next (windowd → app).
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (round trip, `test_reject_*`)

use nexus_display_proto::surface_motion::{MotionBatch, MOTION_WIRE_MAX};

use crate::{
    decode_update_visible_state, encode_update_visible_state, VisibleState, HEADER_LEN, STATE_LEN,
    VISIBLE_STATE_FRAME_LEN,
};

/// The largest update push: the state and a full batch.
pub const VISIBLE_STATE_MOTION_FRAME_MAX: usize = VISIBLE_STATE_FRAME_LEN + MOTION_WIRE_MAX;

/// The update push with `motion` behind the state (an empty batch is its head alone).
#[must_use]
pub fn encode_update_visible_state_motion(
    state: VisibleState,
    motion: &MotionBatch,
) -> ([u8; VISIBLE_STATE_MOTION_FRAME_MAX], usize) {
    let mut out = [0u8; VISIBLE_STATE_MOTION_FRAME_MAX];
    out[..VISIBLE_STATE_FRAME_LEN].copy_from_slice(&encode_update_visible_state(state));
    let n = motion.encode_into(&mut out[VISIBLE_STATE_FRAME_LEN..]).unwrap_or(0);
    out[4..8].copy_from_slice(&((STATE_LEN + n) as u32).to_le_bytes());
    (out, VISIBLE_STATE_FRAME_LEN + n)
}

/// The state and its motion — `None` unless the frame is an update push whose motion exactly
/// fills it and whose header names that length (fail-closed).
#[must_use]
pub fn decode_update_visible_state_motion(frame: &[u8]) -> Option<(VisibleState, MotionBatch)> {
    let (head, rest) = frame.split_at_checked(VISIBLE_STATE_FRAME_LEN)?;
    let named = u32::from_le_bytes(frame.get(4..8)?.try_into().ok()?) as usize;
    if named != frame.len() - HEADER_LEN {
        return None;
    }
    let mut fixed = [0u8; VISIBLE_STATE_FRAME_LEN];
    fixed.copy_from_slice(head);
    fixed[4..8].copy_from_slice(&(STATE_LEN as u32).to_le_bytes());
    Some((decode_update_visible_state(&fixed)?, MotionBatch::decode(rest)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_display_proto::surface_motion::MotionSample;

    fn moved() -> (VisibleState, MotionBatch) {
        let state = VisibleState { cursor_x: 40, cursor_y: 12, ..VisibleState::default() };
        let mut motion = MotionBatch::new();
        for (i, x) in [36, 38, 40].into_iter().enumerate() {
            let t_ns = 5_000_000 + i as u64 * 1_000_000;
            motion.push(MotionSample { x, y: 12, t_ns, seq: 7 + i as u32 * 3 });
        }
        (state, motion)
    }

    #[test]
    fn a_push_carries_the_motion_since_the_last() {
        let (state, motion) = moved();
        let (frame, n) = encode_update_visible_state_motion(state, &motion);
        assert_eq!(decode_update_visible_state_motion(&frame[..n]), Some((state, motion)));
        let (still, m) = encode_update_visible_state_motion(state, &MotionBatch::new());
        assert_eq!(m, VISIBLE_STATE_FRAME_LEN + 11, "an empty batch is its head");
        assert_eq!(
            decode_update_visible_state_motion(&still[..m]),
            Some((state, MotionBatch::new()))
        );
    }

    #[test]
    fn test_reject_a_push_whose_motion_or_length_does_not_fit() {
        let (state, motion) = moved();
        let (frame, n) = encode_update_visible_state_motion(state, &motion);
        assert_eq!(decode_update_visible_state_motion(&frame[..n - 1]), None, "truncated");
        let mut lying = frame;
        lying[4..8].copy_from_slice(&(STATE_LEN as u32).to_le_bytes());
        assert_eq!(decode_update_visible_state_motion(&lying[..n]), None, "a header that lies");
        let fixed = encode_update_visible_state(state);
        assert_eq!(decode_update_visible_state_motion(&fixed), None, "no motion head at all");
    }
}
