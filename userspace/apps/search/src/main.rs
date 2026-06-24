// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: search app entry — composes its own surface and reports readiness.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (thin entry; logic host-tested in `search_app` lib)
//! ADR: docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md

#![forbid(unsafe_code)]

fn main() {
    // The app owns + composes its surface content; windowd hosts it as a per-app
    // layer once the compositor consumes client surfaces (P4b swap).
    let surface = search_app::first_surface();
    debug_assert_eq!(surface.pixels.len(), (surface.width * surface.height * 4) as usize);
    println!("{}", search_app::SEARCH_APP_READY_MARKER);
}
