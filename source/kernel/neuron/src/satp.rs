//! CONTEXT: SATP trampoline call island for safe page table activation
//! OWNERS: @kernel-mm-team
//! PUBLIC API: satp_switch_island(satp_val), __post_satp_marker()
//! DEPENDS_ON: arch trampoline `__satp_trampoline`
//! INVARIANTS: Minimal side-effects; emits post-switch marker; no allocation/locks
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#[no_mangle]
pub extern "C" fn __post_satp_marker() {
    log_info!(target: "as", "AS: post-satp OK");
}

#[link_section = ".text.satp"]
#[no_mangle]
pub extern "C" fn satp_switch_island(satp_val: usize) {
    unsafe {
        extern "C" {
            fn __satp_trampoline(val: usize);
        }
        log_info!(target: "as", "AS: trampoline enter val=0x{:x}", satp_val);
        __satp_trampoline(satp_val);
    }
    __post_satp_marker();
}
