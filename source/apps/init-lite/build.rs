// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build script for init-lite application
//! OWNERS: @runtime
//! STATUS: Deprecated
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - main(): Build script entry point
//!
//! DEPENDENCIES:
//!   - std::env: Environment variables
//!   - std::fs: File system operations
//!   - link.ld: Linker script
//!
//! ADR: docs/adr/0017-service-architecture.md
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    println!("cargo:rerun-if-changed=link.ld");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dst = out.join("link.ld");
    fs::copy("link.ld", &dst).expect("copy link.ld");
    println!("cargo:rustc-link-arg=-T{}", dst.display());
    // Ensure crates using check-cfg know about our custom cfg
    println!("cargo:rustc-check-cfg=cfg(nexus_env, values(\"os\"))");
    // Force OS cfg so nexus-abi exposes syscalls in this no_std binary
    println!("cargo:rustc-cfg=nexus_env=\"os\"");
}
