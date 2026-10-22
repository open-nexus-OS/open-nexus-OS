// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Frame timing on the display wire (RFC-0110, TASK-0343): when a present reached the
//! glass, and the time a client's next frame is computed for. gpud's present ack carries the
//! frame boundary its flip was taken at, the display's refresh period and the cadence gpud holds
//! (a present lands every `cadence` periods while frames come); windowd predicts from them when
//! the frame it composes next is shown and names that time in the frame pulse; the app advances
//! its physics and animations to it — the motion on the glass then moves by the time between the
//! frames it shows, never by the time between the moments the app happened to run.
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Internal v1
//! TEST_COVERAGE: the goldens and `test_reject_*` below
//! INVARIANTS: an ack without the tail is the plain ack (`decode_present_ack` reads its first
//!   five bytes alike); a period outside 1–100 ms or a cadence outside 1–4 does not decode — a
//!   pacer never runs on a nonsense clock; a pulse without the tail is the plain pulse.

use crate::client_surface::{OP_SURFACE_FRAME, OP_SURFACE_FRAME_REQ};
use crate::envelope::{has_op, header, HEADER_LEN};
use crate::{encode_present_ack, PRESENT_ACK_LEN};

/// The plain pulse and its request: `[header, surface_id: u32]` (`client_surface`'s frame pulse,
/// the Choreographer contract).
pub const SURFACE_FRAME_FRAME_LEN: usize = HEADER_LEN + 4;

fn surface_frame(op: u8, surface_id: u32) -> [u8; SURFACE_FRAME_FRAME_LEN] {
    let mut f = [0u8; SURFACE_FRAME_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(op));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f
}

fn surface_id(frame: &[u8]) -> u32 {
    u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]])
}

#[must_use]
pub fn encode_surface_frame_req(surface_id: u32) -> [u8; SURFACE_FRAME_FRAME_LEN] {
    surface_frame(OP_SURFACE_FRAME_REQ, surface_id)
}

#[must_use]
pub fn decode_surface_frame_req(frame: &[u8]) -> Option<u32> {
    (has_op(frame, OP_SURFACE_FRAME_REQ) && frame.len() == SURFACE_FRAME_FRAME_LEN)
        .then(|| surface_id(frame))
}

#[must_use]
pub fn encode_surface_frame(surface_id: u32) -> [u8; SURFACE_FRAME_FRAME_LEN] {
    surface_frame(OP_SURFACE_FRAME, surface_id)
}

/// The pulse's surface — a plain pulse or a timed one (`decode_surface_frame_at` reads its time).
#[must_use]
pub fn decode_surface_frame(frame: &[u8]) -> Option<u32> {
    let len_ok = matches!(frame.len(), SURFACE_FRAME_FRAME_LEN | SURFACE_FRAME_AT_LEN);
    (has_op(frame, OP_SURFACE_FRAME) && len_ok).then(|| surface_id(frame))
}

/// The shortest and the longest refresh period a timing names (1 kHz … 10 Hz).
pub const PERIOD_MIN_NS: u32 = 1_000_000;
pub const PERIOD_MAX_NS: u32 = 100_000_000;
/// The longest cadence: a present every four periods.
pub const CADENCE_MAX: u8 = 4;

/// When a present reached the glass: the frame boundary its flip was taken at (the `nsec`
/// clock), the display's refresh period, and the cadence the driver holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub at_ns: u64,
    pub period_ns: u32,
    pub cadence: u8,
}

impl Shown {
    /// The time between two frames on the glass at this cadence.
    #[must_use]
    pub const fn step_ns(&self) -> u64 {
        self.period_ns as u64 * self.cadence as u64
    }

    fn valid(&self) -> bool {
        (PERIOD_MIN_NS..=PERIOD_MAX_NS).contains(&self.period_ns)
            && (1..=CADENCE_MAX).contains(&self.cadence)
    }
}

/// A present ack with its frame's time: `[status, seq: u32, at_ns: u64, period_ns: u32,
/// cadence: u8]` (little-endian).
pub const PRESENT_ACK_SHOWN_LEN: usize = PRESENT_ACK_LEN + 13;

/// Encodes a present ack that names when its frame reached the glass.
#[must_use]
pub fn encode_present_ack_shown(status: u8, seq: u32, shown: Shown) -> [u8; PRESENT_ACK_SHOWN_LEN] {
    let mut f = [0u8; PRESENT_ACK_SHOWN_LEN];
    f[..PRESENT_ACK_LEN].copy_from_slice(&encode_present_ack(status, seq));
    f[5..13].copy_from_slice(&shown.at_ns.to_le_bytes());
    f[13..17].copy_from_slice(&shown.period_ns.to_le_bytes());
    f[17] = shown.cadence;
    f
}

/// The frame's time a present ack carries; `None` for a plain ack, or a tail that names no sane
/// clock.
#[must_use]
pub fn decode_present_ack_shown(frame: &[u8]) -> Option<Shown> {
    if frame.len() != PRESENT_ACK_SHOWN_LEN {
        return None;
    }
    let mut at = [0u8; 8];
    at.copy_from_slice(&frame[5..13]);
    let shown = Shown {
        at_ns: u64::from_le_bytes(at),
        period_ns: u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]),
        cadence: frame[17],
    };
    shown.valid().then_some(shown)
}

