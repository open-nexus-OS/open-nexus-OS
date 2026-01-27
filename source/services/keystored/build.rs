// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build script for keystored cfg validation
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0017-service-architecture.md

fn main() {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
}
