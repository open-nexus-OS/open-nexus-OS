// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! IPC Benchmark Server - Deterministic echo server for IPC evaluation

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]
#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    os_lite::run()
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod os_lite {
    extern crate alloc;
    use alloc::vec::Vec;
    use nexus_abi::{ipc_recv_v1, ipc_send_v1, MsgHeader};

    const OP_PING: u16 = 1;
    const OP_BULK_META: u16 = 2;
    const OP_STOP: u16 = 3;

    pub fn run() -> Result<(), ()> {
        // Pre-allocate buffer for max frame size (8KB)
        let mut recv_buf = Vec::with_capacity(8192);
        recv_buf.resize(8192, 0u8);

        // Assume init-lite transferred endpoint caps to slots 3 (RECV) and 4 (SEND)
        let recv_slot = 3;
        let send_slot = 4;

        loop {
            // Blocking receive
            let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
            match ipc_recv_v1(recv_slot, &mut hdr, &mut recv_buf, 0, 0) {
                Ok(len) => {
                    match hdr.ty {
                        OP_PING => {
                            // Echo reply immediately
                            let reply_hdr = MsgHeader::new(0, 0, OP_PING, 0, len as u32);
                            let _ = ipc_send_v1(send_slot, &reply_hdr, &recv_buf[..len], 0, 0);
                        }
                        OP_BULK_META => {
                            // VMO bulk test: parse metadata, map, compute checksum, reply
                            // For now: simple ack
                            let ack_hdr = MsgHeader::new(0, 0, OP_BULK_META, 0, 8);
                            let ack_payload = [0xAC, 0x00, 0, 0, 0, 0, 0, 0];
                            let _ = ipc_send_v1(send_slot, &ack_hdr, &ack_payload, 0, 0);
                        }
                        OP_STOP => {
                            // Graceful shutdown
                            break;
                        }
                        _ => {
                            // Unknown op, ignore
                        }
                    }
                }
                Err(_) => {
                    // Error receiving, continue
                }
            }
        }

        Ok(())
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite")))]
fn main() {
    println!("ipc-bench-server: host build not supported (OS-only)");
}
