// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Search daemon entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0010-search-architecture.md

fn main() {
    search::run();
    println!("searchd: ready");
}
