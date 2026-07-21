// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — app-window SLOT lookups (moved out
//! of `runtime/mod.rs`, structure-gate). Pure accessors; no behavior change.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: covered via windowd host integration + QEMU smoke.

use super::*;

impl DisplayServerRuntime {
    pub(crate) fn app_slot_mut(
        &mut self,
        id: crate::window_scene::WindowId,
    ) -> Option<&mut AppWindowSlot> {
        match id {
            crate::window_scene::WindowId::App(i) => self.apps.get_mut(i as usize),
            crate::window_scene::WindowId::Desktop => None,
        }
    }

    /// Slot index currently bound to `surface_id` (present/input routing).
    pub(crate) fn app_index_by_surface(&self, surface_id: u32) -> Option<usize> {
        self.apps.iter().position(|a| a.surface_id == Some(surface_id))
    }

    /// A free slot for a NEW app window (no bound surface).
    pub(crate) fn free_app_index(&self) -> Option<usize> {
        self.apps.iter().position(|a| a.surface_id.is_none())
    }
}
