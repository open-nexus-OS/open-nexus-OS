// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Media service entrypoint – wires service logic to CLI/OS entry
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (lib) + 1 integration test (tests/cli.rs)
//! ADR: docs/adr/0017-service-architecture.md
fn main() {
    media::run();
}
