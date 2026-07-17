// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-app surface lifecycle — the lazy "own VMO per app" model; each
//! app owns its surface, windowd composites it as a layer (RFC-0065 / ADR-0037).
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 tests
//!
//! Per-app surface lifecycle — the lazy "own VMO per app" model (RFC-0065).
//!
//! Each app owns its **own** surface (its own VMO/buffer via `create_surface`),
//! composited as its **own layer** with z-order — never baked into a shared plane
//! with everything else. A surface is **lazily allocated when the app becomes
//! active** (launched/foregrounded) and **freed when it closes/stops** — a
//! window-server model where an inactive app holds no surface VMO.
//!
//! This module is the pure, host-tested bookkeeping: it maps app **instance ids**
//! (from `abilitymgr`) to their surface id + z-order and the residency state. The
//! windowd server owns the actual VMO (`create_surface`/`destroy_surface`); this
//! tracks *which* instance owns *which* surface and *whether it is resident*, so
//! the compositor knows what to draw and what to free.

use alloc::string::String;
use alloc::vec::Vec;

/// Whether an app's surface VMO is currently allocated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Residency {
    /// No surface allocated — app not running / closed. Holds no VMO.
    Unloaded,
    /// Surface allocated + composited as its own layer (app open/active).
    Loaded {
        /// windowd surface id (its own VMO/buffer).
        surface_id: u64,
        /// Composite z-order (higher = nearer the viewer).
        z: i16,
    },
}

/// A per-app surface slot: one app instance ↔ its own surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSurfaceSlot {
    /// abilitymgr instance id.
    pub instance_id: u32,
    /// App/bundle id (e.g. `"search"`).
    pub app_id: String,
    /// Current residency.
    pub residency: Residency,
}

/// Errors from the per-app surface registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppSurfaceError {
    /// The registry is at capacity.
    TooManyApps,
    /// The instance already has a resident surface.
    AlreadyMounted,
    /// No slot for the given instance.
    #[allow(dead_code)] // declared registry error vocabulary (widget-promotion seam)
    UnknownInstance,
}

/// Maximum concurrently-resident app surfaces (bounded; ≤ windowd `MAX_SURFACES`).
pub const MAX_APP_SURFACES: usize = 16;

/// Registry of per-app surfaces with lazy residency + z-ordering.
#[derive(Debug, Default)]
pub struct AppSurfaces {
    slots: Vec<AppSurfaceSlot>,
    next_z: i16,
}

impl AppSurfaces {
    /// Creates an empty registry. App layers start above the shell chrome (z≥100).
    pub fn new() -> Self {
        Self { slots: Vec::new(), next_z: 100 }
    }

    /// Mounts an app's surface as resident (called when the app becomes active and
    /// the windowd server has allocated its surface VMO). Assigns the next z-order.
    ///
    /// Lazy-load: this is the *only* point a surface becomes resident; before this
    /// the app holds no VMO.
    pub fn mount(
        &mut self,
        instance_id: u32,
        app_id: &str,
        surface_id: u64,
    ) -> Result<i16, AppSurfaceError> {
        if let Some(slot) = self.slots.iter().find(|s| s.instance_id == instance_id) {
            if matches!(slot.residency, Residency::Loaded { .. }) {
                return Err(AppSurfaceError::AlreadyMounted);
            }
        }
        if self.resident_count() >= MAX_APP_SURFACES {
            return Err(AppSurfaceError::TooManyApps);
        }
        let z = self.next_z;
        self.next_z = self.next_z.saturating_add(1);
        match self.slots.iter_mut().find(|s| s.instance_id == instance_id) {
            Some(slot) => {
                slot.app_id = String::from(app_id);
                slot.residency = Residency::Loaded { surface_id, z };
            }
            None => self.slots.push(AppSurfaceSlot {
                instance_id,
                app_id: String::from(app_id),
                residency: Residency::Loaded { surface_id, z },
            }),
        }
        Ok(z)
    }

    /// Unmounts (frees) an app's surface — called on close/stop. Returns the
    /// `surface_id` the caller must `destroy_surface` to reclaim the VMO, or `None`
    /// if the instance held no resident surface.
    pub fn unmount(&mut self, instance_id: u32) -> Option<u64> {
        let slot = self.slots.iter_mut().find(|s| s.instance_id == instance_id)?;
        let freed = match slot.residency {
            Residency::Loaded { surface_id, .. } => Some(surface_id),
            Residency::Unloaded => None,
        };
        slot.residency = Residency::Unloaded;
        // Drop the slot entirely once unloaded (the app is gone).
        self.slots.retain(|s| s.instance_id != instance_id);
        freed
    }

