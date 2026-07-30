// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0086 shell taskbar window feed + taskbar verbs. Two
//! append-only ops in the `'I','N'` envelope space (`client_surface` owns
//! the numbering; 27/28 are the next free after the RFC-0083 snapshot's 26):
//!
//! * `OP_SURFACE_WINDOWS = 27` — windowd → the DESKTOP surface's event
//!   channel only: the running app-window set, one entry per open window
//!   (`owner_sid` + minimized/focused flags). Retained latest-wins state —
//!   windowd re-sends on every window-set mutation and on desktop (re)bind;
//!   receivers replace their whole cached set per frame.
//! * `OP_SURFACE_TASKBAR = 28` — the desktop-surface owner (the shell) →
//!   windowd: ACTIVATE (restore-or-raise) a window by its owner sid.
//!   Authority = the kernel sender id matching the captured desktop owner
//!   (`windowd::control_gate`), NEVER these payload bytes.
//!
//! windowd stays app-agnostic: entries carry kernel service ids, not bundle
//! ids — the sid↔app-id join happens in the shell's app-host via
//! `service_id_from_name("app:" + id)`.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0313 Track B)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (roundtrip + reject matrix)
//! RFC: docs/rfcs/RFC-0086-shell-taskbar-window-feed.md

use crate::client_surface::{has_op, header, HEADER_LEN};

/// windowd → desktop surface: the running window set (RFC-0086).
pub const OP_SURFACE_WINDOWS: u8 = 27;
/// shell → windowd: act on a window by owner sid (RFC-0086).
pub const OP_SURFACE_TASKBAR: u8 = 28;

/// The compositor's floating-window slot count bounds the feed. Kept as a
/// LOCAL bound (not an import) so the wire format is self-describing; the
/// encoder refuses larger sets rather than truncating silently.
pub const WINDOWS_MAX: usize = 8;

/// `flags` bit: the window is minimized (still open — activate restores it).
pub const WINDOW_FLAG_MINIMIZED: u8 = 1 << 0;
/// `flags` bit: the window is the focused/topmost floating window.
pub const WINDOW_FLAG_FOCUSED: u8 = 1 << 1;

/// One feed entry: a window owned by the app-host with kernel service id
/// `owner_sid` (execd's `app:<bundle_id>` naming).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WindowEntry {
    pub owner_sid: u64,
    pub flags: u8,
}

pub const SURFACE_WINDOWS_FRAME_MAX: usize = HEADER_LEN + 1 + WINDOWS_MAX * 9;

/// Encodes the window set. `None` when the set exceeds [`WINDOWS_MAX`] —
/// never a silent truncation. Layout:
/// `[hdr:4][count][count × (owner_sid:u64 LE, flags:u8)]`.
#[must_use]
pub fn encode_surface_windows(
    windows: &[WindowEntry],
) -> Option<([u8; SURFACE_WINDOWS_FRAME_MAX], usize)> {
    if windows.len() > WINDOWS_MAX {
        return None;
    }
    let mut f = [0u8; SURFACE_WINDOWS_FRAME_MAX];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_WINDOWS));
    f[HEADER_LEN] = windows.len() as u8;
    let mut n = HEADER_LEN + 1;
    for w in windows {
        f[n..n + 8].copy_from_slice(&w.owner_sid.to_le_bytes());
        f[n + 8] = w.flags;
        n += 9;
    }
    Some((f, n))
}

/// Fail-closed decode: exact length for the declared count, count bounded.
/// Entries land in the caller's fixed buffer; returns the entry count.
#[must_use]
pub fn decode_surface_windows(frame: &[u8], out: &mut [WindowEntry; WINDOWS_MAX]) -> Option<usize> {
    if !has_op(frame, OP_SURFACE_WINDOWS) || frame.len() < HEADER_LEN + 1 {
        return None;
    }
    let count = frame[HEADER_LEN] as usize;
    if count > WINDOWS_MAX || frame.len() != HEADER_LEN + 1 + count * 9 {
        return None;
    }
    for (i, slot) in out.iter_mut().take(count).enumerate() {
        let n = HEADER_LEN + 1 + i * 9;
        let mut sid = [0u8; 8];
        sid.copy_from_slice(&frame[n..n + 8]);
        *slot = WindowEntry { owner_sid: u64::from_le_bytes(sid), flags: frame[n + 8] };
    }
    Some(count)
}

