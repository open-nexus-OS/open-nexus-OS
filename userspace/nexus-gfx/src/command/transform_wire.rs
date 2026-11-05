// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the `LayerTransform` record on the scene wire (RFC-0101 §8, amended by TASK-0354 P3):
//! a transform id, an offset, an opacity, a scale in 1/10000 and the origin every layer of the
//! group scales about — seven little-endian words. Split out of `buffer.rs` (module-size
//! ratchet), beside `layer_wire`.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `buffer_wire_tests.rs` (`layer_transform_roundtrips_over_the_wire`,
//!   `test_reject_a_transform_for_no_window_or_out_of_range`)

use super::buffer::Command;
use crate::core::error::GfxError;

/// The record's tag on the wire.
pub(super) const TAG_LAYER_TRANSFORM: u8 = 11;
/// A scale of one in `Command::LayerTransform`'s 1/10000 steps (a step of a 1920-pixel layer is a
/// fifth of a pixel: a scale motion grows smoothly, never in 1% jumps).
pub const SCALE_ONE: u32 = 10_000;
/// The record's payload after its tag: seven words.
const LEN: usize = 28;

/// Writes the transform at `pos` of `buf` (its tag first); the position after it.
pub(super) fn ser(
    buf: &mut [u8],
    pos: usize,
    cmd: &Command,
    max: usize,
) -> Result<usize, GfxError> {
    let Command::LayerTransform { layer_id, dx, dy, opacity, scale, origin_x, origin_y } = *cmd
    else {
        return Err(GfxError::InvalidArgument);
    };
    let end = pos + 1 + LEN;
    if buf.len() < end || end > max {
        return Err(GfxError::ResourceExhausted);
    }
    buf[pos] = TAG_LAYER_TRANSFORM;
    let words = [layer_id, dx as u32, dy as u32, opacity, scale, origin_x as u32, origin_y as u32];
    for (i, w) in words.iter().enumerate() {
        let at = pos + 1 + i * 4;
        buf[at..at + 4].copy_from_slice(&w.to_le_bytes());
    }
    Ok(end)
}

/// The transform whose payload starts at `pos` of `buf` (after its tag); the position after it.
pub(super) fn deser(buf: &[u8], pos: usize) -> Result<(Command, usize), GfxError> {
    let w = buf.get(pos..pos + LEN).ok_or(GfxError::InvalidArgument)?;
    let word = |i: usize| u32::from_le_bytes([w[i], w[i + 1], w[i + 2], w[i + 3]]);
    let cmd = Command::LayerTransform {
        layer_id: word(0),
        dx: word(4) as i32,
        dy: word(8) as i32,
        opacity: word(12),
        scale: word(16),
        origin_x: word(20) as i32,
        origin_y: word(24) as i32,
    };
    Ok((cmd, pos + LEN))
}