    /// The surface id resident for an instance, if any.
    pub fn surface_of(&self, instance_id: u32) -> Option<u64> {
        self.slots.iter().find_map(|s| match s.residency {
            Residency::Loaded { surface_id, .. } if s.instance_id == instance_id => {
                Some(surface_id)
            }
            _ => None,
        })
    }

    /// `true` if the instance currently holds a resident surface VMO.
    pub fn is_loaded(&self, instance_id: u32) -> bool {
        self.surface_of(instance_id).is_some()
    }

    /// Number of resident (loaded) app surfaces.
    pub fn resident_count(&self) -> usize {
        self.slots.iter().filter(|s| matches!(s.residency, Residency::Loaded { .. })).count()
    }

    /// The resident app layers as `(surface_id, z)`, ascending by z (back to front).
    /// This is what the compositor draws — one layer per app, not a shared plane.
    pub fn layers(&self) -> Vec<(u64, i16)> {
        let mut out: Vec<(u64, i16)> = self
            .slots
            .iter()
            .filter_map(|s| match s.residency {
                Residency::Loaded { surface_id, z } => Some((surface_id, z)),
                Residency::Unloaded => None,
            })
            .collect();
        out.sort_by_key(|&(_, z)| z);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_allocates_own_surface_and_zorder() {
        let mut apps = AppSurfaces::new();
        let z = apps.mount(1, "search", 5000).expect("mount");
        assert_eq!(z, 100);
        assert!(apps.is_loaded(1));
        assert_eq!(apps.surface_of(1), Some(5000));
        assert_eq!(apps.resident_count(), 1);
    }

    #[test]
    fn each_app_gets_its_own_layer_and_increasing_z() {
        let mut apps = AppSurfaces::new();
        apps.mount(1, "search", 5000).unwrap();
        apps.mount(2, "chat", 5001).unwrap();
        // Two independent surfaces, each its own layer, ascending z.
        assert_eq!(apps.layers(), vec![(5000, 100), (5001, 101)]);
    }

    #[test]
    fn unmount_frees_the_surface_for_destroy() {
        let mut apps = AppSurfaces::new();
        apps.mount(1, "search", 5000).unwrap();
        // Close → returns the surface id to destroy (reclaim the VMO).
        assert_eq!(apps.unmount(1), Some(5000));
        assert!(!apps.is_loaded(1));
        assert_eq!(apps.resident_count(), 0);
        assert!(apps.layers().is_empty());
    }

    #[test]
    fn unmount_unknown_instance_is_none() {
        let mut apps = AppSurfaces::new();
        assert_eq!(apps.unmount(99), None);
    }

    #[test]
    fn double_mount_rejected() {
        let mut apps = AppSurfaces::new();
        apps.mount(1, "search", 5000).unwrap();
        assert_eq!(apps.mount(1, "search", 5001).unwrap_err(), AppSurfaceError::AlreadyMounted);
    }

    #[test]
    fn remount_after_close_is_allowed() {
        let mut apps = AppSurfaces::new();
        apps.mount(1, "search", 5000).unwrap();
        apps.unmount(1);
        // Re-opening the app allocates a fresh surface (lazy load again).
        let z = apps.mount(1, "search", 6000).expect("remount");
        assert_eq!(apps.surface_of(1), Some(6000));
        assert!(z >= 100);
    }

    #[test]
    fn resident_count_is_bounded() {
        let mut apps = AppSurfaces::new();
        for i in 0..MAX_APP_SURFACES as u32 {
            apps.mount(i + 1, "app", 5000 + i as u64).unwrap();
        }
        let err = apps.mount(999, "overflow", 9999).unwrap_err();
        assert_eq!(err, AppSurfaceError::TooManyApps);
    }

    #[test]
    fn inactive_app_holds_no_surface() {
        // The model's core invariant: an app that is not mounted holds no VMO.
        let apps = AppSurfaces::new();
        assert!(!apps.is_loaded(1));
        assert_eq!(apps.surface_of(1), None);
        assert!(apps.layers().is_empty());
    }
}
