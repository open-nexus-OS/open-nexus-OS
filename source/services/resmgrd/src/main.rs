// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Resource manager daemon entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (via userspace/resmgr)
//! ADR: docs/adr/0015-resource-manager-architecture.md

fn main() {
    resmgr::run();
    println!("resmgrd: ready");
}
