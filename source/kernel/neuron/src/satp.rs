// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SATP trampoline call island for safe page table activation
//! OWNERS: @kernel-mm-team
//! PUBLIC API: satp_switch_island(satp_val), __post_satp_marker()
//! DEPENDS_ON: arch trampoline `__satp_trampoline`
//! INVARIANTS: Minimal side-effects; emits post-switch marker; no allocation/locks
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use core::fmt::Write;

#[no_mangle]
pub extern "C" fn __post_satp_marker() {
    const LOG_LIMIT: usize = 8;
    static LOG_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    if LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) >= LOG_LIMIT {
        return;
    }
    // CRITICAL: After satp switch, NO heap allocation allowed!
    // Use raw UART only (no formatting that might allocate)
    let mut u = crate::uart::raw_writer();
    let _ = u.write_str("AS: post-satp OK\n");
}

#[link_section = ".text.satp"]
#[no_mangle]
pub extern "C" fn satp_switch_island(satp_val: usize) {
    // Log BEFORE satp switch (safe to use formatting here) – rate limited
    const LOG_LIMIT: usize = 8;
    static LOG_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    if LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < LOG_LIMIT {
        log_info!(target: "as", "AS: trampoline enter val=0x{:x}", satp_val);
    }
    unsafe {
        extern "C" {
            fn __satp_trampoline(val: usize);
        }
        __satp_trampoline(satp_val);
    }
    // CRITICAL: After __satp_trampoline, we're in a different AS!
    __post_satp_marker();
}
