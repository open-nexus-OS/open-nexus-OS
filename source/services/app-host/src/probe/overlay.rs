// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the app-host side of modal semantics (TASK-0074, ADR-0068): after every dispatch
//! the host mirrors the runtime's modal depth to windowd through the ONE verb
//! (`CONTROL_WIN_MODAL`, app-modal across the owner's windows), arms its one-shot timer for the
//! transient layer's declared `.dismissAfter(ms)` (the DSL holds no clock), and names every
//! runtime-fired `on Dismiss` with its reason — the markers the ladders read. ESC reaches here
//! as imed's `ACTION_ESCAPE` on the focused surface and fires the topmost modal's handler; with
//! no modal open it changes nothing (the retired "Escape drops widget focus" path is gone).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: runtime conformance (`tests/dsl_conformance/tests/overlays.rs`) + the
//! `windowd: win modal on` / `apphost: modal dismiss` ladder rungs.

use super::marker::raw_marker;
use super::{device_for, tokens_for};
use nexus_dsl_runtime::{Damage, DismissReason};

impl super::DslApp {
    /// Reconciles the scene's overlay stack with the outside world: the windowd verb on a
    /// depth edge (0↔n), the depth marker on every rise, the transient timer when the
    /// transient layer appears or leaves. Called before each paint (every dispatch path
    /// marks the surface dirty) — a compare, no allocation on the steady state. A modal that
    /// closed and ANOTHER that opened before one paint (ESC on the search, then Print) keep the
    /// depth: the top modal's identity names the new one (the marker), while windowd — modal
    /// throughout — hears no verb.
    pub(super) fn overlay_sync(&mut self) {
        let depth = self.view.overlays().modal_depth();
        let top = self.view.overlays().top_modal().map(|e| (e.path.as_slice(), e.instance));
        let sent_top = self.modal_top_sent.as_ref().map(|(path, inst)| (path.as_slice(), *inst));
        let edge = crate::modal_edge::modal_edge((self.modal_depth_sent, sent_top), (depth, top));
        if edge.opened {
            raw_marker(&alloc::format!("apphost: modal open (depth={depth})"));
        }
        if let Some(on) = edge.verb {
            let verb = if on { "modal.on" } else { "modal.off" };
            if self.host.presentation_control("window.control", verb).is_err() {
                raw_marker("apphost: modal control FAIL (send)");
            }
        }
        self.modal_depth_sent = depth;
        if top != sent_top {
            self.modal_top_sent = top.map(|(path, inst)| (path.to_vec(), inst));
        }
        let next = self.view.overlays().transients().find(|e| e.dismiss_after_ms.is_some());
        match (next, self.transient_armed.as_ref()) {
            (Some(entry), Some((path, _))) if *path == entry.path => {}
            (Some(entry), _) => {
                let ms = u64::from(entry.dismiss_after_ms.unwrap_or(0));
                let deadline = super::clock::mono_now_ns().saturating_add(ms * 1_000_000).max(1);
                self.transient_armed = Some((entry.path.clone(), deadline));
            }
            (None, Some(_)) => self.transient_armed = None,
            (None, None) => {}
        }
    }

    /// The host's ONE one-shot timer deadline (absolute monotonic ns, 0 = nothing to arm):
    /// the earlier of the clock's minute boundary and the transient layer's timeout.
    pub(super) fn timer_deadline_ns(&self) -> u64 {
        let clock = self.clock_deadline_ns();
        let transient = self.transient_armed.as_ref().map(|(_, deadline)| *deadline);
        match (clock, transient) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => 0,
        }
    }

    /// The one-shot fired: run whichever deadline is due (both, when they coincide). Returns
    /// whether the app changed (re-render).
    pub(super) fn timer_fired(&mut self) -> bool {
        // A one-shot fires AT its deadline; the 1 ms slack covers the clock read after it.
        let now = super::clock::mono_now_ns().saturating_add(1_000_000);
        let mut changed = false;
        if self.clock_supported() && now >= self.clock_deadline_ns {
            changed |= self.clock_tick();
        }
        if self.transient_armed.as_ref().is_some_and(|(_, deadline)| now >= *deadline) {
            changed |= self.transient_timeout();
        }
        changed
    }

    /// The transient layer's declared lifetime ran out: fire its `on Dismiss` (reason
    /// Timeout). The layer leaves when the reducer changes the state that emits it — never
    /// by the host hiding it.
    fn transient_timeout(&mut self) -> bool {
        let Some((path, _)) = self.transient_armed.take() else { return false };
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        let damage = match self.view.dismiss_at(
            tokens,
            &device,
            &locale,
            &mut self.host,
            &path,
            DismissReason::Timeout,
        ) {
            Ok(d) => d,
            Err(e) => {
                raw_marker(&alloc::format!("apphost: modal dismiss ERR {e:?}"));
                None
            }
        };
        self.note_dismiss();
        self.apply_dismiss_damage(damage)
    }

    /// ESC on the focused surface (imed `ACTION_ESCAPE`): the topmost modal's `on Dismiss`
    /// (reason Escape). No modal → nothing, by contract (TASK-0074 D3).
    pub(super) fn dismiss_escape(&mut self) -> bool {
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        let damage = match self.view.dismiss_top(
            tokens,
            &device,
            &locale,
            &mut self.host,
            DismissReason::Escape,
        ) {
            Ok(d) => d,
            Err(e) => {
                raw_marker(&alloc::format!("apphost: modal dismiss ERR {e:?}"));
                None
            }
        };
        self.note_dismiss();
        self.apply_dismiss_damage(damage)
    }

    /// Names the runtime-fired dismissal, once per firing (the ladder's rung).
    pub(super) fn note_dismiss(&mut self) {
        if let Some(reason) = self.view.take_dismissed() {
            raw_marker(&alloc::format!("apphost: modal dismiss (reason={})", reason.as_str()));
        }
    }

    fn apply_dismiss_damage(&mut self, damage: Option<Damage>) -> bool {
        if matches!(damage, Some(Damage::Layout)) {
            self.relayout_retained();
            self.layers_dirty = true;
        }
        self.anim_sync();
        matches!(damage, Some(Damage::Paint) | Some(Damage::Layout))
    }
}
