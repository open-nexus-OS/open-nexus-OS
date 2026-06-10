// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Compositor daemon entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 unit tests (lib) + 1 integration test (tests/headless.rs)
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md
fn main() {
    compositor::run();
}
