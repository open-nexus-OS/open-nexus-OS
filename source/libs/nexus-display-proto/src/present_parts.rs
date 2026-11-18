// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A present in parts, the sender's side (RFC-0101 §8e, TASK-0351 P6): a frame of
//! many windows is longer than one IPC frame and leaves as `OP_PRESENT_PART` frames, in
//! order, the last one flagged — gpud serves it once it is whole (its `present_parts`) and
//! acks that one. The parts leave from where the body lies: each header is written over the
//! bytes in front of its part (the head room first, then the part already sent), so there is
//! no copy and no buffer on the sender's stack. Pure; the sender gives it its queue's send.
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `mod tests` below; gpud's `present_parts` (the two sides together, a scene
//!   of sixty-four windows through the planner)

use crate::{write_present_part_header, PRESENT_PART_HEADER_LEN as HEAD};

/// Sends `wire` — a part header's room and the present's body behind it — as the parts of
/// present `seq`, none longer than `frame_max`. A part `send` refuses ends the run with its
/// error: the present is not issued, and the next attempt builds the frame again and starts
/// at part 0 (which makes the receiver drop what it gathered).
pub fn send<E>(
    wire: &mut [u8],
    seq: u32,
    frame_max: usize,
    mut send: impl FnMut(&[u8]) -> Result<(), E>,
) -> Result<(), E> {
    let room = frame_max.saturating_sub(HEAD).max(1);
    let body = wire.len().saturating_sub(HEAD);
    let count = body.div_ceil(room);
    for i in 0..count {
        let (at, end) = (i * room, ((i + 1) * room).min(body));
        write_present_part_header(&mut wire[at..], seq, i as u8, i + 1 == count);
        send(&wire[at..HEAD + end])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OP_PRESENT_PART, PRESENT_BODY_MAX, PRESENT_PART_LAST};

    const FRAME_MAX: usize = 8 * 1024;

    fn wire_of(body: &[u8]) -> Vec<u8> {
        let mut wire = vec![0u8; HEAD];
        wire.extend_from_slice(body);
        wire
    }

    /// The parts carry the body whole and in order, each within one IPC frame, each naming
    /// the present and its place, the last one flagged.
    #[test]
    fn the_parts_are_the_body_in_order_each_within_a_frame() {
        for len in [1, FRAME_MAX - HEAD, FRAME_MAX - HEAD + 1, 21_760, PRESENT_BODY_MAX] {
            let body: Vec<u8> = (0..len).map(|i| (i * 7 + i / 251) as u8).collect();
            let mut wire = wire_of(&body);
            let mut got = Vec::new();
            let mut frames = 0u8;
            let sent: Result<(), ()> = send(&mut wire, 0x0102_0304, FRAME_MAX, |f| {
                assert!(f.len() <= FRAME_MAX && f.len() > HEAD, "{}", f.len());
                assert_eq!(f[..5], [OP_PRESENT_PART, 4, 3, 2, 1]);
                assert_eq!(f[5] & !PRESENT_PART_LAST, frames);
                got.extend_from_slice(&f[HEAD..]);
                frames += 1;
                // Only the last part is flagged.
                assert_eq!(f[5] & PRESENT_PART_LAST != 0, got.len() == len);
                Ok(())
            });
            assert_eq!(sent, Ok(()));
            assert_eq!(got, body, "len={len}");
            assert_eq!(usize::from(frames), len.div_ceil(FRAME_MAX - HEAD));
        }
    }

    /// A part the queue refuses ends the run at once, with the queue's word.
    #[test]
    fn a_refused_part_ends_the_run() {
        let mut wire = wire_of(&[9u8; 20_000]);
        let mut calls = 0;
        let sent = send(&mut wire, 1, FRAME_MAX, |_| {
            calls += 1;
            if calls == 2 {
                Err("queue full")
            } else {
                Ok(())
            }
        });
        assert_eq!(sent, Err("queue full"));
        assert_eq!(calls, 2);
    }
}
