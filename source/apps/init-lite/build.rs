// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build script for the init-lite binary. init links like every other OS service
//! (the workspace's target flags: every PT_LOAD on its own file page) and carries no program:
//! the core services are files of the bootfs the kernel hands it (ADR-0081). This script only
//! sets the `nexus_env = "os"` cfg for the OS target and forwards the log/probe knobs.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - main(): Build script entry point
//!
//! ADR: docs/adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md

fn main() {
    // IMPORTANT: init-lite is an OS-only binary; however, `cargo test --workspace` builds and
    // *runs* a host test harness for every bin target, so the OS cfg is set only when the OS
    // target is actually being built.
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_arch == "riscv64" && target_os == "none" {
        println!("cargo:rustc-check-cfg=cfg(nexus_env, values(\"os\"))");
        println!("cargo:rustc-cfg=nexus_env=\"os\"");
    }
    println!("cargo:rerun-if-env-changed=INIT_LITE_LOG_TOPICS");
    match std::env::var("INIT_LITE_LOG_TOPICS") {
        Ok(spec) => println!("cargo::rustc-env=INIT_LITE_LOG_TOPICS={spec}"),
        Err(_) => println!("cargo::rustc-env=INIT_LITE_LOG_TOPICS="),
    }
    println!("cargo:rerun-if-env-changed=INIT_LITE_FORCE_PROBE");
    match std::env::var("INIT_LITE_FORCE_PROBE") {
        Ok(val) => println!("cargo::rustc-env=INIT_LITE_FORCE_PROBE={val}"),
        Err(_) => println!("cargo::rustc-env=INIT_LITE_FORCE_PROBE="),
    }
}
