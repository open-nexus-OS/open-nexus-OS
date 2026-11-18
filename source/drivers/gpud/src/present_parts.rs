// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A present in parts (RFC-0101 §8e, TASK-0351 P6). One IPC frame holds 8 KiB — about
//! 64 layers: with twenty windows the compositor dropped layers at that bound and the launcher
//! vanished (`board--2026-11-18T11-44-54`). A present whose body is longer arrives as
//! `OP_PRESENT_PART` frames, gathered here; the last one hands the service loop the present as
//! if it had come whole (`[OP_PRESENT_DAMAGE, seq, body]`), so everything behind it — the
//! validation, the surfaces' admission, the ack — is the one path. Pure and host-tested.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below incl. `test_reject_*` (order, seq, sender, the body's bound)
//! INVARIANTS: the gathered bytes are bounded by `PRESENT_BODY_MAX`; parts are one sender's,
//!   one seq's, in order from index 0; a last part that does not close such a run yields a
//!   present that fails validation (its ack is `STATUS_MALFORMED`, so the sender's window of
//!   presents in flight is credited) — never a present of some of the bytes.

use alloc::vec::Vec;

use nexus_display_proto::{
    OP_PRESENT_DAMAGE, OP_PRESENT_PART, PRESENT_BODY_MAX, PRESENT_HEADER_LEN,
    PRESENT_PART_HEADER_LEN, PRESENT_PART_LAST,
};

/// The parts gathered so far: whose, of which present, and the index due next.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Run {
    sender: u64,
    seq: u32,
    next: u8,
}

/// A present's parts as they arrive. The buffer is taken from the heap by the first long
/// present and kept.
#[derive(Default)]
pub(crate) struct Parts {
    /// `[OP_PRESENT_DAMAGE, seq]` and the body gathered behind it.
    whole: Vec<u8>,
    run: Option<Run>,
}

