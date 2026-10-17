// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a client's GPU context and its pipeline library (RFC-0101 §1, §7, §7b; TASK-0215
//! 5c-3 P3), windowd → gpud. `[OP_GPU_CAPABILITIES]` asks what the device runs; the answer is
//! `[status, OP_GPU_CAPABILITIES, max_contexts, 0, features: u32, code_target: u64]` — a device
//! that runs no GPU code answers [`STATUS_UNSUPPORTED`](crate::STATUS_UNSUPPORTED). Then
//! `[OP_GPU_CONTEXT_OPEN, priority, flags, 0, library_len: u32]` with the library moved in: a VMO
//! holding `library_len` bytes of the archive for the device's code target. gpud opens a context
//! for the sender — identity is the kernel's `sender_service_id`, never a payload field —
//! validates the library, places its programs and answers `[status, OP_GPU_CONTEXT_OPEN, 0, 0,
//! ctx: u32, accepted: u16, refused: u16, first_refused: u32]`. An open by a client that holds a
//! context replaces it whole: a restarted client gets a fresh one. Both answers are sixteen
//! bytes with the op in their second byte, a shape windowd never reads as a cursor status (one
//! byte), a readback answer (two) or a present ack (five).
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: unit tests below

/// windowd → gpud: what the device runs.
pub const OP_GPU_CAPABILITIES: u8 = 16;
/// windowd → gpud: a context for the sender, with its library moved in.
pub const OP_GPU_CONTEXT_OPEN: u8 = 17;
/// Request lengths.
pub const CAPABILITIES_FRAME_LEN: usize = 1;
pub const OPEN_FRAME_LEN: usize = 8;
/// Both answers' length.
pub const REPLY_LEN: usize = 16;
/// The largest library an open carries: the archive format's own bound.
pub const LIBRARY_MAX_BYTES: u32 = 1 << 20;

/// A context's priority (RFC-0101 §6), as the device orders them.
pub const PRIORITY_REALTIME: u8 = 0;
pub const PRIORITY_HIGH: u8 = 1;
pub const PRIORITY_NORMAL: u8 = 2;
pub const PRIORITY_LOW: u8 = 3;

/// What the device runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuCapabilities {
    pub status: u8,
    /// Contexts the device holds at once.
    pub max_contexts: u8,
    /// The feature bits beyond the archive format's baseline the device runs.
    pub features: u32,
    /// The code target a library must be built for (0 when the device runs no GPU code).
    pub code_target: u64,
}

/// An open request: the priority asked and the library's length in the moved VMO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextOpen {
    pub priority: u8,
    pub library_len: u32,
}

/// An open's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextOpened {
    pub status: u8,
    /// The context, as the device numbers it (0 when none was opened).
    pub ctx: u32,
    /// The library's entries the device took and refused.
    pub accepted: u16,
    pub refused: u16,
    /// The first refused entry's id (0 when none was refused).
    pub first_refused: u32,
}

/// Encodes the capabilities request.
#[must_use]
pub const fn encode_capabilities_request() -> [u8; CAPABILITIES_FRAME_LEN] {
    [OP_GPU_CAPABILITIES]
}

/// Whether `frame` is a capabilities request (exact length).
#[must_use]
pub fn decode_capabilities_request(frame: &[u8]) -> bool {
    frame == [OP_GPU_CAPABILITIES]
}

/// Encodes the capabilities answer.
#[must_use]
pub fn encode_capabilities_reply(caps: &GpuCapabilities) -> [u8; REPLY_LEN] {
    let mut f = [0u8; REPLY_LEN];
    f[0] = caps.status;
    f[1] = OP_GPU_CAPABILITIES;
    f[2] = caps.max_contexts;
    f[4..8].copy_from_slice(&caps.features.to_le_bytes());
    f[8..16].copy_from_slice(&caps.code_target.to_le_bytes());
    f
}

/// Fail-closed decode of the capabilities answer: exact length, its op, the reserved byte zero.
#[must_use]
pub fn decode_capabilities_reply(reply: &[u8]) -> Option<GpuCapabilities> {
    if reply.len() != REPLY_LEN || reply[1] != OP_GPU_CAPABILITIES || reply[3] != 0 {
        return None;
    }
    Some(GpuCapabilities {
        status: reply[0],
        max_contexts: reply[2],
        features: u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]),
        code_target: u64::from_le_bytes([
            reply[8], reply[9], reply[10], reply[11], reply[12], reply[13], reply[14], reply[15],
        ]),
    })
}

