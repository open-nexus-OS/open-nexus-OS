// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! app-host `DslApp` text I/O (RFC-0075): tap-to-focus resolution and its
//! announcement upstream, committed-text and editing-action delivery (ESC =
//! the modal's dismissal, TASK-0074), the IME strip state. A pure move out of
//! `interaction.rs` (structure ratchet), no behavior change.

use super::super::*;

impl DslApp {
    /// RFC-0075 tap-to-focus: resolve widget text focus at the tap point
    /// (AFTER `tap` — its dispatch may have re-laid-out). Returns the
    /// announcement for windowd when the focus state CHANGED:
    /// `(focused, field_kind, caret rect)`; `None` = unchanged.
    pub(crate) fn text_focus_update(
        &mut self,
        x: i32,
        y: i32,
    ) -> Option<(bool, u8, (u16, u16, u16, u16))> {
        use nexus_display_proto::surface_text;
        let before = self.view.text_focus();
        let scroll = self.scroll_param();
        let snap = self.view.focus_text_at(
            &self.layout.boxes,
            nexus_layout_types::FxPx::new(x),
            nexus_layout_types::FxPx::new(y),
            scroll,
        );
        // Same-field taps STILL announce when focused (RFC-0075 Phase 8c):
        // the tap is the user's explicit "open the keyboard" gesture — it
        // must re-open a dismissed OSK. Only a no-op UNFOCUSED state stays
        // silent (pointer taps outside any field).
        if snap == before && snap.is_none() {
            return None;
        }
        match snap {
            Some(s) => {
                let clamp = |v: i32| v.max(0).min(i32::from(u16::MAX)) as u16;
                let rect = self
                    .layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == s.box_id)
                    .map(|b| {
                        (
                            clamp(b.rect.x.0),
                            clamp(b.rect.y.0),
                            clamp(b.rect.width.0),
                            clamp(b.rect.height.0),
                        )
                    })
                    .unwrap_or((0, 0, 0, 0));
                let kind = if s.secure {
                    surface_text::SURFACE_FIELD_PASSWORD
                } else {
                    surface_text::SURFACE_FIELD_TEXT
                };
                Some((true, kind, rect))
            }
            None => Some((false, surface_text::SURFACE_FIELD_TEXT, (0, 0, 0, 0))),
        }
    }

    /// RFC-0075 committed-text delivery: insert into the FOCUSED field.
    /// Returns whether a re-render is needed.
    pub(crate) fn text_commit(&mut self, text: &str) -> bool {
        use nexus_dsl_runtime::Damage;
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        let damage = match self.view.insert_text(tokens, &device, &locale, &mut self.host, text) {
            Ok(d) => d,
            Err(e) => {
                raw_marker(&alloc::format!("apphost: text commit ERR {e:?}"));
                None
            }
        };
        if !matches!(damage, Some(Damage::Paint) | Some(Damage::Layout)) {
            return false;
        }
        // One-shot end-to-end proof (RFC-0075): the first commit that changed
        // the focused field. Typed text and its rhythm NEVER hit markers.
        if crate::proof_line::TEXT_COMMIT.claim() {
            raw_marker("apphost: text commit applied");
        }
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_retained();
        }
        true
    }

    /// RFC-0075 editing-action delivery (imed wire `ACTION_*` in `aux`).
    /// Backspace edits the focused field; Escape fires the topmost modal's
    /// `on Dismiss` (TASK-0074 D3 — the ONE escape path; with no modal open
    /// it changes nothing). Enter/Tab are page-level concerns (deferred — no
    /// focus traversal yet). Returns re-render need.
    pub(crate) fn text_action(&mut self, action: u8) -> bool {
        match action {
            nexus_wire::imed::ACTION_ESCAPE => self.dismiss_escape(),
            // Backspace and the TASK-0067B editing commands: `text_edit.rs`.
            nexus_wire::imed::ACTION_BACKSPACE => self.edit_action(action),
            a if nexus_wire::imed::is_edit_action(a) => self.edit_action(a),
            _ => false,
        }
    }

    /// RFC-0075 tap-to-focus announcement: resolve widget focus at the tap
    /// point and, on a TRANSITION, send `OP_SURFACE_TEXT_FOCUS` to windowd
    /// (which relays to imed). Marker carries no text content.
    pub(crate) fn announce_text_focus(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        x: i32,
        y: i32,
    ) {
        use nexus_display_proto::surface_text;
        if surface_id == 0 {
            return; // no surface yet — nothing to claim
        }
        let Some((focused, field_kind, caret)) = self.text_focus_update(x, y) else {
            return;
        };
        let f = surface_text::encode_surface_text_focus(surface_id, focused, field_kind, caret);
        let _ = client.send(&f, Wait::NonBlocking);
        raw_marker(if focused { "apphost: text focus set" } else { "apphost: text focus cleared" });
    }

    /// `.autofocus(true)` (TASK-0067B): while no field holds focus and an autofocus
    /// field is on screen, focus it through the tap path and announce it. Called after a
    /// present — the layout its box comes from is the one on screen.
    pub(crate) fn apply_autofocus(&mut self, client: &KernelClient, surface_id: u32) {
        let Some(box_id) = self.view.autofocus_box() else { return };
        let Some(b) = self.layout.boxes.iter().find(|b| b.node_id == box_id) else { return };
        let (cx, cy) = (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2);
        self.announce_text_focus(client, surface_id, cx, cy);
    }

    /// RFC-0075 composed-text delivery: decode an `OP_SURFACE_TEXT` frame and
    /// apply it to the focused field. Returns re-render need (`false` also
    /// for non-text frames). Text content NEVER hits markers/logs.
    pub(crate) fn apply_surface_text(&mut self, frame: &[u8]) -> bool {
        use nexus_display_proto::surface_text as st;
        let Some((tkind, aux, text)) = st::decode_surface_text(frame) else {
            return false;
        };
        match tkind {
            st::SURFACE_TEXT_COMMIT => self.text_commit(text),
            st::SURFACE_TEXT_ACTION => self.text_action(aux),
            // Preedit display lands with the candidate UI (RFC-0075 Phase 3).
            _ => false,
        }
    }

    /// Composition-strip push (`OP_SURFACE_IME_STATE`, RFC-0075 Phase 3):
    /// dispatched into the mounted program as `ImeStripEvent::Preedit(text)`
    /// / `ImeStripEvent::Cands(8 × text)` — only the ime-ui app declares
    /// them; every other app ignores the frame (no event case = no work).
    pub(crate) fn apply_ime_state(&mut self, frame: &[u8]) -> bool {
        use nexus_display_proto::surface_text as st;
        use nexus_dsl_runtime::Value;
        let Some(push) = st::decode_ime_state(frame) else {
            return false;
        };
        let (case_name, args) = match push {
            st::ImeStatePush::Preedit(text) => {
                ("Preedit", alloc::vec![Value::Str(alloc::string::String::from(text))])
            }
            st::ImeStatePush::Candidates(_page, items, _count) => (
                "Cands",
                items
                    .iter()
                    .map(|t| Value::Str(alloc::string::String::from(*t)))
                    .collect::<alloc::vec::Vec<_>>(),
            ),
        };
        let Some((event, case)) = self.view.runtime.event_case("ImeStripEvent", case_name) else {
            return false;
        };
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        let damage =
            self.view.dispatch(tokens, &device, &locale, &mut self.host, event, case, args);
        match damage {
            Ok(nexus_dsl_runtime::Damage::Layout) => {
                self.relayout_retained();
                true
            }
            Ok(nexus_dsl_runtime::Damage::Paint) => true,
            _ => false,
        }
    }
}