impl Parts {
    /// The frame the service loop serves for `frame` from `sender`: the frame itself unless it
    /// is a part — then nothing until the present's last part, which yields the present whole
    /// (or one that fails validation when its parts were not one run).
    pub(crate) fn take<'a>(&'a mut self, frame: &'a [u8], sender: u64) -> Option<&'a [u8]> {
        if frame.first() != Some(&OP_PRESENT_PART) {
            // Any other request ends a run that was not closed: its last part will not match.
            if frame.first() == Some(&OP_PRESENT_DAMAGE) {
                self.run = None;
            }
            return Some(frame);
        }
        let head = frame.get(..PRESENT_PART_HEADER_LEN)?;
        let seq = u32::from_le_bytes([head[1], head[2], head[3], head[4]]);
        let (index, last) = (head[5] & !PRESENT_PART_LAST, head[5] & PRESENT_PART_LAST != 0);
        let bytes = &frame[PRESENT_PART_HEADER_LEN..];
        if index == 0 {
            self.whole.clear();
            self.run = Some(Run { sender, seq, next: 0 });
        }
        let due = Run { sender, seq, next: index };
        let room = (PRESENT_HEADER_LEN + PRESENT_BODY_MAX).saturating_sub(self.whole.len());
        let fits = self.run == Some(due)
            && bytes.len() + if index == 0 { PRESENT_HEADER_LEN } else { 0 } <= room
            && self.whole.try_reserve(bytes.len() + PRESENT_HEADER_LEN).is_ok();
        if fits {
            if index == 0 {
                self.whole.push(OP_PRESENT_DAMAGE);
                self.whole.extend_from_slice(&seq.to_le_bytes());
            }
            self.whole.extend_from_slice(bytes);
            self.run = Some(Run { next: index + 1, ..due });
        } else {
            self.run = None;
        }
        if !last {
            return None;
        }
        if self.run.take().is_none() {
            // Not one run: a present of this seq whose body no decoder accepts.
            self.whole.clear();
            if self.whole.try_reserve(PRESENT_HEADER_LEN + 1).is_err() {
                return None;
            }
            self.whole.push(OP_PRESENT_DAMAGE);
            self.whole.extend_from_slice(&seq.to_le_bytes());
            self.whole.push(0);
        }
        Some(&self.whole)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_display_proto::write_present_part_header;

    const WINDOWD: u64 = 0x77;

    fn part(seq: u32, index: u8, last: bool, bytes: &[u8]) -> Vec<u8> {
        let mut f = alloc::vec![0u8; PRESENT_PART_HEADER_LEN];
        write_present_part_header(&mut f, seq, index, last);
        f.extend_from_slice(bytes);
        f
    }

    fn present(seq: u32, body: &[u8]) -> Vec<u8> {
        let mut f = alloc::vec![OP_PRESENT_DAMAGE];
        f.extend_from_slice(&seq.to_le_bytes());
        f.extend_from_slice(body);
        f
    }

    /// The present a refused run yields: its seq, and a body too short to decode.
    fn malformed(seq: u32) -> Vec<u8> {
        present(seq, &[0])
    }

    #[test]
    fn parts_in_order_are_the_present_whole_and_other_frames_pass() {
        let mut parts = Parts::default();
        let cursor = [2u8, 1, 2, 3, 4];
        assert_eq!(parts.take(&cursor, WINDOWD), Some(&cursor[..]));
        let whole = present(9, &[1, 2, 3]);
        assert_eq!(parts.take(&whole, WINDOWD), Some(&whole[..]));
        let body: Vec<u8> = (0..20_000u32).map(|i| i as u8).collect();
        assert_eq!(parts.take(&part(10, 0, false, &body[..8000]), WINDOWD), None);
        // A request between the parts is served and breaks nothing.
        assert_eq!(parts.take(&cursor, WINDOWD), Some(&cursor[..]));
        assert_eq!(parts.take(&part(10, 1, false, &body[8000..16_000]), WINDOWD), None);
        let got = parts.take(&part(10, 2, true, &body[16_000..]), WINDOWD).map(<[u8]>::to_vec);
        assert_eq!(got, Some(present(10, &body)));
        // The next long present starts over.
        assert_eq!(parts.take(&part(11, 0, false, &[5; 100]), WINDOWD), None);
        let got = parts.take(&part(11, 1, true, &[6; 50]), WINDOWD).map(<[u8]>::to_vec);
        assert_eq!(got, Some(present(11, &[[5u8; 100].as_slice(), &[6; 50]].concat())));
    }

    #[test]
    fn test_reject_a_part_out_of_order_or_of_another_present() {
        let mut parts = Parts::default();
        // The second part is missing.
        assert_eq!(parts.take(&part(4, 0, false, &[1; 10]), WINDOWD), None);
        let got = parts.take(&part(4, 2, true, &[3; 10]), WINDOWD).map(<[u8]>::to_vec);
        assert_eq!(got, Some(malformed(4)));
        // A last part without a first.
        let got = parts.take(&part(5, 1, true, &[3; 10]), WINDOWD).map(<[u8]>::to_vec);
        assert_eq!(got, Some(malformed(5)));
        // Another present's part does not join the run.
        assert_eq!(parts.take(&part(6, 0, false, &[1; 10]), WINDOWD), None);
        let got = parts.take(&part(7, 1, true, &[2; 10]), WINDOWD).map(<[u8]>::to_vec);
        assert_eq!(got, Some(malformed(7)));
        // A whole present between the parts ends the run.
        assert_eq!(parts.take(&part(8, 0, false, &[1; 10]), WINDOWD), None);
        let whole = present(8, &[9, 9]);
        assert_eq!(parts.take(&whole, WINDOWD), Some(&whole[..]));
        let got = parts.take(&part(8, 1, true, &[2; 10]), WINDOWD).map(<[u8]>::to_vec);
        assert_eq!(got, Some(malformed(8)));
    }

    #[test]
    fn test_reject_a_part_of_another_sender() {
        let mut parts = Parts::default();
        assert_eq!(parts.take(&part(4, 0, false, &[1; 10]), WINDOWD), None);
        let got = parts.take(&part(4, 1, true, &[2; 10]), WINDOWD + 1).map(<[u8]>::to_vec);
        assert_eq!(got, Some(malformed(4)), "identity is the kernel's sender id, not the seq");
    }

    #[test]
    fn test_reject_a_body_past_its_bound_and_a_part_without_a_header() {
        let mut parts = Parts::default();
        let chunk = [7u8; 8000];
        let mut index = 0u8;
        while usize::from(index + 1) * chunk.len() <= PRESENT_BODY_MAX {
            assert_eq!(parts.take(&part(3, index, false, &chunk), WINDOWD), None);
            index += 1;
        }
        // The part that would pass the bound ends the run; nothing of it is served.
        assert_eq!(parts.take(&part(3, index, false, &chunk), WINDOWD), None);
        let got = parts.take(&part(3, index + 1, true, &[1]), WINDOWD).map(<[u8]>::to_vec);
        assert_eq!(got, Some(malformed(3)));
        assert!(parts.whole.len() <= PRESENT_HEADER_LEN + PRESENT_BODY_MAX);
        // A body exactly at the bound is served.
        let body = alloc::vec![1u8; PRESENT_BODY_MAX];
        let mut sent = 0;
        for (i, piece) in body.chunks(8000).enumerate() {
            sent += piece.len();
            let frame = part(12, i as u8, sent == body.len(), piece);
            let got = parts.take(&frame, WINDOWD).map(<[u8]>::len);
            assert_eq!(got, (sent == body.len()).then_some(5 + body.len()));
        }
        // Shorter than its header: no part, no present.
        assert_eq!(parts.take(&[OP_PRESENT_PART, 1, 0, 0], WINDOWD), None);
    }
}
