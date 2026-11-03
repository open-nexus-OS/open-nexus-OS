// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The two frames that tell an app's window why its event channel went quiet (RFC-0112,
//! amendment 2026-11-02, TASK-0348 Phase 6). `OP_SURFACE_CLOSED`: the compositor closed the
//! window ON PURPOSE (the user, the window manager) and says so before it lets go of the channel
//! — the app saves its record and ends. `OP_COMPOSITOR_BACK`: the compositor died and a new one
//! runs — execd, the app's parent, sends it on the app's channel with a fresh SEND of that
//! channel moved along (the app gave its last one to the dead compositor); the app attaches the
//! channel again and draws. An EOF on the channel without a CLOSED before it is the compositor's
//! death. Header only, one direction each, fail-closed decode.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (round trips; other ops and long frames refused)

use crate::client_surface::{has_op, header, HEADER_LEN};

/// The compositor closed the window on purpose (compositor → app, before the channel goes).
pub const OP_SURFACE_CLOSED: u8 = 39;
/// A new compositor runs (execd → app, a fresh SEND of the app's event channel moved along).
pub const OP_COMPOSITOR_BACK: u8 = 40;

/// The apps execd told (execd → the new compositor, RFC-0112 Phase 3): the kernel ids of every
/// live app it sent `OP_COMPOSITOR_BACK` — the owners whose ghosts may wait for a rejoin. A
/// ghost of an owner not listed is of an app that is gone: it is closed.
pub const OP_APPS_ALIVE: u8 = 41;
/// Owners one `OP_APPS_ALIVE` carries at most (the frame stays within windowd's 512 bytes).
pub const APPS_ALIVE_MAX: usize = 60;
/// `OP_APPS_ALIVE`: `[hdr, count u8, owner u64 × count]`.
pub const APPS_ALIVE_HEADER_LEN: usize = HEADER_LEN + 1;

/// Both frames are the bare header.
pub const SURFACE_RESTART_FRAME_LEN: usize = HEADER_LEN;

/// Writes `OP_APPS_ALIVE` for `owners` into `out`: its length. `None` when more than
/// `APPS_ALIVE_MAX` owners or `out` is too short (the caller sends several then).
#[must_use]
pub fn encode_apps_alive(owners: &[u64], out: &mut [u8]) -> Option<usize> {
    let len = APPS_ALIVE_HEADER_LEN + owners.len() * 8;
    if owners.len() > APPS_ALIVE_MAX || out.len() < len {
        return None;
    }
    out[..HEADER_LEN].copy_from_slice(&header(OP_APPS_ALIVE));
    out[HEADER_LEN] = owners.len() as u8;
    for (i, owner) in owners.iter().enumerate() {
        let at = APPS_ALIVE_HEADER_LEN + i * 8;
        out[at..at + 8].copy_from_slice(&owner.to_le_bytes());
    }
    Some(len)
}

/// Decodes `OP_APPS_ALIVE` into `out`: the owners' count. Length-exact for its count; refuses a
/// count past `APPS_ALIVE_MAX`, an owner of 0 (the kernel names every app) and `out` too short.
#[must_use]
pub fn decode_apps_alive(frame: &[u8], out: &mut [u64]) -> Option<usize> {
    if !has_op(frame, OP_APPS_ALIVE) || frame.len() < APPS_ALIVE_HEADER_LEN {
        return None;
    }
    let count = usize::from(frame[HEADER_LEN]);
    if count > APPS_ALIVE_MAX
        || out.len() < count
        || frame.len() != APPS_ALIVE_HEADER_LEN + count * 8
    {
        return None;
    }
    for (i, slot) in out.iter_mut().take(count).enumerate() {
        let at = APPS_ALIVE_HEADER_LEN + i * 8;
        let mut b = [0u8; 8];
        b.copy_from_slice(&frame[at..at + 8]);
        *slot = u64::from_le_bytes(b);
        if *slot == 0 {
            return None;
        }
    }
    Some(count)
}

#[must_use]
pub fn encode_surface_closed() -> [u8; SURFACE_RESTART_FRAME_LEN] {
    header(OP_SURFACE_CLOSED)
}