/// Taskbar verb: restore the window if minimized, else raise + focus it.
pub const TASKBAR_ACTIVATE: u8 = 0;
/// Reserved (RFC-0086): minimize the app's focused window (click-toggle).
pub const TASKBAR_MINIMIZE: u8 = 1;

pub const SURFACE_TASKBAR_FRAME_LEN: usize = HEADER_LEN + 1 + 8;

/// Encodes a taskbar verb. Layout: `[hdr:4][verb][target_sid:u64 LE]`.
#[must_use]
pub fn encode_surface_taskbar(verb: u8, target_sid: u64) -> [u8; SURFACE_TASKBAR_FRAME_LEN] {
    let mut f = [0u8; SURFACE_TASKBAR_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_TASKBAR));
    f[HEADER_LEN] = verb;
    f[HEADER_LEN + 1..].copy_from_slice(&target_sid.to_le_bytes());
    f
}

/// Fail-closed decode: exact length, known verb. Returns `(verb, target_sid)`.
#[must_use]
pub fn decode_surface_taskbar(frame: &[u8]) -> Option<(u8, u64)> {
    if !has_op(frame, OP_SURFACE_TASKBAR) || frame.len() != SURFACE_TASKBAR_FRAME_LEN {
        return None;
    }
    let verb = frame[HEADER_LEN];
    if verb != TASKBAR_ACTIVATE && verb != TASKBAR_MINIMIZE {
        return None;
    }
    let mut sid = [0u8; 8];
    sid.copy_from_slice(&frame[HEADER_LEN + 1..]);
    Some((verb, u64::from_le_bytes(sid)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_roundtrip() {
        let set = [
            WindowEntry { owner_sid: 0xAA55_1234_5678_9ABC, flags: WINDOW_FLAG_MINIMIZED },
            WindowEntry { owner_sid: 7, flags: WINDOW_FLAG_FOCUSED },
        ];
        let (f, n) = encode_surface_windows(&set).expect("encodes");
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        let count = decode_surface_windows(&f[..n], &mut out).expect("decodes");
        assert_eq!(count, 2);
        assert_eq!(&out[..2], &set);
    }

    #[test]
    fn windows_empty_set_roundtrips() {
        let (f, n) = encode_surface_windows(&[]).expect("encodes");
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        assert_eq!(decode_surface_windows(&f[..n], &mut out), Some(0));
    }

    #[test]
    fn windows_encode_refuses_oversized_set_no_truncation() {
        let set = [WindowEntry { owner_sid: 1, flags: 0 }; WINDOWS_MAX + 1];
        assert!(encode_surface_windows(&set).is_none());
    }

    #[test]
    fn windows_decode_rejects_length_mismatch_and_count_lies() {
        let set = [WindowEntry { owner_sid: 1, flags: 0 }];
        let (f, n) = encode_surface_windows(&set).expect("encodes");
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        assert!(decode_surface_windows(&f[..n - 1], &mut out).is_none(), "short");
        let mut lying = f;
        lying[HEADER_LEN] = 2; // claims two entries, carries one
        assert!(decode_surface_windows(&lying[..n], &mut out).is_none(), "count lie");
        let mut oversized = [0u8; SURFACE_WINDOWS_FRAME_MAX];
        oversized[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_WINDOWS));
        oversized[HEADER_LEN] = (WINDOWS_MAX + 1) as u8;
        assert!(decode_surface_windows(&oversized, &mut out).is_none(), "count bound");
    }

    #[test]
    fn taskbar_roundtrip_and_rejects() {
        let f = encode_surface_taskbar(TASKBAR_ACTIVATE, 0xDEAD_BEEF);
        assert_eq!(decode_surface_taskbar(&f), Some((TASKBAR_ACTIVATE, 0xDEAD_BEEF)));
        assert!(decode_surface_taskbar(&f[..f.len() - 1]).is_none(), "short");
        let mut bad = f;
        bad[HEADER_LEN] = 9; // unknown verb
        assert!(decode_surface_taskbar(&bad).is_none(), "unknown verb");
    }
}
