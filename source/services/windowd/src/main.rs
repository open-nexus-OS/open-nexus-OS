// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `windowd` daemon entrypoint forwarding to CLI/runtime execution.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

fn main() {
    windowd::run();
}
