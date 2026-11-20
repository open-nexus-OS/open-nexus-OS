// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud's DRAWN notice — a present's reads of client slots are done (RFC-0114 §5,
//! amended 2026-11-20, TASK-0355 P2). On a display with a ring a present lands a period or more
//! after gpud's last pass waited; windowd releases a client's slots at the read floor (every frame
//! up to it drawn or acked) instead of at the landing. The notice is its own 5-byte frame
//! `[STATUS_DRAWN, seq]`; `decode_present_ack` refuses it, so no reader can credit it as an ack —
//! the ack stays the frame clock and the in-flight gate.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable (wire)
//! TEST_COVERAGE: below (round trip, the ack decoder's refusal, the length)

use crate::{PRESENT_ACK_LEN, STATUS_DRAWN};

/// The notice that present `seq`'s reads are done: `[STATUS_DRAWN, seq]`.
#[must_use]
pub fn encode_present_drawn(seq: u32) -> [u8; PRESENT_ACK_LEN] {
    let mut f = [0u8; PRESENT_ACK_LEN];
    f[0] = STATUS_DRAWN;
    f[1..5].copy_from_slice(&seq.to_le_bytes());
    f
}

/// The seq of a drawn notice; `None` for any other frame (another status or another length).
#[must_use]
pub fn decode_present_drawn(frame: &[u8]) -> Option<u32> {
    if frame.len() != PRESENT_ACK_LEN || frame[0] != STATUS_DRAWN {
        return None;
    }
    Some(u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_present_ack, encode_present_ack, STATUS_OK};

    #[test]
    fn present_drawn_roundtrips() {
        assert_eq!(decode_present_drawn(&encode_present_drawn(0x0102_0304)), Some(0x0102_0304));
        assert_eq!(decode_present_drawn(&encode_present_ack(STATUS_OK, 7)), None, "an ack");
    }

    #[test]
    fn test_reject_a_drawn_notice_read_as_an_ack() {
        assert_eq!(decode_present_ack(&encode_present_drawn(9)), None);
    }

    #[test]
    fn test_reject_a_drawn_notice_of_another_length() {
        let f = encode_present_drawn(9);
        assert_eq!(decode_present_drawn(&f[..4]), None);
        let mut long = [0u8; PRESENT_ACK_LEN + 1];
        long[..PRESENT_ACK_LEN].copy_from_slice(&f);
        assert_eq!(decode_present_drawn(&long), None);
    }
}
