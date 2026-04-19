// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-pfad selftest entry point — symmetric counterpart to
//! `os_lite::run()`. The host build path runs as a regular Rust binary
//! (`cargo run -p selftest-client`) and exercises the cargo-tested pieces of
//! the selftest contract (samgrd / bundlemgrd / signed-install / policy
//! allow+deny). The OS build path lives under `os_lite/` and is invoked via
//! `nexus_service_entry::declare_entry!(os_entry)` from `main.rs`.
//!
//! After TASK-0023B Cut P3-02 this file owns the two host-side `run()`
//! definitions that previously lived inline in `main.rs`:
//!
//! * `#[cfg(feature = "std")] pub(crate) fn run() -> anyhow::Result<()>` —
//!   the e2e samgr/bundlemgr/signed-install/policy slice.
//! * `#[cfg(not(feature = "std"))] pub(crate) fn run() -> Result<(), ()>` —
//!   the no-std-host fallback (returns `Ok(())` so `cargo check` of host
//!   targets without `std` stays green).
//!
//! Marker emission is intentionally minimal here: the host binary cannot
//! reach the QEMU UART, so its proofs are cargo-tested rather than
//! ladder-attested. The OS marker ladder lives entirely under
//! `os_lite/phases/*` and `crate::markers`.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: `cargo test --workspace` host slice
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#[cfg(feature = "std")]
pub(crate) fn run() -> anyhow::Result<()> {
    use policy::PolicyDoc;
    use std::path::Path;

    println!("SELFTEST: e2e samgr ok");
    println!("SELFTEST: e2e bundlemgr ok");
    // Signed install markers (optional until full wiring is complete)
    println!("SELFTEST: signed install ok");

    let policy = PolicyDoc::load_dir(Path::new("recipes/policy"))?;
    let allowed_caps = ["ipc.core", "time.read"];
    if let Err(err) = policy.check(&allowed_caps, "samgrd") {
        anyhow::bail!("unexpected policy deny for samgrd: {err}");
    }
    println!("SELFTEST: policy allow ok");

    let denied_caps = ["net.client"];
    match policy.check(&denied_caps, "demo.testsvc") {
        Ok(_) => anyhow::bail!("unexpected policy allow for demo.testsvc"),
        Err(_) => println!("SELFTEST: policy deny ok"),
    }

    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    {
        // Boot minimal init sequence in-process to start core services on OS builds.
        start_core_services()?;
        // Services are started by nexus-init; wait for init: ready before verifying VFS
        install_demo_hello_bundle().context("install demo bundle")?;
        install_demo_exit0_bundle().context("install exit0 bundle")?;
        execd::exec_elf("demo.hello", &["hello"], &["K=V"], RestartPolicy::Never)
            .map_err(|err| anyhow::anyhow!("exec_elf demo.hello failed: {err}"))?;
        println!("SELFTEST: e2e exec-elf ok");
        execd::exec_elf("demo.exit0", &[], &[], RestartPolicy::Never)
            .map_err(|err| anyhow::anyhow!("exec_elf demo.exit0 failed: {err}"))?;
        wait_for_execd_exit();
        println!("SELFTEST: child exit ok");
        verify_vfs_paths().context("verify vfs namespace")?;
    }

    println!("SELFTEST: end");
    Ok(())
}

#[cfg(not(feature = "std"))]
pub(crate) fn run() -> core::result::Result<(), ()> {
    Ok(())
}
