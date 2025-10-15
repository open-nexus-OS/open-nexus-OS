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
        __satp_trampoline(satp_val);
    }
    __post_satp_marker();
}