/// Encodes an open request (the library's VMO is moved beside the frame).
#[must_use]
pub fn encode_open(req: &ContextOpen) -> [u8; OPEN_FRAME_LEN] {
    let mut f = [0u8; OPEN_FRAME_LEN];
    f[0] = OP_GPU_CONTEXT_OPEN;
    f[1] = req.priority;
    f[4..8].copy_from_slice(&req.library_len.to_le_bytes());
    f
}

/// Fail-closed decode of an open request: exact length, a known priority, no flag bits (none
/// is defined yet), the reserved byte zero, a library of at least one and at most
/// [`LIBRARY_MAX_BYTES`] bytes.
#[must_use]
pub fn decode_open(frame: &[u8]) -> Option<ContextOpen> {
    if frame.len() != OPEN_FRAME_LEN || frame[0] != OP_GPU_CONTEXT_OPEN {
        return None;
    }
    let (priority, flags, reserved) = (frame[1], frame[2], frame[3]);
    if priority > PRIORITY_LOW || flags != 0 || reserved != 0 {
        return None;
    }
    let library_len = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    if library_len == 0 || library_len > LIBRARY_MAX_BYTES {
        return None;
    }
    Some(ContextOpen { priority, library_len })
}

/// Encodes an open's answer.
#[must_use]
pub fn encode_open_reply(answer: &ContextOpened) -> [u8; REPLY_LEN] {
    let mut f = [0u8; REPLY_LEN];
    f[0] = answer.status;
    f[1] = OP_GPU_CONTEXT_OPEN;
    f[4..8].copy_from_slice(&answer.ctx.to_le_bytes());
    f[8..10].copy_from_slice(&answer.accepted.to_le_bytes());
    f[10..12].copy_from_slice(&answer.refused.to_le_bytes());
    f[12..16].copy_from_slice(&answer.first_refused.to_le_bytes());
    f
}

/// Fail-closed decode of an open's answer: exact length, its op, the reserved bytes zero.
#[must_use]
pub fn decode_open_reply(reply: &[u8]) -> Option<ContextOpened> {
    if reply.len() != REPLY_LEN || reply[1] != OP_GPU_CONTEXT_OPEN || reply[2..4] != [0, 0] {
        return None;
    }
    Some(ContextOpened {
        status: reply[0],
        ctx: u32::from_le_bytes([reply[4], reply[5], reply[6], reply[7]]),
        accepted: u16::from_le_bytes([reply[8], reply[9]]),
        refused: u16::from_le_bytes([reply[10], reply[11]]),
        first_refused: u32::from_le_bytes([reply[12], reply[13], reply[14], reply[15]]),
    })
}

