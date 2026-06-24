// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Search app (RFC-0065) — search as a real app that owns its data,
//! filter, and surface content. windowd hosts the surface as a per-app layer
//! (ADR-0037); the app does not render into the shared atlas.
//! OWNERS: @ui
//! STATUS: Functional (model + owned-surface render; windowd present wired in P4b swap)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: model (filter/geometry) + render (owned surface) host tests
//! ADR: docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md

// `no_std` for the non-test build so the OS compositor (windowd, no_std) can link
// the app's data/logic directly as a per-app surface source (ADR-0037 step 1);
// host tests keep `std`. `alloc` provides `Vec`/`String` in both.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

#[macro_use]
extern crate alloc;

pub mod model;
pub mod render;

pub use render::OwnedSurface;

/// Marker emitted once the app has composed its first surface frame.
pub const SEARCH_APP_READY_MARKER: &str = "search-app: surface ready";

/// Composes the app's first surface frame and returns it (the app owns this VMO
/// content). The OS path hands this to windowd via `create_surface`/present once
/// the compositor consumes per-app client surfaces (P4b swap).
pub fn first_surface() -> OwnedSurface {
    render::render("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_surface_is_nonempty_and_sized() {
        let s = first_surface();
        assert_eq!(s.width, model::SEARCH_W);
        assert!(!s.pixels.is_empty());
    }
}
