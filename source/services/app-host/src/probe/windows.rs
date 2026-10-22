// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's pushes to the shell — RFC-0086 window-feed intake (decode
//! `OP_SURFACE_WINDOWS` into the effect host's cache; retained latest-wins: every frame
//! REPLACES the set, so a dropped push heals at the next one and a duplicate is free) and the
//! RFC-0095 capture keys (`OP_SURFACE_CAPTURE_KEY`: Print, Shift+Print, Alt+Print fire the
//! shell's `CaptureOpen` / `CaptureScreen` / `CaptureWindow`) — plus the ONE by-name trigger
//! dispatch every container-scoped event uses.
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

    /// A push from windowd to the shell: the window feed, or a capture key. `true` when the
    /// page re-emitted (the caller full-repaints).
    pub(super) fn absorb_shell_push(&mut self, frame: &[u8]) -> bool {
        use nexus_display_proto::surface_capture as cap;
        if let Some((kind, _seq)) = cap::decode_capture_key(frame) {
            return self.fire_named(match kind {
                cap::CAPTURE_KEY_SCREEN => "CaptureScreen",
                cap::CAPTURE_KEY_WINDOW => "CaptureWindow",
                _ => "CaptureOpen",
            });
        }
        self.absorb_window_feed(frame)
    }

    /// Dispatches the page's declarative `on <name>` handler — container-scoped, BY NAME: the
    /// event has no pixel to hit-test. `true` when the model changed (the caller repaints).
    pub(super) fn fire_named(&mut self, name: &str) -> bool {
        self.fire(None, name)
    }

    /// Dispatches `on <name>` of the box `box_id` — the container the event belongs to (a scroll
    /// layer's `EndReached`, a pager's `PageNext`: RFC-0109, every layer its own). `true` when the
    /// model changed.
    pub(super) fn fire_at(&mut self, box_id: usize, name: &str) -> bool {
        self.fire(Some(box_id), name)
    }

    fn fire(&mut self, box_id: Option<usize>, name: &str) -> bool {
        use nexus_dsl_runtime::Damage;
        let tokens = super::tokens_for(self.theme_mode);
        let device = super::device_for(
            self.shell_profile,
            self.w,
            &self.locale_tag,
            &self.keymap,
            self.theme_mode,
        );
        let locale = super::app_locale!(self);
        let damage = match box_id {
            Some(b) => self.view.fire_on_box(tokens, &device, &locale, &mut self.host, b, name),
            None => self.view.fire_trigger(tokens, &device, &locale, &mut self.host, name),
        }
        .ok()
        .flatten();
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            return false;
        }
        // Bounded like a tap's (TASK-0337, cycle 52): the window feed changes the taskbar's
        // rows, not the screen — the full repaint this was cost the desktop a full-screen CPU
        // raster and the board a full-screen frame on every window open, close and focus
        // (25 of them in one cycle). The diff of the layout before and after names the rows.
        self.tap_render_span = None;
        if matches!(damage, Some(Damage::Layout)) {
            let old_boxes = core::mem::take(&mut self.layout.boxes);
            let old_texts = core::mem::take(&mut self.texts);
            self.relayout_retained();
            self.tap_render_span = crate::layout_diff::changed_row_span(
                &old_boxes,
                &self.layout.boxes,
                &old_texts,
                &self.texts,
                self.layout_bottom(),
            );
            self.tree_dirty = true;
        }
        true
    }
}
