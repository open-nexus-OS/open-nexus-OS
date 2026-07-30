// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0086 window-feed derivation — the pure "which windows does
//! the shell see" decision, host-tested (`compositor/` is OS glue). windowd
//! stays app-agnostic: entries carry the owning app-host's KERNEL service id
//! (execd's `app:<bundle_id>` naming), never a bundle-id string — the shell's
//! app-host does the sid↔app-id join.
//!
//! A slot without identity (`owner_sid == 0`) is OMITTED: the shell could not
//! address it anyway (every taskbar verb resolves by sid), and publishing an
//! un-addressable window would render a tile whose click does nothing.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below.

use nexus_display_proto::surface_windows::{
    WindowEntry, WINDOWS_MAX, WINDOW_FLAG_FOCUSED, WINDOW_FLAG_MINIMIZED,
};

/// One compositor app slot, reduced to what the feed cares about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotInfo {
    /// Kernel service id of the owning app-host (0 = never captured).
    pub owner_sid: u64,
    /// The slot holds an OPEN window (minimized still counts as open).
    pub open: bool,
    pub minimized: bool,
    pub focused: bool,
}

/// Derives the feed entry set from the compositor's app slots. Returns the
/// entry count written into `out` (bounded by [`WINDOWS_MAX`]).
#[must_use]
pub fn build_window_set(slots: &[SlotInfo], out: &mut [WindowEntry; WINDOWS_MAX]) -> usize {
    let mut n = 0;
    for s in slots {
        if n == WINDOWS_MAX {
            break;
        }
        if !s.open || s.owner_sid == 0 {
            continue;
        }
        let mut flags = 0u8;
        if s.minimized {
            flags |= WINDOW_FLAG_MINIMIZED;
        }
        // A minimized window is never the focused one, whatever the stack
        // says — the shell renders "focused" as the active-app marker.
        if s.focused && !s.minimized {
            flags |= WINDOW_FLAG_FOCUSED;
        }
        out[n] = WindowEntry { owner_sid: s.owner_sid, flags };
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(owner_sid: u64, open: bool, minimized: bool, focused: bool) -> SlotInfo {
        SlotInfo { owner_sid, open, minimized, focused }
    }

    #[test]
    fn publishes_open_windows_with_flags() {
        let slots = [slot(11, true, false, true), slot(22, true, true, false)];
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        assert_eq!(build_window_set(&slots, &mut out), 2);
        assert_eq!(out[0], WindowEntry { owner_sid: 11, flags: WINDOW_FLAG_FOCUSED });
        assert_eq!(out[1], WindowEntry { owner_sid: 22, flags: WINDOW_FLAG_MINIMIZED });
    }

    #[test]
    fn closed_slots_are_omitted() {
        let slots = [slot(11, false, false, false), slot(22, true, false, false)];
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        assert_eq!(build_window_set(&slots, &mut out), 1);
        assert_eq!(out[0].owner_sid, 22);
    }

    #[test]
    fn unidentified_slots_are_omitted() {
        // sid 0 = no identity: the shell could not activate it, so a tile
        // for it would be a dead control.
        let slots = [slot(0, true, false, true)];
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        assert_eq!(build_window_set(&slots, &mut out), 0);
    }

    #[test]
    fn minimized_never_reports_focused() {
        let slots = [slot(11, true, true, true)];
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        assert_eq!(build_window_set(&slots, &mut out), 1);
        assert_eq!(out[0].flags, WINDOW_FLAG_MINIMIZED);
    }

    #[test]
    fn bounded_by_windows_max() {
        let slots = [slot(9, true, false, false); WINDOWS_MAX + 3];
        let mut out = [WindowEntry::default(); WINDOWS_MAX];
        assert_eq!(build_window_set(&slots, &mut out), WINDOWS_MAX);
    }
}
