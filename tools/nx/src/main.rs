// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Thin binary entrypoint for the canonical host-first `nx` CLI.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests; covered transitively by 17 unit tests and 6 integration tests in the `nx` crate.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

fn main() {
    std::process::exit(nx::run());
}
