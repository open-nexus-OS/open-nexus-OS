// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Time sync daemon entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (via userspace/time_sync)
//! ADR: docs/adr/0012-time-sync-architecture.md

fn main() {
    time_sync::run();
    println!("time-syncd: ready");
}
