// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0086 window-feed intake — decode `OP_SURFACE_WINDOWS` from
//! windowd's push channel into the effect host's cache. Retained
//! latest-wins: every frame REPLACES the set, so a dropped push heals at the
//! next one and a duplicate is free.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: wire path via the QEMU marker chain (`windowd: windows
//! push` → the shell's re-enumerate).
//! RFC: docs/rfcs/RFC-0086-shell-taskbar-window-feed.md

impl super::DslApp {
    /// Applies one `OP_SURFACE_WINDOWS` frame; `false` = not this op (the
    /// caller keeps matching) or malformed (dropped, fail-closed).
    pub(super) fn apply_window_feed(&mut self, frame: &[u8]) -> bool {
        use nexus_display_proto::surface_windows as feed;
        let mut set = [feed::WindowEntry::default(); feed::WINDOWS_MAX];
        let Some(n) = feed::decode_surface_windows(frame, &mut set) else {
            return false;
        };
        self.host.windows[..n].copy_from_slice(&set[..n]);
        self.host.windows_len = n;
        true
    }

    /// [`Self::apply_window_feed`] + the `WindowsChanged` dispatch: `true`
    /// when the frame WAS a window feed AND the shell re-emitted (the caller
    /// full-repaints). A feed that changes nothing visible still returns
    /// false — no repaint is owed for it.
    pub(super) fn absorb_window_feed(&mut self, frame: &[u8]) -> bool {
        self.apply_window_feed(frame) && self.fire_windows_changed()
    }
}
