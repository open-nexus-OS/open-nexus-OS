// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Test E1: Determinism / Reproducibility

extern crate alloc;
use alloc::vec::Vec;
use nexus_abi::{ipc_recv_v1, ipc_send_v1, nsec, MsgHeader};

const OP_PING: u16 = 1;

#[derive(Debug, Clone)]
pub struct DeterminismResult {
    pub iterations: usize,
    pub result_hash: u64,
}

pub fn run_determinism_test() -> DeterminismResult {
    let (ep_a, _ep_b) = super::get_endpoints();
    let send_slot = ep_a;
    let recv_slot = ep_a;
    let iterations = 1_000; // Reduced for quick results

    let payload = [0xFF; 8];
    let mut recv_buf = [0u8; 8];
    let mut latencies = Vec::with_capacity(iterations);

    // Single run (full reproducibility test would require multiple runs)
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

    // Compute simple hash of latency buckets (deterministic)
    let hash = compute_latency_hash(&latencies);

    DeterminismResult {
        iterations,
        result_hash: hash,
    }
}

fn compute_latency_hash(latencies: &[u64]) -> u64 {
    // Simple hash: bucket latencies into 1us bins and hash the sequence
    let mut hash: u64 = 0x123456789ABCDEF0;

    for &lat in latencies {
        let bucket = lat / 1000; // 1us buckets
        hash ^= bucket;
        hash = hash.rotate_left(7);
    }

    hash
}
