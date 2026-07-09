// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]

//! CONTEXT: netstackd (v0) — networking owner service for OS bring-up
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Proven via QEMU markers (TASK-0003..0005 / scripts/qemu-test.sh + tools/os2vm.sh)
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//!
//! Responsibilities (v0, Step 1):
//! - Own virtio-net + smoltcp via `userspace/nexus-net-os`.
//! - Prove the facade can do real on-wire traffic (gateway ping + UDP DNS).
//! - Export a minimal sockets facade via IPC for other services (TASK-0003).

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod os;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    // Boot determinism (soft-real-time start): netstackd is a BACKGROUND service — self-lower to Idle
    // QoS (lowering own QoS needs no privilege) so its ~1s bring-up runs AFTER the display/input
    // critical path (Normal) and never starves the first frame in the strict-priority scheduler.
    // Verdict folding: fold netstackd's bring-up markers (facade up / dhcp bound / rpc listen) into
    // one `netstackd N/N` grid line in interactive boots; flushed once the network is up, before the
    // facade serve loop (later per-RPC markers print raw). Proof boots emit everything raw.
    nexus_abi::service_verdict_arm();
    // `ready` means "the process reached entry and is alive" (see
    // emit_ready_marker) — emit it BEFORE self-lowering to Idle QoS. On the
    // strict-priority scheduler an Idle task can be starved indefinitely by
    // any Normal-QoS yield-spinner (observed: the headless proof ladder
    // failed on a missing `netstackd: ready` for exactly this ordering).
    crate::os::entry::emit_ready_marker();
    // Bring the network up at NORMAL qos FIRST — that is netstackd's actual job
    // and must finish before demoting to background. Self-lowering to Idle here
    // (as before) starved `bootstrap_network` on the strict-priority scheduler:
    // the Normal busy-spinners (display/input path) keep the Normal queue
    // non-empty, so an Idle bootstrap never runs → `net: virtio-net up` never
    // emits (headless OTA ladder stall). netstackd resumes BEFORE the display
    // drivers and `bootstrap_network` yields cooperatively, so its brief Normal
    // window can't starve the first frame.
    let crate::os::bootstrap::BootstrapResult { net, bind_ip: _bind_ip } =
        crate::os::bootstrap::bootstrap_network();
    // Network up → the steady-state facade loop is background: self-lower to Idle
    // now so per-RPC serving can never starve the display/input critical path.
    #[cfg(nexus_env = "os")]
    let _ = nexus_abi::task_qos_set_self(nexus_abi::QosClass::Idle);
    nexus_abi::service_verdict_flush("netstackd");
    crate::os::facade::runtime::run_facade_loop(net);
}

#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
fn main() -> ! {
    // Host builds intentionally do nothing for now.
    loop {
        core::hint::spin_loop();
    }
}
