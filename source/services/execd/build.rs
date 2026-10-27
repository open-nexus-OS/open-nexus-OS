// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build script — the `nexus_env` check-cfg line only. The app → caps table it used to
//! generate from the manifests is gone (RFC-0111 §6, ADR-0084): the grants execd provisions
//! routes for are the permission authority's answer at spawn (`os_lite/grants.rs`).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (build script)

fn main() {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
}
