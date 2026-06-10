// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Ability manager daemon entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 unit tests (lib) + 1 integration test (tests/cli.rs)
//! ADR: docs/adr/0017-service-architecture.md
fn main() {
    abilitymgr::run();
}
