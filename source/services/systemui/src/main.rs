// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SystemUI service entrypoint for the modular first-frame seed.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p systemui -- --nocapture`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#![forbid(unsafe_code)]

fn main() {
    systemui::run();
}