#[must_use]
pub fn encode_compositor_back() -> [u8; SURFACE_RESTART_FRAME_LEN] {
    header(OP_COMPOSITOR_BACK)
}

/// Whether `frame` is a well-formed `OP_SURFACE_CLOSED`.
#[must_use]
pub fn is_surface_closed(frame: &[u8]) -> bool {
    has_op(frame, OP_SURFACE_CLOSED) && frame.len() == SURFACE_RESTART_FRAME_LEN
}

/// Whether `frame` is a well-formed `OP_COMPOSITOR_BACK`.
#[must_use]
pub fn is_compositor_back(frame: &[u8]) -> bool {
    has_op(frame, OP_COMPOSITOR_BACK) && frame.len() == SURFACE_RESTART_FRAME_LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_frames_round_trip() {
        assert!(is_surface_closed(&encode_surface_closed()));
        assert!(is_compositor_back(&encode_compositor_back()));
        assert!(!is_surface_closed(&encode_compositor_back()));
        assert!(!is_compositor_back(&encode_surface_closed()));
    }

    #[test]
    fn the_apps_alive_frame_round_trips_its_owners() {
        let owners = [0xA11CEu64, 0xB0B, u64::MAX];
        let mut out = [0u8; 64];
        let n = encode_apps_alive(&owners, &mut out).expect("fits");
        assert_eq!(n, APPS_ALIVE_HEADER_LEN + 24);
        let mut got = [0u64; APPS_ALIVE_MAX];
        assert_eq!(decode_apps_alive(&out[..n], &mut got), Some(3));
        assert_eq!(&got[..3], &owners);
        let n = encode_apps_alive(&[], &mut out).expect("none alive");
        assert_eq!(decode_apps_alive(&out[..n], &mut got), Some(0));
        let many = [7u64; APPS_ALIVE_MAX];
        let mut big = [0u8; 512];
        let n = encode_apps_alive(&many, &mut big).expect("the most fits the frame");
        assert_eq!(decode_apps_alive(&big[..n], &mut got), Some(APPS_ALIVE_MAX));
    }

    #[test]
    fn test_reject_an_apps_alive_frame_past_its_bound_or_naming_nobody() {
        let mut out = [0u8; 512];
        let too_many = [1u64; APPS_ALIVE_MAX + 1];
        assert!(encode_apps_alive(&too_many, &mut out).is_none(), "past the bound");
        assert!(encode_apps_alive(&[1, 2], &mut out[..12]).is_none(), "too short a buffer");
        let n = encode_apps_alive(&[0xA11CE, 0xB0B], &mut out).expect("two");
        let mut got = [0u64; APPS_ALIVE_MAX];
        assert!(decode_apps_alive(&out[..n - 1], &mut got).is_none(), "short");
        let mut long = out[..n].to_vec();
        long.push(0);
        assert!(decode_apps_alive(&long, &mut got).is_none(), "long");
        let mut zero = out[..n].to_vec();
        zero[APPS_ALIVE_HEADER_LEN..APPS_ALIVE_HEADER_LEN + 8].fill(0);
        assert!(decode_apps_alive(&zero, &mut got).is_none(), "owner 0 names nobody");
        let mut small = [0u64; 1];
        assert!(decode_apps_alive(&out[..n], &mut small).is_none(), "out too short");
        let mut count = out[..n].to_vec();
        count[HEADER_LEN] = APPS_ALIVE_MAX as u8 + 1;
        assert!(decode_apps_alive(&count, &mut got).is_none(), "count past the bound");
        assert!(decode_apps_alive(&encode_compositor_back(), &mut got).is_none(), "other op");
    }

    #[test]
    fn test_reject_a_long_or_foreign_frame() {
        let mut long = [0u8; SURFACE_RESTART_FRAME_LEN + 1];
        long[..HEADER_LEN].copy_from_slice(&encode_surface_closed());
        assert!(!is_surface_closed(&long), "trailing bytes");
        assert!(!is_compositor_back(&header(crate::surface_pressure::OP_SURFACE_PRESSURE)));
        assert!(!is_compositor_back(&[]), "empty");
    }
}
