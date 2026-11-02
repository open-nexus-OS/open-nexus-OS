// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the app-window SLOTS (RFC-0114 §7): they grow with the
//! windows (a free one is reused, a new one registered in the z-stack), and every window a slot
//! holds gets a fresh KEY (`window_keys`) — its layers' id and owner, its damage's origin and its
//! transform on every wire — never two live windows alike, never the key of one whose frames may
//! still be in flight. A window new to its slot takes the slot's place in the cascade. Plus the
//! id→slot lookups and the windows' marker names.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host-tested parts: the stack's growth (`window_scene`), the keys
//! (`window_keys`), the cascade (`surface_presentation`); the many-windows lane.

use super::*;

pub(crate) use crate::window_keys::{WindowKeys, DESKTOP_KEY};

/// A window's short name for markers and diagnostics (`app3`, `desktop`).
pub(crate) struct WindowName(crate::window_scene::WindowId);

impl core::fmt::Display for WindowName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            crate::window_scene::WindowId::App(i) => write!(f, "app{i}"),
            crate::window_scene::WindowId::Desktop => f.write_str("desktop"),
        }
    }
}

impl DisplayServerRuntime {
    /// Stable short name for markers/diagnostics.
    pub(super) fn window_name(id: crate::window_scene::WindowId) -> WindowName {
        WindowName(id)
    }

    pub(crate) fn app_slot_mut(
        &mut self,
        id: crate::window_scene::WindowId,
    ) -> Option<&mut AppWindowSlot> {
        match id {
            crate::window_scene::WindowId::App(i) => self.apps.get_mut(usize::from(i)),
            crate::window_scene::WindowId::Desktop => None,
        }
    }

    /// Slot index currently bound to `surface_id` (present/input routing).
    pub(crate) fn app_index_by_surface(&self, surface_id: u32) -> Option<usize> {
        self.apps.iter().position(|a| a.surface_id == Some(surface_id))
    }

    /// The wire key of slot `idx`'s window (`0`: no such slot).
    pub(crate) fn window_key(&self, idx: usize) -> u32 {
        self.apps.get(idx).map_or(0, |a| u32::from(a.key))
    }

    /// A slot for a NEW app window, with a fresh key: a free one (no surface, none being
    /// replaced, no launch waiting in it), or a new one — the slots grow with the windows.
    /// `None` only when memory refuses one more.
    pub(crate) fn take_app_slot(&mut self) -> Option<usize> {
        let apps = &self.apps;
        let key = self.window_keys.fresh(|key| {
            apps.iter().any(|a| {
                a.key == key && (a.surface_id.is_some() || a.retiring.is_some() || a.starting)
            })
        })?;
        let free = self
            .apps
            .iter()
            .position(|a| a.surface_id.is_none() && a.retiring.is_none() && !a.starting);
        let idx = match free {
            Some(idx) => idx,
            None => {
                let idx = self.apps.len();
                let id = u16::try_from(idx).ok()?;
                if self.apps.try_reserve(1).is_err()
                    || !self.windows.register(crate::window_scene::WindowId::App(id))
                {
                    return None;
                }
                self.apps.push(AppWindowSlot::new());
                idx
            }
        };
        self.apps[idx].key = key;
        self.apps[idx].placed = false;
        Some(idx)
    }

    /// Frames slot `idx`'s floating window at `(w, h)` on the work area: a window new to its
    /// slot takes the slot's place in the cascade (`cascade_origin`), a placed one keeps its
    /// origin, slid back onto the area.
    pub(crate) fn frame_floating(&mut self, idx: usize, (w, h): (u32, u32)) {
        let area = (self.mode.width, self.work_area_h());
        let min_y = self.full_surface_frame(idx).0;
        let slot = &mut self.apps[idx];
        if slot.placed {
            slot.win.set_frame_clamped(w, h, area, min_y);
        } else {
            let (x, y) = crate::surface_presentation::cascade_origin(idx, (w, h), area, min_y);
            slot.win.set_frame(x, y, w, h);
            slot.placed = true;
        }
    }
}
