// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Embedded demo payload for process lifecycle testing
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0007-executable-payloads-architecture.md

/// Prebuilt ELF payload for the `demo.exit0` bundle.
pub const DEMO_EXIT0_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/demo-exit0.elf"));

/// Manifest used by selftests when staging `demo.exit0` through bundlemgrd.
pub const DEMO_EXIT0_MANIFEST_TOML: &str = r#"name = \"demo.exit0\"
version = \"0.0.1\"
abilities = [\"demo\"]
caps = []
min_sdk = \"0.1.0\"
publisher = \"0123456789abcdef0123456789abcdef\"
sig = \"2222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222\""#;
