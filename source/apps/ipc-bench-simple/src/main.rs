// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Simple IPC benchmark - measures loopback IPC latency

#![cfg_attr(target_arch = "riscv64", no_std, no_main)]

#[cfg(target_arch = "riscv64")]
extern crate alloc;

#[cfg(target_arch = "riscv64")]
nexus_service_entry::declare_entry!(bench_entry);

#[cfg(target_arch = "riscv64")]
fn bench_entry() -> core::result::Result<(), ()> {
    use alloc::vec::Vec;
    use nexus_abi::{debug_putc, ipc_endpoint_create_v2, ipc_send_v1, ipc_recv_v1, nsec, MsgHeader, IPC_SYS_NONBLOCK};

    fn print(s: &str) {
        for b in s.bytes() {
            let _ = debug_putc(b);
        }
    }

    fn print_u64(n: u64) {
        let mut buf = [0u8; 20];
        let mut pos = 0;
        let mut num = n;

        if num == 0 {
            let _ = debug_putc(b'0');
            return;
        }

        while num > 0 {
            buf[pos] = b'0' + (num % 10) as u8;
            num /= 10;
            pos += 1;
        }

        for i in (0..pos).rev() {
            let _ = debug_putc(buf[i]);
        }
    }

    print("BENCH: simple ipc benchmark starting\n");

    // Create endpoint using factory in slot 1
    let ep = match ipc_endpoint_create_v2(1, 4) {
        Ok(slot) => {
            print("BENCH: endpoint created in slot ");
            print_u64(slot as u64);
            print("\n");
            slot
        }
        Err(_) => {
            print("BENCH: ERROR - failed to create endpoint\n");
            return Err(());
        }
    };

    // Measure loopback latency
    let iterations = 10_000;
    let payload_size = 64;
    let mut send_buf = Vec::with_capacity(payload_size);
    send_buf.resize(payload_size, 0xAA);
    let mut recv_buf = Vec::with_capacity(payload_size);
    recv_buf.resize(payload_size, 0);

    // Warmup
    for _ in 0..100 {
        let hdr = MsgHeader::new(0, 0, 1, 0, payload_size as u32);
        let _ = ipc_send_v1(ep, &hdr, &send_buf, IPC_SYS_NONBLOCK, 0);
        let mut rhdr = MsgHeader::new(0, 0, 0, 0, 0);
        let _ = ipc_recv_v1(ep, &mut rhdr, &mut recv_buf, 0, 0);
    }

    // Measure
    let mut latencies = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = nsec().unwrap_or(0);

        let hdr = MsgHeader::new(0, 0, 1, 0, payload_size as u32);
        let _ = ipc_send_v1(ep, &hdr, &send_buf, IPC_SYS_NONBLOCK, 0);
        let mut rhdr = MsgHeader::new(0, 0, 0, 0, 0);
        let _ = ipc_recv_v1(ep, &mut rhdr, &mut recv_buf, 0, 0);

        let end = nsec().unwrap_or(0);
        latencies.push(end.saturating_sub(start));
    }

    // Compute stats
    latencies.sort_unstable();
    let sum: u64 = latencies.iter().sum();
    let avg = sum / iterations as u64;
    let p50 = latencies[iterations / 2];
    let p90 = latencies[iterations * 90 / 100];
    let p99 = latencies[iterations * 99 / 100];
    let min = latencies[0];
    let max = latencies[iterations - 1];

    // Output results
    print("\n=== IPC LOOPBACK LATENCY (64B payload) ===\n");
    print("Iterations: ");
    print_u64(iterations as u64);
    print("\nAvg: ");
    print_u64(avg);
    print(" ns\nP50: ");
    print_u64(p50);
    print(" ns\nP90: ");
    print_u64(p90);
    print(" ns\nP99: ");
    print_u64(p99);
    print(" ns\nMin: ");
    print_u64(min);
    print(" ns\nMax: ");
    print_u64(max);
    print(" ns\n\nSELFTEST: bench ok\n");

    Ok(())
}

#[cfg(not(target_arch = "riscv64"))]
fn main() {
    println!("ipc-bench-simple: host build not supported (OS-only)");
}
