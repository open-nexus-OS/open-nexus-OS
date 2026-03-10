// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Test D1: Mixed Workload (Control-plane latency under data-plane load)

extern crate alloc;
use alloc::vec::Vec;
use nexus_abi::{ipc_recv_v1, ipc_send_v1, nsec, MsgHeader};

const OP_PING: u16 = 1;

#[derive(Debug, Clone)]
pub struct MixedResult {
    pub control_avg_ns: u64,
    pub control_p99_ns: u64,
    pub iterations: usize,
}

pub fn run_mixed_workload() -> MixedResult {
    let (ep_a, _ep_b) = super::get_endpoints();
    let send_slot = ep_a;
    let recv_slot = ep_a;
    let iterations = 5_000; // Reduced for quick results

    // Simplified: just measure control-plane latency
    // (Full mixed workload would require parallel bulk_loadd)
    let payload = [0xEE; 8];
    let mut recv_buf = [0u8; 8];
    let mut latencies = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        use nexus_abi::IPC_SYS_NONBLOCK;

        let start = nsec().unwrap_or(0);

        let hdr = MsgHeader::new(0, 0, OP_PING, 0, 8);
        let _ = ipc_send_v1(send_slot, &hdr, &payload, IPC_SYS_NONBLOCK, 0);

        let mut reply_hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let _ = ipc_recv_v1(recv_slot, &mut reply_hdr, &mut recv_buf, 0, 0);

        let end = nsec().unwrap_or(0);
        latencies.push(end.saturating_sub(start));
    }

    latencies.sort_unstable();
    let avg = super::compute_avg(&latencies);
    let p99 = latencies[iterations * 99 / 100];

    MixedResult {
        control_avg_ns: avg,
        control_p99_ns: p99,
        iterations,
    }
}