/// Whether `reply` has the shape of either answer (its length and an op of this module in its
/// second byte) — what a reader that drains replies tells apart from a present ack.
#[must_use]
pub fn is_gpu_context_reply(reply: &[u8]) -> bool {
    reply.len() == REPLY_LEN && matches!(reply[1], OP_GPU_CAPABILITIES | OP_GPU_CONTEXT_OPEN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{STATUS_DENIED, STATUS_OK, STATUS_UNSUPPORTED};

    #[test]
    fn requests_and_replies_round_trip_golden() {
        assert_eq!(encode_capabilities_request(), [16]);
        assert!(decode_capabilities_request(&[16]));
        let caps = GpuCapabilities {
            status: STATUS_OK,
            max_contexts: 8,
            features: 1,
            code_target: 0x0024_001D_0034_00B6,
        };
        let bytes = encode_capabilities_reply(&caps);
        assert_eq!(
            bytes,
            [0, 16, 8, 0, 1, 0, 0, 0, 0xB6, 0x00, 0x34, 0x00, 0x1D, 0x00, 0x24, 0x00],
            "the capabilities answer's bytes"
        );
        assert_eq!(decode_capabilities_reply(&bytes), Some(caps));

        let open = ContextOpen { priority: PRIORITY_REALTIME, library_len: 4748 };
        let frame = encode_open(&open);
        assert_eq!(frame, [17, 0, 0, 0, 0x8C, 0x12, 0, 0], "the open request's bytes");
        assert_eq!(decode_open(&frame), Some(open));

        let answer = ContextOpened {
            status: STATUS_OK,
            ctx: 0x301,
            accepted: 7,
            refused: 0,
            first_refused: 0,
        };
        let bytes = encode_open_reply(&answer);
        assert_eq!(bytes, [0, 17, 0, 0, 0x01, 0x03, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(decode_open_reply(&bytes), Some(answer));
        assert!(is_gpu_context_reply(&bytes));
        assert!(is_gpu_context_reply(&encode_capabilities_reply(&caps)));
    }

    #[test]
    fn a_device_without_gpu_code_and_a_refusal_keep_their_shape() {
        let none = GpuCapabilities {
            status: STATUS_UNSUPPORTED,
            max_contexts: 0,
            features: 0,
            code_target: 0,
        };
        assert_eq!(decode_capabilities_reply(&encode_capabilities_reply(&none)), Some(none));
        let denied = ContextOpened {
            status: STATUS_DENIED,
            ctx: 0,
            accepted: 0,
            refused: 0,
            first_refused: 0,
        };
        assert_eq!(decode_open_reply(&encode_open_reply(&denied)), Some(denied));
    }

    #[test]
    fn test_reject_open_frames_of_unknown_flags_priorities_lengths_and_bounds() {
        let good = encode_open(&ContextOpen { priority: PRIORITY_HIGH, library_len: 64 });
        for (bad, why) in [
            (
                {
                    let mut f = good;
                    f[2] = 1;
                    f
                },
                "a flag bit none defines",
            ),
            (
                {
                    let mut f = good;
                    f[3] = 1;
                    f
                },
                "the reserved byte",
            ),
            (
                {
                    let mut f = good;
                    f[1] = PRIORITY_LOW + 1;
                    f
                },
                "an unknown priority",
            ),
            (
                {
                    let mut f = good;
                    f[4..8].copy_from_slice(&0u32.to_le_bytes());
                    f
                },
                "no library",
            ),
            (
                {
                    let mut f = good;
                    f[4..8].copy_from_slice(&(LIBRARY_MAX_BYTES + 1).to_le_bytes());
                    f
                },
                "a library past the bound",
            ),
            (
                {
                    let mut f = good;
                    f[0] = OP_GPU_CAPABILITIES;
                    f
                },
                "another op",
            ),
        ] {
            assert_eq!(decode_open(&bad), None, "{why}");
        }
        assert_eq!(decode_open(&good[..7]), None, "truncated");
        let mut long = [0u8; OPEN_FRAME_LEN + 1];
        long[..OPEN_FRAME_LEN].copy_from_slice(&good);
        assert_eq!(decode_open(&long), None, "a byte too many");
        let max =
            encode_open(&ContextOpen { priority: PRIORITY_LOW, library_len: LIBRARY_MAX_BYTES });
        assert!(decode_open(&max).is_some(), "the bound itself is taken");
    }

    #[test]
    fn test_reject_capabilities_frames_and_replies_of_another_shape() {
        assert!(!decode_capabilities_request(&[]), "empty");
        assert!(!decode_capabilities_request(&[16, 0]), "a byte too many");
        assert!(!decode_capabilities_request(&[17]), "another op");
        let caps = encode_capabilities_reply(&GpuCapabilities {
            status: STATUS_OK,
            max_contexts: 8,
            features: 0,
            code_target: 7,
        });
        assert_eq!(decode_capabilities_reply(&caps[..15]), None, "truncated");
        let mut reserved = caps;
        reserved[3] = 1;
        assert_eq!(decode_capabilities_reply(&reserved), None, "the reserved byte");
        let opened = encode_open_reply(&ContextOpened {
            status: STATUS_OK,
            ctx: 1,
            accepted: 1,
            refused: 0,
            first_refused: 0,
        });
        assert_eq!(decode_capabilities_reply(&opened), None, "an open's answer is not this one");
        assert_eq!(decode_open_reply(&caps), None, "nor the other way round");
        let mut reserved = opened;
        reserved[2] = 1;
        assert_eq!(decode_open_reply(&reserved), None, "the open answer's reserved bytes");
        assert!(!is_gpu_context_reply(&[0, 16, 0, 0, 0]), "a present ack's length");
        assert!(!is_gpu_context_reply(&[0, 15]), "a readback answer");
        let mut foreign = opened;
        foreign[1] = 4;
        assert!(!is_gpu_context_reply(&foreign), "sixteen bytes of another op");
    }
}