/// The frame pulse with the time its frame is computed for: the plain pulse, then `target_ns:
/// u64` (when the client's next frame is shown, the `nsec` clock) and `step_ns: u32` (the time
/// between frames on the glass).
pub const SURFACE_FRAME_AT_LEN: usize = SURFACE_FRAME_FRAME_LEN + 12;

/// The time a frame pulse names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameAt {
    pub surface_id: u32,
    pub target_ns: u64,
    pub step_ns: u32,
}

/// Encodes a frame pulse that names the time its frame is shown.
#[must_use]
pub fn encode_surface_frame_at(at: FrameAt) -> [u8; SURFACE_FRAME_AT_LEN] {
    let mut f = [0u8; SURFACE_FRAME_AT_LEN];
    let o = SURFACE_FRAME_FRAME_LEN;
    f[..o].copy_from_slice(&encode_surface_frame(at.surface_id));
    f[o..o + 8].copy_from_slice(&at.target_ns.to_le_bytes());
    f[o + 8..o + 12].copy_from_slice(&at.step_ns.to_le_bytes());
    f
}

/// The time a frame pulse names; `None` for a plain pulse, or a step no display has.
#[must_use]
pub fn decode_surface_frame_at(frame: &[u8]) -> Option<FrameAt> {
    if frame.len() != SURFACE_FRAME_AT_LEN {
        return None;
    }
    let surface_id = decode_surface_frame(frame)?;
    let o = SURFACE_FRAME_FRAME_LEN;
    let mut target = [0u8; 8];
    target.copy_from_slice(&frame[o..o + 8]);
    let step_ns = u32::from_le_bytes([frame[o + 8], frame[o + 9], frame[o + 10], frame[o + 11]]);
    let steps = u64::from(PERIOD_MIN_NS)..=u64::from(PERIOD_MAX_NS) * u64::from(CADENCE_MAX);
    steps.contains(&u64::from(step_ns)).then_some(FrameAt {
        surface_id,
        target_ns: u64::from_le_bytes(target),
        step_ns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_present_ack;

    const SIXTY: Shown = Shown { at_ns: 0x0102_0304_0506_0708, period_ns: 16_666_667, cadence: 1 };

    #[test]
    fn a_shown_ack_is_the_plain_ack_and_its_frame_time() {
        let f = encode_present_ack_shown(0, 7, SIXTY);
        assert_eq!(
            f,
            [
                0, 7, 0, 0, 0, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x2b, 0x50, 0xfe,
                0x00, 1
            ]
        );
        assert_eq!(decode_present_ack(&f), Some((0, 7)), "the plain decoder reads its head");
        assert_eq!(decode_present_ack_shown(&f), Some(SIXTY));
        assert_eq!(SIXTY.step_ns(), 16_666_667);
        assert_eq!(Shown { cadence: 2, ..SIXTY }.step_ns(), 33_333_334);
    }

    #[test]
    fn test_reject_a_plain_ack_or_a_nonsense_clock_as_a_frame_time() {
        assert_eq!(decode_present_ack_shown(&encode_present_ack(0, 7)), None);
        for bad in [
            Shown { period_ns: 0, ..SIXTY },
            Shown { period_ns: PERIOD_MAX_NS + 1, ..SIXTY },
            Shown { cadence: 0, ..SIXTY },
            Shown { cadence: CADENCE_MAX + 1, ..SIXTY },
        ] {
            assert_eq!(decode_present_ack_shown(&encode_present_ack_shown(0, 7, bad)), None);
        }
        let f = encode_present_ack_shown(0, 7, SIXTY);
        assert_eq!(decode_present_ack_shown(&f[..PRESENT_ACK_SHOWN_LEN - 1]), None);
    }

    #[test]
    fn a_timed_pulse_is_the_plain_pulse_and_the_time_its_frame_is_shown() {
        let at = FrameAt { surface_id: 5, target_ns: 1_000_000_123, step_ns: 16_666_667 };
        let f = encode_surface_frame_at(at);
        assert_eq!(f.len(), SURFACE_FRAME_AT_LEN);
        assert_eq!(f[..SURFACE_FRAME_FRAME_LEN], encode_surface_frame(5));
        assert_eq!(decode_surface_frame(&f), Some(5), "the plain decoder reads its head");
        assert_eq!(decode_surface_frame_at(&f), Some(at));
    }

    #[test]
    fn test_reject_a_plain_pulse_or_a_step_no_display_has_as_a_frame_time() {
        assert_eq!(decode_surface_frame_at(&encode_surface_frame(5)), None);
        for step_ns in [0, PERIOD_MIN_NS - 1, PERIOD_MAX_NS * u32::from(CADENCE_MAX) + 1] {
            let f = encode_surface_frame_at(FrameAt { surface_id: 5, target_ns: 1, step_ns });
            assert_eq!(decode_surface_frame_at(&f), None, "step {step_ns}");
        }
        let mut f =
            encode_surface_frame_at(FrameAt { surface_id: 5, target_ns: 1, step_ns: 16_666_667 });
        f[3] ^= 0xff;
        assert_eq!(decode_surface_frame_at(&f), None, "another op");
    }
}
