// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the memory level at the app (TASK-0333 6e-3, RFC-0100 §3, ADR-0086). The compositor
//! tells the window the level the one memory authority told it (`OP_SURFACE_PRESSURE`); app-host
//! raises it into the mounted program as the HOST EVENT `MemoryPressure` — one case per level
//! (`Normal`, `Warn`, `Critical`, `Emergency`), no payload — so a program that declares it drops
//! what it can rebuild (a decoded image, a long list's off-screen rows), and a program without it
//! does no work: no event case, no dispatch — exactly `ImeStripEvent`'s posture. The host itself
//! holds nothing it could give back to the pool here (its caches live in its heap, which does not
//! decommit; the frame arena is the window's) — it names the level: `apphost: mem pressure
//! (level= handled=)`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (the event's name and cases are the DSL's host-event contract,
//!   `docs/dev/dsl/runtime.md`)
//! TEST_COVERAGE: `case_name` below (host); the visible lane's `apphost: mem pressure (level=warn`

use nexus_abi::syscall::memory::{
    mem_pressure_name, MEM_PRESSURE_CRITICAL, MEM_PRESSURE_EMERGENCY, MEM_PRESSURE_NORMAL,
    MEM_PRESSURE_WARN,
};

use super::*;

/// The host event a program declares to hear the level.
pub(super) const EVENT: &str = "MemoryPressure";

/// An `OP_SURFACE_PRESSURE` frame from the compositor: the level raised into the program and
/// named on the console. `Some(handled)` when `frame` was one — `handled` says the program
/// declared the case and its dispatch changed the view; `None` for any other frame.
pub(super) fn absorb(frame: &[u8], app: Option<&mut DslApp>) -> Option<bool> {
    let level = nexus_display_proto::surface_pressure::decode_surface_pressure(frame)?;
    let handled = app.is_some_and(|d| d.apply_mem_pressure(level));
    raw_marker(&alloc::format!(
        "apphost: mem pressure (level={} handled={})",
        mem_pressure_name(level),
        u8::from(handled)
    ));
    Some(handled)
}

/// The event case a level is raised as; `None` for a level this build does not know.
#[must_use]
pub(super) fn case_name(level: u8) -> Option<&'static str> {
    match level {
        MEM_PRESSURE_NORMAL => Some("Normal"),
        MEM_PRESSURE_WARN => Some("Warn"),
        MEM_PRESSURE_CRITICAL => Some("Critical"),
        MEM_PRESSURE_EMERGENCY => Some("Emergency"),
        _ => None,
    }
}

impl DslApp {
    /// Raises the level into the program as `MemoryPressure::<Case>`; `true` when the program
    /// declared the case and its dispatch changed the view.
    pub(super) fn apply_mem_pressure(&mut self, level: u8) -> bool {
        let Some(case) = case_name(level) else { return false };
        let Some((event, case)) = self.view.runtime.event_case(EVENT, case) else {
            return false;
        };
        let tokens = tokens_for(self.theme_mode);
        let device =
            device_for(self.shell_profile, self.w, &self.locale_tag, &self.keymap, self.theme_mode);
        let locale = super::app_locale!(self);
        let payload = alloc::vec::Vec::new();
        match self.view.dispatch(tokens, &device, &locale, &mut self.host, event, case, payload) {
            Ok(nexus_dsl_runtime::Damage::Layout) => {
                self.relayout_retained();
                true
            }
            Ok(nexus_dsl_runtime::Damage::Paint) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::case_name;

    #[test]
    fn every_level_has_its_case_and_an_unknown_one_none() {
        assert_eq!(case_name(0), Some("Normal"));
        assert_eq!(case_name(1), Some("Warn"));
        assert_eq!(case_name(2), Some("Critical"));
        assert_eq!(case_name(3), Some("Emergency"));
        assert_eq!(case_name(4), None);
    }
}
