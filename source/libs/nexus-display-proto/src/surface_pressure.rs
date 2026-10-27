// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `OP_SURFACE_PRESSURE` (TASK-0333 6e-3, RFC-0100 §3, ADR-0086): the compositor tells
//! an app's window how tight memory is — the level the one memory authority told it (normal,
//! warn, critical, emergency; `nexus_abi::syscall::memory::MEM_PRESSURE_*`). app-host raises it
//! into the program as the host event `MemoryPressure` (a case per level; a program without the
//! event does no work) and drops what it can rebuild. One byte, one direction (compositor →
//! client), fail-closed decode.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (round trip, short/foreign/unknown frames rejected)

use crate::client_surface::{has_op, header, HEADER_LEN};

/// The memory level, told to the receiving window.
pub const OP_SURFACE_PRESSURE: u8 = 36;

pub const SURFACE_PRESSURE_FRAME_LEN: usize = HEADER_LEN + 1;

/// The highest level this frame speaks (`MEM_PRESSURE_EMERGENCY`).
const LEVEL_MAX: u8 = 3;

#[must_use]
pub fn encode_surface_pressure(level: u8) -> [u8; SURFACE_PRESSURE_FRAME_LEN] {
    let mut f = [0u8; SURFACE_PRESSURE_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_PRESSURE));
    f[HEADER_LEN] = level;
    f
}

/// The level, or `None` for a frame that is not a well-formed pressure frame (another op, a
/// short or a long frame, a level this build does not know).
#[must_use]
pub fn decode_surface_pressure(frame: &[u8]) -> Option<u8> {
    if !has_op(frame, OP_SURFACE_PRESSURE) || frame.len() != SURFACE_PRESSURE_FRAME_LEN {
        return None;
    }
    let level = frame[HEADER_LEN];
    (level <= LEVEL_MAX).then_some(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_four_levels() {
        for level in 0..=LEVEL_MAX {
            assert_eq!(decode_surface_pressure(&encode_surface_pressure(level)), Some(level));
        }
    }

    #[test]
    fn test_reject_short_foreign_or_unknown_pressure() {
        let f = encode_surface_pressure(1);
        assert_eq!(decode_surface_pressure(&f[..HEADER_LEN]), None, "short");
        let mut long = [0u8; SURFACE_PRESSURE_FRAME_LEN + 1];
        long[..SURFACE_PRESSURE_FRAME_LEN].copy_from_slice(&f);
        assert_eq!(decode_surface_pressure(&long), None, "long");
        let mut other = f;
        other[3] = crate::surface_seen::OP_SURFACE_SEEN;
        assert_eq!(decode_surface_pressure(&other), None, "foreign op");
        assert_eq!(decode_surface_pressure(&encode_surface_pressure(4)), None, "unknown level");
    }
}
