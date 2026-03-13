#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus daemon entrypoint (os-lite)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Proven via QEMU markers (TASK-0003..0005 / scripts/qemu-test.sh + tools/os2vm.sh)
//!
//! SECURITY INVARIANTS:
//! - No network capability transfer: remote proxy forwards bounded request/response bytes only.
//! - Remote proxy is deny-by-default (explicit allowlist).
//! - No secrets (keys/session material) are logged to UART.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod os;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> core::result::Result<(), ()> {
    use nexus_ipc::reqrep::ReplyBuffer;

    // dsoftbusd must NOT own MMIO; it uses netstackd's IPC facade.
    // Wait for init-lite to finish transferring capability slots before proceeding.
    os::entry::wait_for_slots_ready();
    let net = os::entry::init_netstack_client()?;

    let mut nonce_ctr: u64 = 1;
    // Shared reply inbox correlation: keep a bounded buffer of unmatched netstackd replies keyed by nonce.
    // This prevents silent drops when multiple netstackd ops share one reply inbox.
    let mut pending_replies: ReplyBuffer<16, 512> = ReplyBuffer::new();

    // Wait for netstackd to finish IPv4 configuration (DHCP or deterministic static fallback).
    let local_ip = os::entry::resolve_local_ip_with_wait(&mut pending_replies, &net, &mut nonce_ctr);
    let is_cross_vm = os::entry::is_cross_vm_ip(local_ip);
    if is_cross_vm {
        // Cross-VM mode (TASK-0005 / RFC-0010): real UDP datagrams + TCP sessions across two QEMU instances.
        // This path is opt-in via the 2-VM harness (socket/mcast backend) and MUST remain deterministic.
        cross_vm_main(&net, local_ip)?;
        return Ok(());
    }

    // UDP discovery socket bind (Phase 1): bind to 0.0.0.0:<port>.
    let disc_port: u16 = 37_020;
    let udp_id =
        os::entry::bind_discovery_udp_with_wait(&mut pending_replies, &net, &mut nonce_ctr, disc_port);

    let port: u16 = 34_567;
    let lid = os::session::single_vm::run_single_vm_dual_node_bringup(
        &mut pending_replies,
        &net,
        &mut nonce_ctr,
        udp_id,
        disc_port,
        port,
    )?;
    os::session::selftest_server::run_selftest_server_loop(
        &mut pending_replies,
        &net,
        &mut nonce_ctr,
        lid,
        port,
    );
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn cross_vm_main(net: &nexus_ipc::KernelClient, local_ip: [u8; 4]) -> core::result::Result<(), ()> {
    os::session::cross_vm::run_cross_vm_main(net, local_ip)
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    dsoftbus::run();
    loop {
        core::hint::spin_loop();
    }
}
